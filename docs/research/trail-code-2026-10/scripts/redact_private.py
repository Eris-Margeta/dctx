#!/usr/bin/env python3
"""Redact identifiers of a private corpus from the published data directory.

usage: redact_private.py DATA_DIR CORPUS
Keeps every number (timings, sizes, counts, scores); drops symbol names, paths, commands and source text
for rows of CORPUS, removes its raw outputs, and reduces its target/question files to counts.
"""
import csv
import json
import os
import shutil
import sys

KEEP = {"corpus", "tool", "rep", "rc", "timed_out", "outer_wall_s", "load1_before", "load1_after", "stdout_bytes",
        "stderr_bytes", "est_tokens", "wall_s", "user_s", "sys_s", "maxrss_kb", "exit", "category", "kind", "gold_n",
        "ambiguous_n", "other_def_n", "n_defs_same_name", "name_level", "trail_def_matched", "trail_matches_n",
        "trail_hits", "trail_hits_any_relation", "trail_tp", "trail_fp", "trail_unjudged", "trail_found",
        "trail_found_any_relation", "trail_ask_def_rank", "dctx_find_def_rank", "dctx_find_records", "dctx_hits",
        "dctx_tp", "dctx_fp", "dctx_unjudged", "dctx_found", "i", "qid", "calls", "bytes", "facts_n",
        "facts_found", "fraction", "score", "index_bytes", "index_files", "index_bytes_graph", "index_bytes_cache",
        "index_dir", "budget", "within_budget", "ext", "files", "trail_files_with_symbols", "trail_symbols",
        "sample_n", "dctx_sample_with_symbols", "trail_sample_with_symbols", "dctx_coverage_labels", "git_files",
        "trail_nodes", "trail_edges", "trail_languages", "trail_edge_relations", "trail_edge_confidence",
        "phase", "event", "load1", "file_bytes", "repo_added_n", "step", "stdout_sha256", "line", "container"}
SAFE_LABEL_PREFIX = ("cold_", "warm_", "noop_", "unrelated_", "profile_", "cache_", "broad_", "budget_", "default_",
                     "base_", "edit_", "untracked_", "staged_", "reverted_")


def redact_row(r, corpus):
    if r.get("corpus") != corpus and corpus not in str(r.get("cwd", "")):
        return r
    out = {k: v for k, v in r.items() if k in KEEP}
    lab = r.get("label")
    if isinstance(lab, str):
        out["label"] = lab if lab.startswith(SAFE_LABEL_PREFIX) else lab.split("_")[0] + "_<redacted>"
    for k in ("line", "container"):
        out.pop(k, None)
    out["redacted"] = True
    return out


def main():
    data, corpus = sys.argv[1], sys.argv[2]
    shutil.rmtree(os.path.join(data, "outputs", corpus), ignore_errors=True)
    raw = os.path.join(data, "raw")
    for fn in sorted(os.listdir(raw)):
        p = os.path.join(raw, fn)
        if not (fn.endswith(".jsonl") or fn.endswith(".pre-regold")):
            continue
        rows = []
        with open(p) as f:
            for line in f:
                if line.strip():
                    rows.append(redact_row(json.loads(line), corpus))
        with open(p, "w") as f:
            for r in rows:
                f.write(json.dumps(r, sort_keys=True) + "\n")
    tdir = os.path.join(data, "targets")
    for fn in os.listdir(tdir):
        if fn.startswith(corpus + "."):
            p = os.path.join(tdir, fn)
            with open(p) as f:
                doc = json.load(f)
            if isinstance(doc, dict) and "targets" in doc:
                doc["targets"] = [{"category": t["category"], "kind": t["def"]["kind"],
                                   "n_defs_same_name": t["n_defs_same_name"],
                                   "gold_counts": {k: len(v) for k, v in t["gold"].items()}} for t in doc["targets"]]
            else:
                doc = [{"id": q.get("id"), "kind": q.get("kind"), "facts_n": len(q.get("facts", []))} for q in doc]
            doc_note = {"redacted": "private corpus: identifiers, paths and source text removed", "data": doc}
            with open(p, "w") as f:
                json.dump(doc_note, f, indent=1)
    for fn in os.listdir(data):
        if fn.endswith(".csv"):
            p = os.path.join(data, fn)
            with open(p, newline="") as f:
                rows = list(csv.reader(f))
            hdr = rows[0]
            sensitive = [i for i, h in enumerate(hdr) if h in ("name", "file", "dctx_id", "container", "line")]
            for r in rows[1:]:
                if r and r[0] == corpus:
                    for i in sensitive:
                        r[i] = "<redacted>"
            with open(p, "w", newline="") as f:
                csv.writer(f).writerows(rows)


if __name__ == "__main__":
    main()
