#!/usr/bin/env python3
"""Aggregate raw jsonl of one run into aggregate.json + headline.md.   usage: aggregate.py RESULTS_DIR"""
import json
import math
import os
import sys
from collections import defaultdict

RD = sys.argv[1]
TOOLS = ["dctx", "trail", "graphify"]
CORP_ORDER = ["flask", "ripgrep", "pocketbase", "trailcode", "mixed", "docs"]
QLABELS = ["find", "callers1", "callers2", "skeleton", "overview", "grep"]


def rows(name):
    p = os.path.join(RD, name)
    return [json.loads(l) for l in open(p)] if os.path.exists(p) else []


def mb(kb):
    return None if kb is None else round(kb / 1024, 1)


env = json.load(open(os.path.join(RD, "env.json"))) if os.path.exists(os.path.join(RD, "env.json")) else {}
agg = {"meta": env, "index": {}, "query": {}, "accuracy": {}, "determinism": {}, "canary": [], "network": [], "coverage": {}}

for r in rows("index.jsonl"):
    agg["index"].setdefault(r["corpus"], {})[r["tool"]] = {
        "ok": r["ok"], "n_reps": r["n_reps"], "cold_wall_s_median": r["cold_wall_s_median"],
        "cold_wall_s_all": [x["wall_s"] for x in r["reps"]],
        "peak_rss_mb_largest_process": mb(r["maxrss_kb_median"]),
        "peak_pss_mb_process_tree_sampled": mb(r["peak_tree_pss_kb_median"]),
        "peak_rss_mb_process_tree_sampled": mb(r["peak_tree_rss_kb_median"]),
        "index_mb": round(r["index_bytes"] / 1e6, 2), "index_files": r["index_files"],
        "nodes": r.get("nodes"), "edges": r.get("edges"), "step_walls_last_rep": r["reps"][-1]["step_walls"],
        "repo_touched": r["repo_touched"], "stderr_tail": r["reps"][-1].get("stderr_tail", "")[:200]}

for r in rows("queries.jsonl"):
    d = {k: r.get(k) for k in ("supported", "via", "why", "rc", "wall_s_median", "stdout_bytes", "est_tokens",
                               "identical_across_runs", "maxrss_kb_median", "walls")}
    agg["query"].setdefault(r["corpus"], {}).setdefault(r["tool"], {})[r["label"]] = d

for r in rows("determinism.jsonl"):
    agg["determinism"].setdefault(r["corpus"], {})[r["tool"]] = r

# ---------------- accuracy
acc = defaultdict(lambda: defaultdict(lambda: defaultdict(int)))
per_t = defaultdict(lambda: defaultdict(list))
for r in rows("accuracy.jsonl"):
    for t in TOOLS:
        e = r.get(t)
        if not e or "tp" not in e:
            if e is not None:
                acc[r["corpus"]][t]["unresolved"] += 1
            acc[r["corpus"]][t]["gold_all"] += r["gold_n"]
            continue
        a = acc[r["corpus"]][t]
        a["targets"] += 1
        a["gold_all"] += r["gold_n"]
        a["gold"] += r["gold_n"]
        a["hits"] += e["hits"]
        a["tp"] += e["tp"]
        a["fp"] += e["fp"]
        a["found"] += e["found"]
        a["tokens"] += e.get("tokens", 0)
        if t == "dctx":
            a["tp_allpages"] += e.get("tp_allpages", 0)
            a["fp_allpages"] += e.get("fp_allpages", 0)
            a["found_allpages"] += e.get("found_allpages", 0)
        if e["tp"] + e["fp"]:
            per_t[(r["corpus"], t)]["p"].append(e["tp"] / (e["tp"] + e["fp"]))
        if r["gold_n"]:
            per_t[(r["corpus"], t)]["r"].append(e["found"] / r["gold_n"])


def prf(a, tp="tp", fp="fp", found="found"):
    p = a[tp] / (a[tp] + a[fp]) if a[tp] + a[fp] else None
    rc = a[found] / a["gold_all"] if a["gold_all"] else None
    return (None if p is None else round(p, 3)), (None if rc is None else round(rc, 3))


tot = defaultdict(lambda: defaultdict(int))
for c, tt in acc.items():
    for t, a in tt.items():
        p, rc = prf(a)
        out = {"targets_scored": a["targets"], "gold_sites": a["gold_all"], "hits": a["hits"], "tp": a["tp"], "fp": a["fp"],
               "found": a["found"], "precision_micro": p, "recall_micro": rc,
               "precision_macro": round(sum(per_t[(c, t)]["p"]) / len(per_t[(c, t)]["p"]), 3) if per_t[(c, t)]["p"] else None,
               "recall_macro": round(sum(per_t[(c, t)]["r"]) / len(per_t[(c, t)]["r"]), 3) if per_t[(c, t)]["r"] else None,
               "unresolved_targets": a["unresolved"], "tokens_total": a["tokens"]}
        if t == "dctx":
            out["precision_micro_allpages"], out["recall_micro_allpages"] = prf(a, "tp_allpages", "fp_allpages", "found_allpages")
        agg["accuracy"].setdefault(c, {})[t] = out
        for k in ("tp", "fp", "found", "gold_all", "tp_allpages", "fp_allpages", "found_allpages", "tokens"):
            tot[t][k] += a[k]
agg["accuracy"]["ALL_GOLD_CORPORA"] = {}
for t in TOOLS:
    a = tot[t]
    p, rc = prf(a)
    e = {"tp": a["tp"], "fp": a["fp"], "found": a["found"], "gold_sites": a["gold_all"], "precision_micro": p, "recall_micro": rc}
    if t == "dctx":
        e["precision_micro_allpages"], e["recall_micro_allpages"] = prf(a, "tp_allpages", "fp_allpages", "found_allpages")
    agg["accuracy"]["ALL_GOLD_CORPORA"][t] = e

# ---------------- canary / network / coverage
agg["canary"] = [{k: v for k, v in r.items()} for r in rows("canary.jsonl")]
agg["network"] = rows("network.jsonl")
cov = defaultdict(dict)
for r in rows("coverage.jsonl"):
    cov[r["lang"]][r["tool"]] = {"files": r["files_total"], "symbols": r["symbols"], "call_edges": r["call_edges"],
                                 "symbol_ok": r["symbols"] >= 1, "call_edge_ok": r["call_edges"] >= 1}
agg["coverage"] = cov
json.dump(agg, open(os.path.join(RD, "aggregate.json"), "w"), indent=1, sort_keys=False, default=str)

# ================================================================ headline.md
L = []
w = L.append


def fmt(x, nd=2, dash="n/a"):
    return dash if x is None else (f"{x:.{nd}f}" if isinstance(x, float) else str(x))


def best(vals, lower=True):
    v = {k: x for k, x in vals.items() if x is not None}
    if not v:
        return None
    return (min if lower else max)(v, key=v.get)


w(f"# 3-way benchmark headline ({env.get('date_utc','?')})\n")
w(f"dctx `{env.get('dctx_caps',{}).get('version','?')}` sha256 `{(env.get('dctx_sha256') or '')[:12]}`; Trail-Code {env.get('trail_version')}; graphify {env.get('graphify_version')} "
  f"(code-only, no LLM). CPUs {env.get('cpus_pinned')}, nice 10, unshare -Urn no-network: {env.get('unshare_Urn_net_isolation')}. "
  f"Host load at start: {env.get('load1_at_start')} (shared host).\n")
w("## Cold index (median wall s | peak RSS MB largest process / sampled tree PSS | index MB)\n")
w("| corpus | dctx | Trail | graphify | fastest |")
w("|---|---|---|---|---|")
for c in CORP_ORDER:
    ix = agg["index"].get(c)
    if not ix:
        continue
    cells, walls = [], {}
    for t in TOOLS:
        e = ix.get(t)
        if not e or not e["ok"]:
            cells.append("FAILED / no index" if e else "n/a")
            continue
        walls[t] = e["cold_wall_s_median"]
        cells.append(f"{e['cold_wall_s_median']:.2f}s, {fmt(e['peak_rss_mb_largest_process'],0)}/{fmt(e['peak_pss_mb_process_tree_sampled'],0)} MB, {e['index_mb']:.1f} MB")
    w(f"| {c} | " + " | ".join(cells) + f" | {best(walls) or '-'} |")
w("\nLatency below is warm median of 5, wall ms per CLI invocation (includes process start; dctx is a native binary, Trail is Node, graphify is Python).\n")
for lab, title in (("find", "Symbol search"), ("callers1", "Callers depth 1"), ("skeleton", "File skeleton / outline"), ("overview", "Repo overview")):
    w(f"### {title}: median ms | est tokens\n")
    w("| corpus | dctx | Trail | graphify | fastest | smallest answer |")
    w("|---|---|---|---|---|---|")
    for c in CORP_ORDER:
        q = agg["query"].get(c)
        if not q:
            continue
        cells, walls, toks = [], {}, {}
        for t in TOOLS:
            e = (q.get(t) or {}).get(lab)
            if not e or not e.get("supported"):
                cells.append("unsupported" if e else "n/a")
                continue
            walls[t] = e["wall_s_median"]
            toks[t] = e["est_tokens"]
            cells.append(f"{e['wall_s_median']*1000:.0f} ms, {e['est_tokens']} tok" + ("" if e.get("rc") == 0 else f" (rc={e['rc']})"))
        w(f"| {c} | " + " | ".join(cells) + f" | {best(walls) or '-'} | {best(toks) or '-'} |")
    w("")
w("## Callers accuracy vs tool-independent regex gold (depth 1, ~20 targets per corpus; micro precision / recall)\n")
w("| corpus | dctx P / R | Trail P / R | graphify P / R |")
w("|---|---|---|---|")
for c in CORP_ORDER + ["ALL_GOLD_CORPORA"]:
    a = agg["accuracy"].get(c)
    if not a:
        continue
    cells = []
    for t in TOOLS:
        e = a.get(t)
        cells.append("n/a" if not e else f"{fmt(e['precision_micro'],3)} / {fmt(e['recall_micro'],3)}")
    w(f"| {c} | " + " | ".join(cells) + " |")
dd = agg["accuracy"].get("ALL_GOLD_CORPORA", {}).get("dctx", {})
if dd:
    w(f"\ndctx following all result pages (<=5): P {fmt(dd.get('precision_micro_allpages'),3)} / R {fmt(dd.get('recall_micro_allpages'),3)}.")
w("\n## Determinism / cache-friendliness\n")
w("| corpus | tool | identical across 5 runs (queries) | identical after unrelated edit | ... ignoring dctx snapshot header line | changed after edit | first query after edit (s) | graphify `update` (s) |")
w("|---|---|---|---|---|---|---|---|")
for c in CORP_ORDER:
    for t in TOOLS:
        d = agg["determinism"].get(c, {}).get(t)
        q = agg["query"].get(c, {}).get(t, {})
        sup = [k for k, v in q.items() if v.get("supported")]
        ident = [k for k in sup if q[k].get("identical_across_runs")]
        if not d:
            continue
        w(f"| {c} | {t} | {len(ident)}/{len(sup)} | {len(d['identical_after_edit'])}/{len(d['identical_after_edit'])+len(d['changed_after_edit'])} | "
          f"{len(d.get('identical_after_edit_ignoring_header_line',[]))}/{len(d['identical_after_edit'])+len(d['changed_after_edit'])} | "
          f"{', '.join(d['changed_after_edit']) or '-'} | {fmt(d.get('post_edit_first_find_s'),3)} | {fmt(d.get('post_edit_update_s'),2)} |")
w("\n## Privacy canary (fixtures with .env, .env.local, settings.env, config/prod.env holding CANARY strings)\n")
w("| fixture | tool | canary in any tool output | canary in index artifacts | queries that leaked |")
w("|---|---|---|---|---|")
for r in agg["canary"]:
    w(f"| {r['fixture']} | {r['tool']} | {r['leak_in_output']} | {r['leak_in_index']} | {', '.join(r['leaks_by_query']) or '-'} |")
w("\n## Network attempts (ptrace socket audit inside `unshare -Urn`, flask)\n")
w("| tool | env variant | steps traced | AF_INET/INET6 connect/send attempts | targets (sample) | unix sockets |")
w("|---|---|---|---|---|---|")
byk = defaultdict(list)
for r in agg["network"]:
    byk[(r["tool"], r["variant"])].append(r)
for (t, v), rs in byk.items():
    att = sum(r.get("inet_attempt_count", 0) for r in rs)
    tg = sorted({x for r in rs for x in r.get("inet_connect_attempts", [])})[:3]
    ux = sorted({x for r in rs for x in r.get("unix_connects", []) if x})[:2]
    w(f"| {t} | {v} | {', '.join(r['step'] for r in rs)} | {att} | {'; '.join(tg) or '-'} | {', '.join(ux) or '-'} |")
w("\n## Language coverage on the mixed corpus (>=1 symbol / >=1 call edge from the 6 largest files per language)\n")
w("| language | files | dctx sym/edge | Trail sym/edge | graphify sym/edge |")
w("|---|---|---|---|---|")
for lang, d in cov.items():
    cells = []
    for t in TOOLS:
        e = d.get(t)
        cells.append("n/a" if not e else f"{'Y' if e['symbol_ok'] else 'n'}/{'Y' if e['call_edge_ok'] else 'n'} ({e['symbols']}/{e['call_edges']})")
    w(f"| {lang} | {next(iter(d.values()))['files']} | " + " | ".join(cells) + " |")
open(os.path.join(RD, "headline.md"), "w").write("\n".join(L) + "\n")
print("wrote", os.path.join(RD, "aggregate.json"), os.path.join(RD, "headline.md"))
