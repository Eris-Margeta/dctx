#!/usr/bin/env python3
"""Phase 5: call-graph (callers, depth 1) and find-definition accuracy against the adjudicated gold set.

usage: phase_accuracy.py CORPUS [CORPUS...]
Gold: select_targets.py output (oracle in goldlib.py). Writes results/accuracy.jsonl.
"""
import json
import os
import re
import sys

from benchlib import W, WORK, RES, PRISTINE, run, append_jsonl, loadavg
from goldlib import Corpus


def span(s):
    m = re.match(r"L(\d+)(?:-L(\d+))?", s or "")
    if not m:
        return (0, 10 ** 9)
    a = int(m.group(1))
    return (a, int(m.group(2) or a))


def judge(hits, gold_sites, all_sites, name_level):
    """hits: list of (path, start, end). Returns tp, fp, unjudged hit lists and found/missed gold sites."""
    tp, fp, unj = [], [], []
    for h in hits:
        path, a, b = h
        inside = [s for s in all_sites if s["file"] == path and a <= s["line"] <= b]
        good = [s for s in inside if s["_cls"] == "assigned" or
                (name_level and s["_cls"] in ("other", "ambiguous_self"))]
        if good:
            tp.append(h)
        elif any(s["_cls"] in ("ambiguous", "ambiguous_self") for s in inside):
            unj.append(h)
        else:
            fp.append({"hit": h, "sites_in_span": [(s["line"], s["_cls"], s.get("bound_to")) for s in inside]})
    found, missed = [], []
    for s in gold_sites:
        if any(h[0] == s["file"] and h[1] <= s["line"] <= h[2] for h in hits):
            found.append(s)
        else:
            missed.append(s)
    return tp, fp, unj, found, missed


def main(corpus):
    with open(os.path.join(RES, "targets", f"{corpus}.targets.json")) as f:
        targets = json.load(f)["targets"]
    td, dd = os.path.join(WORK, "trail", corpus), os.path.join(WORK, "dctx", corpus)
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    for i, t in enumerate(targets):
        D = t["def"]
        name = D["name"]
        name_level = t["category"] == "interface"
        g = t["gold"]
        all_sites = ([dict(s, _cls="assigned") for s in g["assigned"]] +
                     [dict(s, _cls="ambiguous_self" if s.get("recv") in ("self", "this", "Self") else "ambiguous")
                      for s in g["ambiguous"]] +
                     [dict(s, _cls="other") for s in g["other_def"]])
        # name-level (interface/trait dispatch) gold: every site bound to some in-repo definition of the
        # name, plus self/this calls; receiver calls that could reach a foreign type stay unjudged.
        gold = ([s for s in all_sites if s["_cls"] in ("assigned", "other", "ambiguous_self")] if name_level
                else [s for s in all_sites if s["_cls"] == "assigned"])
        row = {"corpus": corpus, "i": i, "category": t["category"], "name": name, "file": D["file"],
               "line": D["line"], "kind": D["kind"], "container": D["container"],
               "n_defs_same_name": t["n_defs_same_name"], "name_level": name_level,
               "gold_n": len(gold), "ambiguous_n": len(g["ambiguous"]), "other_def_n": len(g["other_def"])}
        # ---------------- Trail
        r = run(["trail", "callers", name, "--json"], td, tool="trail", corpus=corpus, label=f"acc_callers_{i}_{name}",
                log="acc_runs.jsonl")
        try:
            tj = json.loads(r["_stdout"])
        except ValueError:
            tj = {"matches": []}
        matches = tj.get("matches", [])
        sel = []
        for m in matches:
            s = m.get("symbol", {})
            a, b = span(s.get("span"))
            if name_level or (s.get("path") == D["file"] and a - 3 <= D["line"] <= b):
                sel.append(m)
        row["trail_def_matched"] = bool(sel)
        row["trail_matches_n"] = len(matches)
        hits_calls, hits_any = [], []
        for m in sel:
            for h in m.get("hits", []):
                a, b = span(h.get("span"))
                tup = (h.get("path"), a, b)
                hits_any.append(tup)
                if h.get("relation") == "calls":
                    hits_calls.append(tup)
        tp, fp, unj, found, missed = judge(hits_calls, gold, all_sites, name_level)
        _, _, _, found_any, _ = judge(hits_any, gold, all_sites, name_level)
        row.update(trail_hits=len(hits_calls), trail_hits_any_relation=len(hits_any), trail_tp=len(tp),
                   trail_fp=len(fp), trail_unjudged=len(unj), trail_found=len(found),
                   trail_found_any_relation=len(found_any),
                   trail_fp_list=fp[:15], trail_missed=[(s["file"], s["line"], s["text"][:100]) for s in missed][:15])
        # Trail find-definition: ask
        r = run(["trail", "ask", name, "--json"], td, tool="trail", corpus=corpus, label=f"acc_ask_{i}_{name}",
                log="acc_runs.jsonl", keep=False)
        rank = None
        try:
            for k, h in enumerate(json.loads(r["_stdout"]).get("hits", []), 1):
                p = h.get("pointer", "")
                path, _, sp = p.rpartition(":")
                a, b = span(sp)
                if path == D["file"] and a - 3 <= D["line"] <= b:
                    rank = k
                    break
        except ValueError:
            pass
        row["trail_ask_def_rank"] = rank
        # ---------------- dctx
        r = run(["dctx", "map", "find", name, "--json", "--limit", "10", "--budget", "2000"], dd, tool="dctx",
                corpus=corpus, label=f"acc_find_{i}_{name}", log="acc_runs.jsonl", extra_env=env)
        did, rank = None, None
        try:
            recs = json.loads(r["_stdout"]).get("records", [])
        except ValueError:
            recs = []
        for k, rec in enumerate(recs, 1):
            lines = rec.get("lines") or [0, 0]
            if rec.get("path") == D["file"] and lines[0] - 3 <= D["line"] <= lines[1]:
                rank, did = k, rec.get("id")
                break
        row["dctx_find_def_rank"] = rank
        row["dctx_find_records"] = len(recs)
        row["dctx_id"] = did
        hits = []
        if did:
            r = run(["dctx", "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--limit", "40", "--json",
                     "--budget", "2000"], dd, tool="dctx", corpus=corpus, label=f"acc_neighbors_{i}_{name}",
                    log="acc_runs.jsonl", extra_env=env)
            try:
                for rec in json.loads(r["_stdout"]).get("records", []):
                    lines = rec.get("lines") or [0, 10 ** 9]
                    hits.append((rec.get("path"), lines[0], lines[1]))
            except ValueError:
                pass
        tp, fp, unj, found, missed = judge(hits, gold, all_sites, name_level)
        row.update(dctx_hits=len(hits), dctx_tp=len(tp), dctx_fp=len(fp), dctx_unjudged=len(unj),
                   dctx_found=len(found))
        append_jsonl("accuracy.jsonl", row)
    print("done", corpus, flush=True)


if __name__ == "__main__":
    for c in sys.argv[1:]:
        append_jsonl("phases.jsonl", {"phase": "accuracy", "corpus": c, "event": "start", "load1": loadavg()})
        main(c)
        append_jsonl("phases.jsonl", {"phase": "accuracy", "corpus": c, "event": "end", "load1": loadavg()})
