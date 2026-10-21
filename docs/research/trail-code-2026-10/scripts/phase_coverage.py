#!/usr/bin/env python3
"""Phase 9: language coverage - which files get symbols (Trail: full wiring graph; dctx: per-extension sample).

usage: phase_coverage.py CORPUS [CORPUS...]
"""
import collections
import json
import os
import random
import sys
import zlib

from benchlib import W, WORK, RES, run, as_bench, append_jsonl, trail_index_dir

SAMPLE = 12


def main(corpus):
    td, dd = os.path.join(WORK, "trail", corpus), os.path.join(WORK, "dctx", corpus)
    files = [f for f in as_bench(f"cd '{dd}' && git -c safe.directory='*' ls-files").split("\n") if f]
    by_ext = collections.defaultdict(list)
    for f in files:
        by_ext[os.path.splitext(f)[1].lower() or "(none)"].append(f)
    wiring = json.loads(as_bench(f"cat '{trail_index_dir(td)}/.graph/wiring.json'"))
    trail_files = collections.Counter()
    for n in wiring.get("nodes", []):
        if n.get("kind") not in ("file", "module", "directory") and n.get("path"):
            trail_files[n["path"]] += 1
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    rng = random.Random(zlib.crc32(("cov" + corpus).encode()))
    for ext, fl in sorted(by_ext.items(), key=lambda kv: -len(kv[1])):
        if len(fl) < 2 and ext not in (".rs", ".py", ".go", ".ts", ".js", ".md", ".toml", ".json", ".yaml", ".yml"):
            continue
        sample = sorted(fl) if len(fl) <= SAMPLE else sorted(rng.sample(sorted(fl), SAMPLE))
        dctx_hit, cov_labels = 0, collections.Counter()
        for f in sample:
            r = run(["dctx", "map", "map", f, "--depth", "1", "--limit", "1", "--json"], dd, tool="dctx",
                    corpus=corpus, label="cov", log="cov_runs.jsonl", keep=False, extra_env=env)
            try:
                j = json.loads(r["_stdout"])
                recs = [x for x in j.get("records", []) if x.get("id", "").startswith(("s:", "d:", "t:"))]
                cov_labels[j.get("precision", {}).get("coverage", "?")] += 1
                if recs:
                    dctx_hit += 1
            except ValueError:
                cov_labels["error"] += 1
        row = {"corpus": corpus, "ext": ext, "files": len(fl),
               "trail_files_with_symbols": sum(1 for f in fl if trail_files.get(f)),
               "trail_symbols": sum(trail_files.get(f, 0) for f in fl),
               "sample_n": len(sample), "dctx_sample_with_symbols": dctx_hit,
               "trail_sample_with_symbols": sum(1 for f in sample if trail_files.get(f)),
               "dctx_coverage_labels": dict(cov_labels)}
        append_jsonl("coverage.jsonl", row)
        print(row, flush=True)
    append_jsonl("coverage_summary.jsonl", {"corpus": corpus, "git_files": len(files),
                                            "trail_files_with_symbols": len(trail_files),
                                            "trail_nodes": len(wiring.get("nodes", [])),
                                            "trail_edges": len(wiring.get("edges", [])),
                                            "trail_languages": wiring.get("meta", {}).get("languages"),
                                            "trail_edge_relations": dict(collections.Counter(
                                                e.get("relation") for e in wiring.get("edges", []))),
                                            "trail_edge_confidence": dict(collections.Counter(
                                                str(e.get("confidence")) for e in wiring.get("edges", [])))})


if __name__ == "__main__":
    for c in sys.argv[1:]:
        main(c)
