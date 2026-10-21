#!/usr/bin/env python3
"""Phase 6: task-level information efficiency (10 agent questions per corpus, fixed command sequences).

usage: phase_tasks.py CORPUS [CORPUS...]
Facts are derived from the tool-independent oracle (goldlib) and scored by presence in the
concatenated outputs (JSON outputs are also searched after decoding their string values).
Score per question: 2 if >=80% of facts present, 1 if any, 0 otherwise.
"""
import collections
import json
import os
import random
import re
import sys
import zlib

from benchlib import W, WORK, RES, PRISTINE, run, as_bench, append_jsonl, loadavg
from goldlib import Corpus, KEYWORDS, STD_METHODS, lang_of

TESTY = re.compile(r"(^|/)(tests?|testdata|testing|fixtures?|benches|examples?|docs?)(/|$)|_test\.|(^|/)test_|\.test\.|\.spec\.|conftest")
ENV_RE = [
    re.compile(r"""environ(?:\.get)?\s*[\[(]\s*["']([A-Z][A-Z0-9_]{3,})["']"""),
    re.compile(r"""getenv\s*\(\s*["']([A-Z][A-Z0-9_]{3,})["']"""),
    re.compile(r"""env::var(?:_os)?\s*\(\s*["']([A-Z][A-Z0-9_]{3,})["']"""),
    re.compile(r"""Getenv\s*\(\s*["']([A-Z][A-Z0-9_]{3,})["']"""),
    re.compile(r"""process\.env\.([A-Z][A-Z0-9_]{3,})"""),
    re.compile(r"""config\[\s*["']([A-Z][A-Z0-9_]{3,})["']\s*\]"""),
]


def strings_of(obj, acc):
    if isinstance(obj, str):
        acc.append(obj)
    elif isinstance(obj, dict):
        for v in obj.values():
            strings_of(v, acc)
    elif isinstance(obj, list):
        for v in obj:
            strings_of(v, acc)


def search_text(outs):
    parts = []
    for o in outs:
        s = o.decode("utf-8", "replace")
        parts.append(s)
        try:
            acc = []
            strings_of(json.loads(s), acc)
            parts.extend(acc)
            parts.append(s.replace("\\n", "\n"))
        except ValueError:
            pass
    return "\n".join(parts)


def fact_present(text, fact):
    kind, val = fact
    if kind == "word":
        return re.search(r"(?<![\w])" + re.escape(val) + r"(?![\w])", text) is not None
    if kind == "path":
        return val in text
    if kind == "loc":
        path, line = val
        if path not in text:
            return False
        return any(re.search(r"(?<!\d)" + str(n) + r"(?!\d)", text) for n in range(line - 3, line + 4))
    if kind == "caller":
        path, lines = val
        return path in text and any(re.search(r"(?<!\d)" + str(n) + r"(?!\d)", text) for n in lines)
    if kind == "line":
        return re.search(r"(?<!\d)" + str(val) + r"(?!\d)", text) is not None
    if kind == "text":
        return " ".join(val.split()) in " ".join(text.split())
    return False


def body_range(c, d):
    defs = sorted(x.line for x in c.defs if x.file == d.file and x.line > d.line)
    end = defs[0] - 1 if defs else len(c.cleaned[d.file])
    return d.line, min(end, d.line + 80)


def callees_of(c, d):
    a, b = body_range(c, d)
    names = []
    for line in c.cleaned[d.file][a:b]:
        for m in re.finditer(r"\b([A-Za-z_]\w*)\s*\(", line):
            n = m.group(1)
            if n not in KEYWORDS and n not in STD_METHODS and len(n) > 2 and n != d.name and n not in names:
                if c.by_name.get(n):
                    names.append(n)
    return names[:10]


def public_defs(c, rel):
    lang = lang_of(rel)
    out = []
    for d in c.defs:
        if d.file != rel or d.kind not in ("function", "class", "interface") or d.container:
            continue
        raw = c.raw[rel][d.line - 1]
        if lang == "py" and not d.name.startswith("_"):
            out.append(d.name)
        elif lang == "rs" and re.match(r"^\s*pub\b", raw):
            out.append(d.name)
        elif lang == "go" and d.name[:1].isupper():
            out.append(d.name)
        elif lang == "ts" and re.match(r"^\s*export\b", raw):
            out.append(d.name)
    return out


def build_questions(corpus, c, targets):
    rng = random.Random(zlib.crc32(("tasks" + corpus).encode()))
    by_cat = collections.defaultdict(list)
    for t in targets:
        by_cat[t["category"]].append(t)
    hub = next((t for t in by_cat["hub"] if t["def"]["kind"] == "function"), by_cat["hub"][0])
    rare = by_cat["rare"] or by_cat["hub"]
    recv = (by_cat["receiver"] or by_cat["hub"])[0]
    coll = (by_cat["collision"] or by_cat["hub"])[0]
    D = lambda t: next(d for d in c.defs if d.file == t["def"]["file"] and d.line == t["def"]["line"])

    def caller_facts(t):
        """One fact per calling file: the file path plus one of its gold call-site line numbers."""
        lines = collections.defaultdict(list)
        for s in t["gold"]["assigned"]:
            lines[s["file"]].append(s["line"])
        f = [("caller", (p, sorted(ls))) for p, ls in sorted(lines.items())]
        return f[:12]

    qs = []
    qs.append({"id": "Q1", "kind": "def_and_callers", "q": f"Where is {hub['def']['name']} defined and who calls it?",
               "t": hub, "facts": [("loc", (hub["def"]["file"], hub["def"]["line"]))] + caller_facts(hub)})
    r0 = rare[0]
    qs.append({"id": "Q2", "kind": "impact", "q": f"What breaks if I change the signature of {r0['def']['name']}?",
               "t": r0, "facts": [("loc", (r0["def"]["file"], r0["def"]["line"]))] + caller_facts(r0)})
    F = hub["def"]["file"]
    top = [d.name for d in c.defs if d.file == F and d.kind in ("function", "class", "interface", "method")]
    qs.append({"id": "Q3", "kind": "outline", "q": f"Outline the file {F}.", "file": F,
               "facts": [("word", n) for n in list(dict.fromkeys(top))[:25]]})
    # config key read most often in non-test code
    cnt = collections.Counter()
    where = collections.defaultdict(set)
    for rel, lines in c.raw.items():
        if TESTY.search(rel):
            continue
        for line in lines:
            for rx in ENV_RE:
                for m in rx.finditer(line):
                    cnt[m.group(1)] += 1
                    where[m.group(1)].add(rel)
    if cnt:
        key = sorted(cnt.items(), key=lambda kv: (-len(where[kv[0]]), -kv[1], kv[0]))[0][0]
        qs.append({"id": "Q4", "kind": "config", "q": f"Where is the config value {key} read?", "key": key,
                   "facts": [("path", p) for p in sorted(where[key])[:12]]})
    cands = []
    for rel in c.cleaned:
        if TESTY.search(rel):
            continue
        pub = public_defs(c, rel)
        if 5 <= len(pub) <= 25:
            cands.append((rel, pub))
    cands.sort()
    if cands:
        rel, pub = rng.choice(cands)
        qs.append({"id": "Q5", "kind": "exports", "q": f"What does module {rel} export?", "file": rel,
                   "facts": [("word", n) for n in pub]})
    d = D(recv)
    a, b = body_range(c, d)
    body = [l.strip() for l in c.raw[d.file][a - 1:b] if l.strip()]
    qs.append({"id": "Q6", "kind": "source", "q": f"Show me the implementation of {d.name}.", "t": recv,
               "facts": [("text", x) for x in body[:1] + body[1:4:2]]})
    ct = hub
    for cand in by_cat["hub"] + by_cat["receiver"] + by_cat["rare"] + by_cat["collision"]:
        if len(callees_of(c, D(cand))) >= 2:
            ct = cand
            break
    hd = D(ct)
    qs.append({"id": "Q7", "kind": "callees", "q": f"What does {hd.name} call?", "t": ct,
               "facts": [("word", n) for n in callees_of(c, hd)]})
    tops = collections.Counter(f.split("/")[0] for f in c.cleaned if "/" in f)
    qs.append({"id": "Q8", "kind": "overview", "q": "Give me an overview of the repository layout.",
               "facts": [("word", n) for n, k in tops.most_common(10) if k >= 2]})
    cd = coll["def"]
    qual = f"{cd['container']}.{cd['name']}" if cd["container"] else cd["name"]
    qs.append({"id": "Q9", "kind": "collision", "q": f"Where is {qual} defined and who calls that specific one?",
               "t": coll, "qual": qual,
               "facts": [("loc", (cd["file"], cd["line"]))] + caller_facts(coll)})
    r1 = rare[1] if len(rare) > 1 else rare[0]
    words = " ".join(w for w in re.split(r"_|(?<=[a-z])(?=[A-Z])", r1["def"]["name"]) if w).lower()
    qs.append({"id": "Q10", "kind": "concept", "q": f"Find the code that handles: {words}", "words": words, "t": r1,
               "facts": [("loc", (r1["def"]["file"], r1["def"]["line"]))]})
    return qs


def dctx_find_id(rows_out, t):
    try:
        recs = json.loads(rows_out).get("records", [])
    except ValueError:
        return None
    for rec in recs:
        lines = rec.get("lines") or [0, 0]
        if rec.get("path") == t["def"]["file"] and lines[0] - 3 <= t["def"]["line"] <= lines[1]:
            return rec["id"]
    return recs[0]["id"] if recs else None


def run_dctx(corpus, q, d, env):
    outs, calls, labels = [], 0, []

    def go(cmd, label):
        nonlocal calls
        calls += 1
        r = run(cmd, d, tool="dctx", corpus=corpus, label=f"task_{q['id']}_{label}", extra_env=env,
                log="task_runs.jsonl")
        outs.append(r["_stdout"])
        labels.append(label)
        return r

    k = q["kind"]
    if k in ("def_and_callers", "impact", "source", "callees", "collision"):
        t = q["t"]
        r = go(["dctx", "map", "find", t["def"]["name"], "--json", "--limit", "10"], "find")
        did = dctx_find_id(r["_stdout"], t)
        if did:
            if k == "def_and_callers" or k == "collision":
                go(["dctx", "map", "neighbors", did, "--dir", "in", "--edge", "calls"], "callers")
            elif k == "impact":
                go(["dctx", "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--depth", "2"], "impact")
            elif k == "source":
                go(["dctx", "map", "read", did, "--json"], "read")
            elif k == "callees":
                go(["dctx", "map", "neighbors", did, "--dir", "out", "--edge", "calls"], "callees")
    elif k in ("outline", "exports"):
        go(["dctx", "map", "map", q["file"], "--depth", "2", "--limit", "40"], "map")
    elif k == "config":
        go(["dctx", "map", "find", q["key"], "--limit", "10"], "find")
    elif k == "overview":
        go(["dctx", ".", "-t"], "tree")
        outs.append(as_bench(f"cat '{d}'/.dctx/*.llmcontext.txt").encode())
        labels.append("tree_file")
        as_bench(f"rm -rf '{d}/.dctx'")
    elif k == "concept":
        go(["dctx", "map", "find", q["words"], "--limit", "10"], "find")
    return outs, calls, labels


def run_trail(corpus, q, d):
    outs, calls = [], 0

    def go(cmd, label):
        nonlocal calls
        calls += 1
        r = run(cmd, d, tool="trail", corpus=corpus, label=f"task_{q['id']}_{label}", log="task_runs.jsonl")
        outs.append(r["_stdout"])
        return r

    k = q["kind"]
    if k == "def_and_callers":
        go(["trail", "callers", q["t"]["def"]["name"]], "callers")
    elif k == "impact":
        go(["trail", "callers", q["t"]["def"]["name"], "-d", "2"], "callers_d2")
    elif k in ("outline", "exports"):
        go(["trail", "skeleton", q["file"]], "skeleton")
    elif k == "config":
        go(["trail", "grep", q["key"]], "grep")
    elif k == "source":
        go(["trail", "ask", q["t"]["def"]["name"], "--source", "--full", "-n", "3"], "ask_source")
    elif k == "callees":
        go(["trail", "callers", q["t"]["def"]["name"], "--direction", "out"], "callees")
    elif k == "overview":
        go(["trail", "map"], "map")
    elif k == "collision":
        go(["trail", "callers", q["qual"]], "callers_qualified")
    elif k == "concept":
        go(["trail", "ask", q["words"]], "ask")
    return outs, calls, ["trail"] * len(outs)


def main(corpus):
    c = Corpus(os.path.join(PRISTINE, corpus))
    with open(os.path.join(RES, "targets", f"{corpus}.targets.json")) as f:
        targets = json.load(f)["targets"]
    qs = build_questions(corpus, c, targets)
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    for q in qs:
        for tool in ("dctx", "trail"):
            d = os.path.join(WORK, tool, corpus)
            outs, calls, labels = run_dctx(corpus, q, d, env) if tool == "dctx" else run_trail(corpus, q, d)
            text = search_text(outs)
            # caller facts must come from a caller/impact answer, not from an incidental find listing
            ctext = search_text([o for o, lab in zip(outs, labels) if lab != "find"])
            present = [fact_present(ctext if f[0] == "caller" else text, f) for f in q["facts"]]
            nbytes = sum(len(o) for o in outs)
            frac = (sum(present) / len(present)) if present else 0.0
            score = 2 if frac >= 0.8 else (1 if frac > 0 else 0)
            row = {"corpus": corpus, "qid": q["id"], "kind": q["kind"], "question": q["q"], "tool": tool,
                   "calls": calls, "bytes": nbytes, "est_tokens": -(-nbytes * 2 // 7),
                   "facts_n": len(present), "facts_found": sum(present), "fraction": round(frac, 3),
                   "score": score,
                   "missing": [f for f, p in zip(q["facts"], present) if not p][:12]}
            append_jsonl("tasks.jsonl", row)
    with open(os.path.join(RES, "targets", f"{corpus}.questions.json"), "w") as f:
        json.dump([{k: v for k, v in q.items() if k != "t"} | {"target": q["t"]["def"]["name"]} if "t" in q else q
                   for q in qs], f, indent=1)
    print("done", corpus, flush=True)


if __name__ == "__main__":
    for cc in sys.argv[1:]:
        append_jsonl("phases.jsonl", {"phase": "tasks", "corpus": cc, "event": "start", "load1": loadavg()})
        main(cc)
        append_jsonl("phases.jsonl", {"phase": "tasks", "corpus": cc, "event": "end", "load1": loadavg()})
