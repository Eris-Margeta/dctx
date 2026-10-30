#!/usr/bin/env python3
"""October scorer for the task-level benchmark (70 agent questions), unchanged in logic.

Copied verbatim (strings_of, search_text, fact_present, the 2/1/0 rule) from
docs/research/trail-code-2026-10/scripts/phase_tasks.py so the three-way rerun is scored exactly as in October:

* facts are scored by presence in the concatenated tool outputs of one question
  (JSON string values are also searched after decoding);
* a "caller" fact needs the caller's path and one of its call-line numbers and must come from an answer that is
  not the id-resolution ("find") step;
* score per question = 2 if >= 80% of the facts are present, 1 if any, 0 otherwise;
* tokens = ceil(2 * bytes / 7) over every byte the question's commands printed on stdout.

Also holds the aggregation used for the README (python3 tasks_score.py RESULTS_DIR [--sanitize]).
"""
import collections
import json
import math
import os
import re
import sys

KIND_ORDER = ["def_and_callers", "impact", "outline", "config", "exports", "source", "callees", "overview",
              "collision", "concept"]
CORPUS_ORDER = ["flask", "ripgrep", "pocketbase", "trailcode", "dctx", "django", "core"]
TOOL_ORDER = ["dctx", "trail", "graphify", "dctx-ext"]


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


def est_tokens_total(nbytes):
    return -(-nbytes * 2 // 7)  # = ceil(bytes / 3.5), the October estimator


def score_question(facts, outs, labels):
    """facts: list of (kind, value); outs: list of bytes; labels: parallel list, "find" = id-resolution step."""
    text = search_text(outs)
    ctext = search_text([o for o, lab in zip(outs, labels) if lab != "find"])
    present = [fact_present(ctext if f[0] == "caller" else text, f) for f in facts]
    frac = (sum(present) / len(present)) if present else 0.0
    score = 2 if frac >= 0.8 else (1 if frac > 0 else 0)
    return present, frac, score


# ---------------------------------------------------------------- aggregation
def load(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def agg(rows):
    return {"questions": len(rows), "calls": sum(r["calls"] for r in rows), "tokens": sum(r["est_tokens"] for r in rows),
            "score": sum(r["score"] for r in rows), "max_score": 2 * len(rows),
            "facts_found": sum(r["facts_found"] for r in rows), "facts_total": sum(r["facts_n"] for r in rows)}


def per1k(a):
    return round(1000.0 * a["facts_found"] / a["tokens"], 2) if a["tokens"] else None


def build(rows):
    tools = [t for t in TOOL_ORDER if any(r["tool"] == t for r in rows)]
    out = {"tools": tools, "by_corpus": {}, "overall": {}, "overall_ex_overview": {}, "by_kind": {}}
    for t in tools:
        tr = [r for r in rows if r["tool"] == t]
        out["overall"][t] = agg(tr)
        out["overall_ex_overview"][t] = agg([r for r in tr if r["kind"] != "overview"])
        for c in CORPUS_ORDER:
            cr = [r for r in tr if r["corpus"] == c]
            if cr:
                out["by_corpus"].setdefault(c, {})[t] = agg(cr)
        for k in KIND_ORDER:
            kr = [r for r in tr if r["kind"] == k]
            if kr:
                out["by_kind"].setdefault(k, {})[t] = {"score": sum(r["score"] for r in kr), "max": 2 * len(kr),
                                                      "tokens": sum(r["est_tokens"] for r in kr),
                                                      "facts": f'{sum(r["facts_found"] for r in kr)}/{sum(r["facts_n"] for r in kr)}'}
    return out


def md(a, rows):
    tools = a["tools"]
    L = []
    L.append("| Corpus | " + " | ".join(f"{t} calls / tokens / score / facts" for t in tools) + " |")
    L.append("|---|" + "---|" * len(tools))
    for c in CORPUS_ORDER:
        if c in a["by_corpus"]:
            cells = []
            for t in tools:
                x = a["by_corpus"][c].get(t)
                cells.append(f'{x["calls"]} / {x["tokens"]:,} / {x["score"]} of {x["max_score"]} / {x["facts_found"]} of {x["facts_total"]}' if x else "-")
            L.append(f"| {c} | " + " | ".join(cells) + " |")
    for title, key in (("**All**", "overall"), ("All except overview", "overall_ex_overview")):
        cells = []
        for t in tools:
            x = a[key][t]
            cells.append(f'{x["calls"]} / {x["tokens"]:,} / **{x["score"]} of {x["max_score"]}** / {x["facts_found"]} of {x["facts_total"]} ({per1k(x)} facts/1k tok)')
        L.append(f"| {title} | " + " | ".join(cells) + " |")
    L.append("")
    L.append("By question type (score, out of 2 x number of corpora run):")
    L.append("")
    L.append("| Type | " + " | ".join(tools) + " |")
    L.append("|---|" + "---|" * len(tools))
    for k in KIND_ORDER:
        if k in a["by_kind"]:
            L.append(f"| {k} | " + " | ".join(
                (f'{a["by_kind"][k][t]["score"]}/{a["by_kind"][k][t]["max"]}, {a["by_kind"][k][t]["facts"]} facts, {a["by_kind"][k][t]["tokens"]:,} tok'
                 if t in a["by_kind"][k] else "-") for t in tools) + " |")
    return "\n".join(L)


def sanitize(rows):
    out = []
    for r in rows:
        r = dict(r)
        if r["corpus"] == "core":  # private repository: numbers only
            r.pop("question", None)
            r.pop("missing", None)
            r.pop("cmds", None)
        out.append(r)
    return out


if __name__ == "__main__":
    d = sys.argv[1]
    rows = load(os.path.join(d, "tasks.jsonl"))
    a = build(rows)
    json.dump(a, open(os.path.join(d, "tasks_aggregate.json"), "w"), indent=1)
    open(os.path.join(d, "tasks_headline.md"), "w").write(md(a, rows) + "\n")
    if "--sanitize" in sys.argv:
        with open(os.path.join(d, "tasks_rows_sanitized.jsonl"), "w") as f:
            for r in sanitize(rows):
                f.write(json.dumps(r, sort_keys=True) + "\n")
    print(md(a, rows))
