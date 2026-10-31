#!/usr/bin/env python3
"""Aggregate and render the independent benchmark table (v1 card 6). Published whatever the numbers are.

usage: report.py RESULTS_DIR   (reads tasks.jsonl, callers.jsonl, env.json; writes independent.json and table.md)
"""
import json
import os
import sys

TOOLS = ["dctx", "trail", "graphify"]


def _ratio(a, b):
    return round(a / b, 3) if b else None


def aggregate(tasks, callers):
    """Totals per tool (and per repo) for the task questions and the callers precision / recall."""
    def tsum(rows):
        a = {"questions": len(rows), "score": sum(r["score"] for r in rows), "max_score": 2 * len(rows),
             "facts_found": sum(r["facts_found"] for r in rows), "facts_n": sum(r["facts_n"] for r in rows),
             "tokens": sum(r["tokens"] for r in rows)}
        a["facts_per_1k_tokens"] = round(1000.0 * a["facts_found"] / a["tokens"], 2) if a["tokens"] else None
        return a

    def csum(rows):
        a = {k: sum(r.get(k, 0) for r in rows) for k in ("hits", "tp_hits", "found", "gold", "tokens")}
        a["targets"], a["resolved"] = len(rows), sum(1 for r in rows if r.get("resolved", True))
        a["precision"], a["recall"] = _ratio(a["tp_hits"], a["hits"]), _ratio(a["found"], a["gold"])
        if any("found_all" in r for r in rows):
            a["recall_all_pages"] = _ratio(sum(r.get("found_all", r.get("found", 0)) for r in rows), a["gold"])
        return a

    tools = [t for t in TOOLS if any(r["tool"] == t for r in tasks + callers)]
    repos = sorted({r["repo"] for r in tasks + callers})
    return {"tasks": {t: tsum([r for r in tasks if r["tool"] == t]) for t in tools},
            "callers": {t: csum([r for r in callers if r["tool"] == t]) for t in tools},
            "per_repo": {rp: {"tasks": {t: tsum([r for r in tasks if r["tool"] == t and r["repo"] == rp]) for t in tools},
                              "callers": {t: csum([r for r in callers if r["tool"] == t and r["repo"] == rp]) for t in tools}}
                         for rp in repos}}


def _f(x):
    return "n/a" if x is None else (f"{x:.3f}" if isinstance(x, float) else str(x))


def table(agg, meta):
    """Markdown: selection, pins, the 20-question table and the callers table (overall and per repo)."""
    out = ["# Independent benchmark (v1 card 6)", "",
           f"Seed `{meta['seed']}` ({meta.get('seed_source', 'see selection.json')}). Tokenizer: {meta['tokenizer']} for every tool, "
           "raw stdout. Gold: language servers (callHierarchy), facts from the repos' own history. Published regardless of outcome.", "",
           "| Repo | Language | Tag | Commit |", "|---|---|---|---|"]
    for s in meta["selection"]:
        out.append(f"| {s['name']} | {s['lang']} | {s['tag']} | `{(s.get('sha') or 'unresolved')[:12]}` |")
    out += ["", "## Task questions", "", "| Tool | Points | Max | Facts found | Facts | Tokens | Facts per 1k tokens |", "|---|---|---|---|---|---|---|"]
    for t, a in agg["tasks"].items():
        out.append(f"| {t} | {a['score']} | {a['max_score']} | {a['facts_found']} | {a['facts_n']} | {a['tokens']} | {_f(a['facts_per_1k_tokens'])} |")
    out += ["", "## Callers against LSP gold (first page)", "",
            "| Tool | Targets | Resolved | Precision | Recall | Recall, all pages | Tokens |", "|---|---|---|---|---|---|---|"]
    for t, a in agg["callers"].items():
        out.append(f"| {t} | {a['targets']} | {a['resolved']} | {_f(a['precision'])} | {_f(a['recall'])} | {_f(a.get('recall_all_pages'))} | {a['tokens']} |")
    out += ["", "## Per repo", "", "| Repo | Tool | Points / max | Callers P | Callers R |", "|---|---|---|---|---|"]
    for rp, v in agg["per_repo"].items():
        for t in v["tasks"]:
            a, c = v["tasks"][t], v["callers"][t]
            out.append(f"| {rp} | {t} | {a['score']} / {a['max_score']} | {_f(c['precision'])} | {_f(c['recall'])} |")
    out += ["", "Precision counts a caller the language server did not report as wrong; servers can miss dynamic calls "
            "(Python receivers, Go interfaces), so precision is a lower bound for every tool alike."]
    return "\n".join(out) + "\n"


def load(path):
    return [json.loads(l) for l in open(path) if l.strip()] if os.path.exists(path) else []


def main_dir(d):
    """Write independent.json and table.md for results dir d and print the table."""
    meta = json.load(open(os.path.join(d, "env.json")))
    agg = aggregate(load(os.path.join(d, "tasks.jsonl")), load(os.path.join(d, "callers.jsonl")))
    json.dump(dict(agg, meta=meta), open(os.path.join(d, "independent.json"), "w"), indent=1)
    md = table(agg, meta)
    open(os.path.join(d, "table.md"), "w").write(md)
    print(md)


if __name__ == "__main__":
    main_dir(sys.argv[1])
