#!/usr/bin/env python3
"""The 20 task questions of the independent benchmark, generated from LSP gold and the repos' own fix history.

usage: questions.py SELECTION_JSON GOLD_DIR CORPORA_DIR OUT_JSON [--md OUT_MD]
Rules (see questions.md): 7 + 7 + 6 questions over the three selected repos. Target-based questions draw from the
LSP gold targets in a seeded order. Concept questions come from commits before the pinned tag whose subject
references an issue or pull request (`#123`) and names a fix; the question is the cleaned subject and the facts are
the source files that commit changed which still exist at the tag. No benchmarked tool is run here.
"""
import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from gold_lsp import LANG, TESTY  # noqa: E402

PLAN = {0: ["def", "callers", "callers", "callees", "outline", "impact", "concept"],
        1: ["def", "callers", "callers", "callees", "outline", "impact", "concept"],
        2: ["def", "callers", "callers", "callees", "outline", "concept"]}
REF = re.compile(r"(?:\(\s*)?(?:#|gh-)\d+(?:\s*\))?", re.I)
VERB = re.compile(r"^(fix(e[sd])?|close[sd]?|resolve[sd]?)\b\s*:?\s*", re.I)
TAIL = re.compile(r"[,;]?\s*\b(fix(e[sd])?|close[sd]?|resolve[sd]?)\s*$", re.I)
PREFIX = re.compile(r"^\w+(\([^)]*\))?!?:\s*")
CAP = 10


def clean_subject(s):
    """Question words from a fix commit subject that references an issue/PR, or None."""
    s = s.strip()
    if not REF.search(s) or re.match(r"(?i)(merge|revert)\b", s):
        return None
    s = REF.sub(" ", s)
    s = PREFIX.sub("", s.strip())
    s = VERB.sub("", s.strip())
    s = TAIL.sub("", s.strip())
    s = " ".join(s.split()).strip(" :,;.-")
    return s if len(s.split()) >= 3 else None


def git(root, *args):
    return subprocess.run(["git", "-C", root] + list(args), capture_output=True, text=True, check=True).stdout


def concept_candidates(root, tag, lang, seed, name, want=10, scan=3000):
    """Fix commits before `tag` touching 1-3 non-test source files that exist at `tag`, in seeded order."""
    at_tag = set(git(root, "ls-tree", "-r", "--name-only", tag).splitlines())
    commits = [line.split("\x1f", 1) for line in git(root, "log", "--no-merges", "--format=%H\x1f%s", tag).splitlines() if "\x1f" in line]
    commits = [(h, s, clean_subject(s)) for h, s in commits]
    commits = [c for c in commits if c[2]]
    random.Random(int(hashlib.sha256(f"{seed}:{name}:concepts".encode()).hexdigest(), 16)).shuffle(commits)
    out = []
    for h, subj, words in commits[:scan]:
        fs = [f for f in git(root, "show", "--name-only", "--no-renames", "--format=", h).splitlines()
              if f.endswith(LANG[lang]["exts"]) and not TESTY.search(f) and f in at_tag]
        if 1 <= len(fs) <= 3:
            out.append({"words": words, "files": sorted(set(fs)), "commit": h, "subject": subj})
            if len(out) >= want:
                break
    return out


def label(d):
    return f"{d['container']}.{d['name']}" if d.get("container") else d["name"]


def caller_facts(sites):
    by = {}
    for s in sites:
        by.setdefault(s["file"], set()).add(s["line"])
    order = sorted(by, key=lambda f: (-len(by[f]), f))
    return [["caller", [f, sorted(by[f])]] for f in order[:CAP]]


def uniq(xs):
    seen, out = set(), []
    for x in xs:
        if x not in seen:
            seen.add(x)
            out.append(x)
    return out


FITS = {"def": lambda t, g: True, "callers": lambda t, g: bool(t["callers"]), "callees": lambda t, g: bool(t["callees"]),
        "outline": lambda t, g: len(g["outlines"].get(t["def"]["file"], [])) >= 2, "impact": lambda t, g: bool(t["callers2"])}


def make(kind, t, g):
    d = t["def"]
    if kind == "def":
        return f"Where is `{label(d)}` defined?", [["loc", [d["file"], d["line"]]]]
    if kind == "callers":
        return (f"Which functions call `{label(d)}` (defined in {d['file']})? Give the file and line of each call.",
                caller_facts(t["callers"]))
    if kind == "callees":
        return (f"Which functions of this repository does `{label(d)}` ({d['file']}) call?",
                [["word", n] for n in uniq(c["name"] for c in t["callees"])[:CAP]])
    if kind == "outline":
        return f"List the top-level definitions in {d['file']}.", [["word", n] for n in uniq(g["outlines"][d["file"]])[:12]]
    return (f"If `{label(d)}` ({d['file']}) changes, which files are affected through its callers, up to two levels?",
            [["path", f] for f in uniq([s["file"] for s in t["callers"]] + [s["file"] for s in t["callers2"]])[:CAP]])


def build(repo, idx, gold, concepts, seed):
    """Questions for the repo at position idx of the selection; a slot no target fits is left out (and reported)."""
    pool = list(gold["targets"])
    random.Random(int(hashlib.sha256(f"{seed}:{repo}:questions".encode()).hexdigest(), 16)).shuffle(pool)
    used, cons, qs = set(), list(concepts), []
    for n, kind in enumerate(PLAN[idx]):
        qid = f"{repo}-{n + 1:02d}-{kind}"
        if kind == "concept":
            if not cons:
                continue
            c = cons.pop(0)
            qs.append({"id": qid, "repo": repo, "kind": kind, "q": f"Where is the code for: {c['words']}?", "words": c["words"],
                       "facts": [["path", f] for f in c["files"]], "provenance": {"commit": c["commit"], "subject": c["subject"]}})
            continue
        t = next((t for i, t in enumerate(pool) if i not in used and FITS[kind](t, gold)), None)
        if t is None:
            continue
        used.add(pool.index(t))
        text, facts = make(kind, t, gold)
        q = {"id": qid, "repo": repo, "kind": kind, "q": text, "t": t, "facts": facts, "provenance": {"gold": t["source"]}}
        if kind == "outline":
            q["file"] = t["def"]["file"]
        qs.append(q)
    return qs


def render_md(sel, qs):
    out = [f"# Independent benchmark: generated questions (seed `{sel['seed']}`)", "",
           "Generated by questions.py from LSP gold and fix-commit history; no benchmarked tool was run.", ""]
    for q in qs:
        out.append(f"- **{q['id']}** {q['q']}  ")
        out.append(f"  facts: {len(q['facts'])}; source: {json.dumps(q['provenance'])}")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("gold")
    ap.add_argument("corpora")
    ap.add_argument("out")
    ap.add_argument("--md")
    a = ap.parse_args()
    sel = json.load(open(a.selection))
    allq = []
    for idx, s in enumerate(sel["selection"]):
        g = json.load(open(os.path.join(a.gold, f"{s['name']}.json")))
        cons = concept_candidates(os.path.join(a.corpora, s["name"]), s["sha"] or s["tag"], s["lang"], sel["seed"], s["name"])
        qs = build(s["name"], idx, g, cons, sel["seed"])
        print(f"questions {s['name']}: {len(qs)} of {len(PLAN[idx])} ({len(cons)} concept candidates)", flush=True)
        allq += qs
    json.dump({"seed": sel["seed"], "questions": allq}, open(a.out, "w"), indent=1)
    if a.md:
        open(a.md, "w").write(render_md(sel, allq))
    print(f"QUESTIONS {len(allq)} of 20 -> {a.out}")


if __name__ == "__main__":
    main()
