#!/usr/bin/env python3
"""Phase 2: cold index (x3), on-disk size, no-op refresh (x5), files written.

usage: phase_index.py CORPUS [CORPUS...]
"""
import json
import os
import sys

from benchlib import (W, WORK, RES, run, as_bench, fresh_copy, du_bytes, manifest, manifest_diff,
                      append_jsonl, loadavg, trail_index_dir)

HOME = os.path.join(W, "home")
COLD_REPS = 3
NOOP_REPS = 5


def trail(corpus):
    rows = []
    for r in range(COLD_REPS):
        d = fresh_copy(corpus, "trail")
        if r == 0:
            before, hb = manifest(d), manifest(HOME)
        rows.append(run(["trail", "build", "."], d, tool="trail", corpus=corpus, label="cold_index", rep=r,
                        timeout=3600))
        if r == 0:
            after, ha = manifest(d), manifest(HOME)
            diff = manifest_diff(before, after)
            hdiff = manifest_diff(hb, ha)
            append_jsonl("writes.jsonl", {
                "tool": "trail", "corpus": corpus, "step": "trail build",
                "repo_added_n": len(diff["added"]), "repo_changed": diff["changed"],
                "repo_removed": diff["removed"],
                "repo_added_top": sorted({a.split("/")[0] for a in diff["added"]}),
                "repo_added_sample": diff["added"][:20],
                "home_added": hdiff["added"][:50], "home_changed": hdiff["changed"],
            })
    idx = trail_index_dir(d)
    size = {"tool": "trail", "corpus": corpus, "index_dir": os.path.relpath(idx, d),
            "index_bytes": du_bytes(idx),
            "index_files": int(as_bench(f"find '{idx}' -type f | wc -l").strip())}
    for sub in (".graph", ".cache"):
        size["index_bytes_" + sub.strip(".")] = du_bytes(os.path.join(idx, sub))
    append_jsonl("index_size.jsonl", size)
    for r in range(NOOP_REPS):
        run(["trail", "build", "."], d, tool="trail", corpus=corpus, label="noop_build", rep=r, timeout=3600)
    for r in range(NOOP_REPS):
        run(["trail", "build", "--check", "."], d, tool="trail", corpus=corpus, label="noop_check", rep=r,
            timeout=3600)


def dctx(corpus):
    d = fresh_copy(corpus, "dctx")
    before, hb = manifest(d), manifest(HOME)
    for r in range(COLD_REPS):
        cache = os.path.join(W, "dctx-cache", corpus if r == COLD_REPS - 1 else f"{corpus}.cold{r}")
        as_bench(f"rm -rf '{cache}' && mkdir -p '{cache}'")
        run(["dctx", "map", "find", "main", "--limit", "10"], d, tool="dctx", corpus=corpus,
            label="cold_index", rep=r, extra_env={"DCTX_CACHE_DIR": cache}, timeout=3600)
        if r == 0:
            after, ha = manifest(d), manifest(HOME)
            diff, hdiff = manifest_diff(before, after), manifest_diff(hb, ha)
            append_jsonl("writes.jsonl", {
                "tool": "dctx", "corpus": corpus, "step": "dctx map find (cold)",
                "repo_added_n": len(diff["added"]), "repo_changed": diff["changed"],
                "repo_removed": diff["removed"], "repo_added_sample": diff["added"][:20],
                "home_added": hdiff["added"][:50], "home_changed": hdiff["changed"],
            })
        if r < COLD_REPS - 1:
            append_jsonl("index_size.jsonl", {"tool": "dctx", "corpus": corpus, "rep": r,
                                             "index_dir": "DCTX_CACHE_DIR", "index_bytes": du_bytes(cache)})
            as_bench(f"rm -rf '{cache}'")
    cache = os.path.join(W, "dctx-cache", corpus)
    append_jsonl("index_size.jsonl", {"tool": "dctx", "corpus": corpus, "index_dir": "DCTX_CACHE_DIR",
                                     "index_bytes": du_bytes(cache),
                                     "index_files": int(as_bench(f"find '{cache}' -type f | wc -l").strip())})
    env = {"DCTX_CACHE_DIR": cache}
    for r in range(NOOP_REPS):
        run(["dctx", "map", "find", "main", "--limit", "10"], d, tool="dctx", corpus=corpus, label="noop_query",
            rep=r, extra_env=env)
    run(["dctx", "map", "find", "main", "--limit", "10", "--profile"], d, tool="dctx", corpus=corpus,
        label="profile_warm", extra_env=env)
    run(["dctx", "cache", "status", "--json"], d, tool="dctx", corpus=corpus, label="cache_status", extra_env=env)


if __name__ == "__main__":
    for c in sys.argv[1:]:
        append_jsonl("phases.jsonl", {"phase": "index", "corpus": c, "event": "start", "load1": loadavg()})
        trail(c)
        dctx(c)
        append_jsonl("phases.jsonl", {"phase": "index", "corpus": c, "event": "end", "load1": loadavg()})
        print("done", c, flush=True)
