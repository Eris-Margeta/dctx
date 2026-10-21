#!/usr/bin/env python3
"""Aggregate raw JSONL results into CSV tables and a markdown summary.

usage: aggregate.py RESULTS_DIR OUT_DATA_DIR
"""
import collections
import csv
import glob
import json
import math
import os
import re
import sys

R, OUT = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)
MD = []
CORPORA = ["flask", "ripgrep", "pocketbase", "trailcode", "dctx", "django", "core"]


def jl(name):
    p = os.path.join(R, name)
    if not os.path.exists(p):
        return []
    with open(p) as f:
        return [json.loads(l) for l in f if l.strip()]


def med(xs):
    xs = sorted(x for x in xs if isinstance(x, (int, float)) and not math.isnan(x))
    if not xs:
        return None
    n = len(xs)
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


def fmt(x, nd=2):
    if x is None:
        return "-"
    if isinstance(x, float):
        return f"{x:.{nd}f}"
    return str(x)


def table(title, header, rows, csvname=None):
    MD.append(f"\n### {title}\n")
    MD.append("| " + " | ".join(header) + " |")
    MD.append("|" + "|".join("---" for _ in header) + "|")
    for r in rows:
        MD.append("| " + " | ".join(fmt(x) for x in r) + " |")
    if csvname:
        with open(os.path.join(OUT, csvname), "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(header)
            w.writerows(rows)


runs = jl("runs.jsonl")
by = collections.defaultdict(list)
for r in runs:
    by[(r["corpus"], r["tool"], r["label"])].append(r)


def stat(c, t, label, key):
    return med([r.get(key) for r in by.get((c, t, label), [])])


def cpu(c, t, label):
    return med([(r.get("user_s") or 0) + (r.get("sys_s") or 0) for r in by.get((c, t, label), [])])


# ---------------------------------------------------------------- 2. indexing
size = {(s["corpus"], s["tool"]): s for s in jl("index_size.jsonl") if "rep" not in s}
rows = []
for c in CORPORA:
    for t in ("dctx", "trail"):
        if not by.get((c, t, "cold_index")):
            continue
        s = size.get((c, t), {})
        noop = "noop_query" if t == "dctx" else "noop_build"
        rows.append([c, t, len(by[(c, t, "cold_index")]), stat(c, t, "cold_index", "wall_s"), cpu(c, t, "cold_index"),
                     (stat(c, t, "cold_index", "maxrss_kb") or 0) / 1024, (s.get("index_bytes") or 0) / 1e6,
                     s.get("index_files"), stat(c, t, noop, "wall_s"), cpu(c, t, noop),
                     stat(c, t, "noop_check", "wall_s") if t == "trail" else None])
table("Cold index and no-op refresh (medians; wall s, CPU s = user+sys, RSS MiB, index MB)",
      ["corpus", "tool", "n", "cold wall", "cold CPU", "peak RSS", "index MB", "index files", "no-op wall",
       "no-op CPU", "trail build --check wall"], rows, "indexing.csv")

# ---------------------------------------------------------------- 3/4. queries
labels = ["find", "find_json", "callers1", "callers2", "callees", "outline", "overview", "grep", "read", "read_text",
          "explain", "callers1_norefresh"]
rows = []
for c in CORPORA:
    for lab in labels:
        for t in ("dctx", "trail"):
            rs = by.get((c, t, "warm_" + lab))
            if not rs:
                continue
            ov = None
            if lab == "overview" and t == "dctx":
                ov = med([o["file_bytes"] for o in jl("overview_files.jsonl") if o["corpus"] == c])
            nbytes = ov if ov is not None else rs[0]["stdout_bytes"]
            rows.append([c, lab, t, len(rs), stat(c, t, "warm_" + lab, "wall_s"), cpu(c, t, "warm_" + lab),
                         (stat(c, t, "warm_" + lab, "maxrss_kb") or 0) / 1024, nbytes, math.ceil(nbytes / 3.5),
                         rs[0]["rc"]])
table("Warm query latency and output size (median of 5; bytes/tokens of rep 0; tokens = ceil(bytes/3.5))",
      ["corpus", "query", "tool", "n", "wall s", "CPU s", "RSS MiB", "bytes", "est tokens", "rc"], rows,
      "queries_warm.csv")
rows = []
for c in CORPORA:
    for lab in ("find", "callers1", "outline", "read"):
        rs = by.get((c, "dctx", "cold_" + lab))
        if rs:
            rows.append([c, lab, len(rs), stat(c, "dctx", "cold_" + lab, "wall_s"), cpu(c, "dctx", "cold_" + lab)])
table("dctx cold query (empty cache, includes snapshot+index; median of 3)", ["corpus", "query", "n", "wall s", "CPU s"],
      rows, "queries_cold_dctx.csv")

# ---------------------------------------------------------------- 7. determinism / stability


def load_out(c, t, label, rep):
    p = os.path.join(R, "outputs", c, t, re.sub(r"[^A-Za-z0-9_.-]+", "_", label)[:120] + f".r{rep}.out")
    try:
        with open(p, "rb") as f:
            return f.read()
    except OSError:
        return None


TS = re.compile(rb"\b20\d\d-\d\d-\d\dT\d\d:\d\d|\b1[67]\d{8,11}\b")
HEX = re.compile(rb"\b[0-9a-f]{16,}\b")
ABS = re.compile(rb"/path/to/workdir")
rows = []
for c in CORPORA:
    for lab in labels:
        for t in ("dctx", "trail"):
            rs = by.get((c, t, "warm_" + lab))
            if not rs:
                continue
            shas = {r["stdout_sha256"] for r in rs}
            base = load_out(c, t, "warm_" + lab, 0) or b""
            u = by.get((c, t, "unrelated_" + lab))
            same_after = None
            prefix = None
            changed_lines = None
            if u:
                after = load_out(c, t, "unrelated_" + lab, 0) or b""
                same_after = u[0]["stdout_sha256"] == rs[0]["stdout_sha256"]
                k = 0
                while k < min(len(base), len(after)) and base[k] == after[k]:
                    k += 1
                prefix = k
                bl, al = base.split(b"\n"), after.split(b"\n")
                changed_lines = sum(1 for x, y in zip(bl, al) if x != y) + abs(len(bl) - len(al))
            rows.append([c, lab, t, len(shas) == 1, same_after, prefix, len(base), changed_lines,
                         bool(TS.search(base)), bool(ABS.search(base)), len(HEX.findall(base))])
table("Cache-friendliness: 5 identical runs byte-identical? and stability after an unrelated edit",
      ["corpus", "query", "tool", "5x identical", "identical after unrelated edit", "common prefix bytes",
       "output bytes", "changed lines", "timestamp-like", "abs path", "hex>=16 tokens"], rows, "stability.csv")

# ---------------------------------------------------------------- freshness
rows = []
for f in jl("freshness.jsonl"):
    rows.append([f["corpus"], f["tool"], f["edit_find_found"], f["edit_find_wall"], f["edit_callers_has_probe"],
                 f["untracked_find_found"], f["staged_find_found"], f["reverted_stale_probe"] or f["reverted_stale_probe2"],
                 f["reverted_callers_has_probe"], f.get("edit_find_again_wall")])
table("Freshness after edits (found = query output contains the new symbol)",
      ["corpus", "tool", "unstaged edit: new fn found", "first query wall s", "caller edge to new fn",
       "new untracked file found", "after git add found", "stale after revert", "stale caller after revert",
       "second query wall s"], rows, "freshness.csv")

# ---------------------------------------------------------------- accuracy
acc = jl("accuracy.jsonl")
rows = []
for c in CORPORA:
    for t in ("trail", "dctx"):
        a = [x for x in acc if x["corpus"] == c and not x["name_level"]]
        if not a:
            continue
        gold = sum(x["gold_n"] for x in a)
        found = sum(x[f"{t}_found"] for x in a)
        tp = sum(x[f"{t}_tp"] for x in a)
        fp = sum(x[f"{t}_fp"] for x in a)
        unj = sum(x[f"{t}_unjudged"] for x in a)
        prec = tp / (tp + fp) if tp + fp else None
        rec = found / gold if gold else None
        k = "trail_ask_def_rank" if t == "trail" else "dctx_find_def_rank"
        top1 = sum(1 for x in acc if x["corpus"] == c and x.get(k) == 1)
        any10 = sum(1 for x in acc if x["corpus"] == c and x.get(k))
        n_all = sum(1 for x in acc if x["corpus"] == c)
        nl = [x for x in acc if x["corpus"] == c and x["name_level"]]
        nl_gold = sum(x["gold_n"] for x in nl)
        nl_rec = (sum(x[f"{t}_found"] for x in nl) / nl_gold) if nl_gold else None
        rows.append([c, t, len(a), gold, tp, fp, unj, prec, rec, len(nl), nl_rec, f"{top1}/{n_all}", f"{any10}/{n_all}"])
table("Callers (depth 1) accuracy vs adjudicated gold; definition-level targets (micro over call sites)",
      ["corpus", "tool", "targets", "gold sites", "TP hits", "FP hits", "unjudged hits", "precision", "recall",
       "interface targets", "interface name-level recall", "find-def rank 1", "find-def in results"], rows,
      "accuracy.csv")
rows = []
for cat in ("hub", "rare", "receiver", "collision", "interface", "hub-fill"):
    for t in ("trail", "dctx"):
        a = [x for x in acc if x["category"] == cat]
        if not a:
            continue
        gold = sum(x["gold_n"] for x in a)
        found = sum(x[f"{t}_found"] for x in a)
        tp = sum(x[f"{t}_tp"] for x in a)
        fp = sum(x[f"{t}_fp"] for x in a)
        rows.append([cat, t, len(a), gold, tp / (tp + fp) if tp + fp else None, found / gold if gold else None])
table("Callers accuracy by target category (all corpora)", ["category", "tool", "targets", "gold sites", "precision",
                                                             "recall"], rows, "accuracy_by_category.csv")
with open(os.path.join(OUT, "accuracy_targets.csv"), "w", newline="") as f:
    w = csv.writer(f)
    keys = ["corpus", "category", "name", "file", "line", "kind", "container", "n_defs_same_name", "gold_n",
            "ambiguous_n", "trail_def_matched", "trail_hits", "trail_tp", "trail_fp", "trail_found",
            "trail_found_any_relation", "trail_ask_def_rank", "dctx_find_def_rank", "dctx_id", "dctx_hits", "dctx_found"]
    w.writerow(keys)
    for x in acc:
        w.writerow([x.get(k) for k in keys])

# ---------------------------------------------------------------- tasks
tasks = jl("tasks.jsonl")
rows = []
tot = collections.defaultdict(lambda: [0, 0, 0, 0, 0, 0])
for c in CORPORA:
    for t in ("dctx", "trail"):
        a = [x for x in tasks if x["corpus"] == c and x["tool"] == t]
        if not a:
            continue
        calls = sum(x["calls"] for x in a)
        toks = sum(x["est_tokens"] for x in a)
        score = sum(x["score"] for x in a)
        ff = sum(x["facts_found"] for x in a)
        fn = sum(x["facts_n"] for x in a)
        rows.append([c, t, len(a), calls, toks, f"{score}/{2 * len(a)}", f"{ff}/{fn}",
                     1000 * ff / toks if toks else None])
        for i, v in enumerate((len(a), calls, toks, score, ff, fn)):
            tot[t][i] += v
for t, v in tot.items():
    rows.append(["ALL", t, v[0], v[1], v[2], f"{v[3]}/{2 * v[0]}", f"{v[4]}/{v[5]}", 1000 * v[4] / v[2] if v[2] else None])
table("Task-level information efficiency (10 questions per corpus)",
      ["corpus", "tool", "questions", "calls", "est tokens", "score", "facts found", "facts per 1k tokens"], rows,
      "tasks.csv")
rows = []
for q in sorted({x["qid"] for x in tasks}, key=lambda s: int(s[1:])):
    for t in ("dctx", "trail"):
        a = [x for x in tasks if x["qid"] == q and x["tool"] == t]
        if a:
            toks = sum(x["est_tokens"] for x in a)
            ff = sum(x["facts_found"] for x in a)
            rows.append([q, a[0]["kind"], t, len(a), sum(x["score"] for x in a), 2 * len(a), toks,
                         1000 * ff / toks if toks else None])
table("Task-level by question type (all corpora)", ["qid", "kind", "tool", "corpora", "score", "max", "est tokens",
                                                     "facts per 1k tokens"], rows, "tasks_by_question.csv")

# ---------------------------------------------------------------- bounding
rows = [[b["corpus"], b["tool"], b["label"], b["bytes"], b["est_tokens"], b.get("wall_s"), b.get("within_budget"),
         b["rc"], b["timed_out"]] for b in jl("bounding.jsonl")]
table("Bounding: broad queries and budget compliance", ["corpus", "tool", "query", "bytes", "est tokens", "wall s",
                                                         "within budget", "rc", "timed out"], rows, "bounding.csv")

# ---------------------------------------------------------------- coverage
rows = [[x["corpus"], x["ext"], x["files"], x["trail_files_with_symbols"], x["sample_n"],
         x["trail_sample_with_symbols"], x["dctx_sample_with_symbols"], json.dumps(x["dctx_coverage_labels"])]
        for x in jl("coverage.jsonl")]
table("Language coverage (files with >=1 symbol; dctx measured on a seeded sample of <=12 files per extension)",
      ["corpus", "ext", "git files", "trail files w/ symbols", "sample", "trail sample w/ symbols",
       "dctx sample w/ symbols", "dctx precision labels"], rows, "coverage.csv")

with open(os.path.join(OUT, "SUMMARY-TABLES.md"), "w") as f:
    f.write("# Generated tables\n\nGenerated by scripts/aggregate.py from data/raw/*.jsonl.\n")
    f.write("\n".join(MD) + "\n")
print("\n".join(MD))
