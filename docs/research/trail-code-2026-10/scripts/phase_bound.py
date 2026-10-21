#!/usr/bin/env python3
"""Phase 8: bounding - budget compliance and worst-case output size for broad queries.

usage: phase_bound.py CORPUS [CORPUS...]
"""
import json
import os
import sys

from benchlib import W, WORK, RES, run, as_bench, append_jsonl


def main(corpus):
    with open(os.path.join(RES, "targets", f"{corpus}.targets.json")) as f:
        t = next(x for x in json.load(f)["targets"] if x["category"] == "hub")
    name = t["def"]["name"]
    td, dd = os.path.join(WORK, "trail", corpus), os.path.join(WORK, "dctx", corpus)
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    big = as_bench(f"cd '{dd}' && git -c safe.directory='*' ls-files -z | xargs -0 ls -S 2>/dev/null | "
                   f"grep -E '\\.(rs|py|go|ts|js)$' | head -1").strip()
    did = None
    with open(os.path.join(RES, "query_targets.jsonl")) as fh:
        for line in fh:
            r = json.loads(line)
            if r.get("corpus") == corpus and r.get("dctx_id"):
                did = r["dctx_id"]
    cases = [
        ("dctx", "broad_find_e", ["dctx", "map", "find", "e", "--limit", "40", "--budget", "2000"]),
        ("dctx", "broad_find_e_json", ["dctx", "map", "find", "e", "--limit", "40", "--budget", "2000", "--json"]),
        ("dctx", "broad_map_bigfile", ["dctx", "map", "map", big, "--depth", "3", "--limit", "40", "--budget", "2000"]),
        ("dctx", "broad_read_bigfile", ["dctx", "map", "read", big, "--json"]),
        ("dctx", "default_find_e", ["dctx", "map", "find", "e"]),
        ("trail", "broad_grep_e", ["trail", "grep", "e"]),
        ("trail", "broad_grep_dot", ["trail", "grep", "."]),
        ("trail", "broad_callers_all", ["trail", "callers", name, "-d", "all"]),
        ("trail", "broad_ask_n1000", ["trail", "ask", "the", "-n", "1000"]),
        ("trail", "broad_ask_source_full_n50", ["trail", "ask", name, "--source", "--full", "-n", "50"]),
        ("trail", "broad_map_all", ["trail", "map", "--max-dirs", "100000"]),
        ("trail", "broad_skeleton_bigfile", ["trail", "skeleton", big]),
        ("trail", "default_ask", ["trail", "ask", name]),
    ]
    for b in (20, 100, 500, 2000):
        cases.append(("dctx", f"budget_{b}_find", ["dctx", "map", "find", name, "--limit", "40", "--budget", str(b)]))
        cases.append(("dctx", f"budget_{b}_find_json", ["dctx", "map", "find", name, "--limit", "40", "--budget", str(b), "--json"]))
        if did:
            cases.append(("dctx", f"budget_{b}_read", ["dctx", "map", "read", did, "--json", "--budget", str(b)]))
    for tool, label, cmd in cases:
        r = run(cmd, dd if tool == "dctx" else td, tool=tool, corpus=corpus, label=label, timeout=900,
                extra_env=env if tool == "dctx" else None, log="bound_runs.jsonl")
        row = {"corpus": corpus, "tool": tool, "label": label, "rc": r["rc"], "bytes": r["stdout_bytes"],
               "est_tokens": r["est_tokens"], "wall_s": r.get("wall_s"), "maxrss_kb": r.get("maxrss_kb"),
               "timed_out": r["timed_out"], "bigfile": big}
        if label.startswith("budget_"):
            row["budget"] = int(label.split("_")[1])
            row["within_budget"] = r["est_tokens"] <= row["budget"]
            row["stderr"] = r["_stderr"].decode("utf-8", "replace")[-200:]
        append_jsonl("bounding.jsonl", row)
        print(row, flush=True)


if __name__ == "__main__":
    for c in sys.argv[1:]:
        main(c)
