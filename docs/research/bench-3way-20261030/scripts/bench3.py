#!/usr/bin/env python3
"""3-way benchmark driver: dctx vs Trail-Code vs graphify (code-only, no LLM).

usage: bench3.py [--corpora a,b,...] [--phases corpus,canary,net] [--cold-reps N]
Environment: B3_ROOT, B3_RESULTS, B3_DCTX (see bench3lib.py). Everything runs sequentially (one heavy process at a time).
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time

import bench3lib as L
from bench3lib import run, append_jsonl, median, fresh_copy, dir_bytes, git_touched

TOOLS = ["dctx", "trail", "graphify"]
GOLD = ["flask", "ripgrep", "pocketbase", "trailcode"]
ALL_CORPORA = GOLD + ["mixed", "docs"]
WARM = 5
CFG = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpora.json")))
CANARIES = ["CANARY_ENV_7f3a9c21d0", "CANARY_ENVPW_44e1b7", "CANARY_ENVLOCAL_b81d44e0a3", "CANARY_SETTINGS_5c0e92aa17",
            "CANARY_PROD_91ab33c0f2"]


# ------------------------------------------------------------------ context
class Ctx:
    def __init__(self, tool, corpus, d):
        self.tool, self.corpus, self.dir = tool, corpus, d
        self.cache = os.path.join(L.CACHE, f"{tool}-{corpus}")
        self.env = {"DCTX_CACHE_DIR": self.cache} if tool == "dctx" else {}
        self._g = None

    def r(self, argv, label, rep=0, **kw):
        kw.setdefault("env_extra", self.env)
        return run(argv, self.dir, tool=self.tool, corpus=self.corpus, label=label, rep=rep, **kw)

    def graph(self):
        if self._g is None:
            p = os.path.join(self.dir, "graphify-out", "graph.json")
            self._g = json.load(open(p)) if os.path.exists(p) else {"nodes": [], "links": []}
        return self._g


def sh_out(argv, cwd=None):
    return subprocess.run(argv, cwd=cwd, capture_output=True, text=True).stdout


def comment_for(path):
    ext = os.path.splitext(path)[1]
    if ext in (".py", ".sh", ".bash", ".rb", ".toml", ".yaml", ".yml", ".ex", ".exs"):
        return "# bench3 unrelated edit"
    if ext in (".md", ".rst", ".txt"):
        return "<!-- bench3 unrelated edit -->"
    return "// bench3 unrelated edit"


def largest(corpus, ext, prefix=""):
    best = (0, None)
    root = os.path.join(L.CORP, corpus)
    for dp, dns, fns in os.walk(root):
        if ".git" in dns:
            dns.remove(".git")
        for fn in fns:
            if fn.endswith(ext):
                p = os.path.join(dp, fn)
                rel = os.path.relpath(p, root)
                if rel.startswith(prefix) and os.path.getsize(p) > best[0]:
                    best = (os.path.getsize(p), rel)
    return best[1]


def probe_for(corpus):
    """Per-corpus query probes. Gold corpora use the first 'hub' target of select_targets (seeded)."""
    if corpus in GOLD:
        t = next(x for x in json.load(open(os.path.join(L.R, "targets", f"{corpus}.targets.json")))["targets"]
                 if x["category"] == "hub")
        D = t["def"]
        ext = os.path.splitext(D["file"])[1]
        files = sh_out(["git", "-c", "safe.directory=*", "-C", os.path.join(L.CORP, corpus), "ls-files", "*" + ext]).split("\n")
        tdir = os.path.dirname(D["file"])
        edit = next((f for f in sorted(files) if f and os.path.dirname(f) != tdir and "test" not in f.lower() and "/" in f),
                    files[0])
        return {"sym": D["name"], "file": D["file"], "line": D["line"], "lit": D["name"], "edit": edit, "callers": True,
                "name_level": t["category"] == "interface"}
    if corpus == "mixed":
        return {"sym": "map", "file": "lua/lume.lua", "line": None, "lit": "clamp", "edit": "shell/install.sh", "callers": False}
    if corpus == "docs":
        f = largest("docs", ".rst", "devguide/")
        e = largest("docs", ".md", "rust-book/")
        return {"sym": "release", "file": f, "line": None, "lit": "backport", "edit": e, "callers": False}
    raise KeyError(corpus)


# ------------------------------------------------------------------ dctx capability probe
# Capability probe result, shared with phases that do not receive caps.
CAPS = {}


def dctx_caps():
    d = fresh_copy("dctx", "canary-tracked", "-caps")
    ctx = Ctx("dctx", "canary-tracked-caps", d)
    shutil.rmtree(ctx.cache, ignore_errors=True)
    caps = {}
    v = run([L.DCTX, "--version"], d, tool="dctx", corpus="caps", label="version", keep=False, env_extra=ctx.env)
    caps["version"] = v["_out"].decode().strip()
    r = ctx.r([L.DCTX, "map", "find", "helper", "--limit", "3"], "caps_find", keep=False)
    caps["find_rc"] = r["rc"]
    for name, argv in [("skeleton", ["map", "skeleton", "a.py"]), ("grep", ["map", "grep", "helper"]), ("hubs", ["map", "hubs"]),
                       ("depth3", ["map", "map", "a.py", "--depth", "3"]), ("depth4", ["map", "map", "a.py", "--depth", "4"]),
                       ("neighbors_depth2", ["map", "neighbors", "s:a.py#helper", "--depth", "2"]),
                       ("neighbors_depth3", ["map", "neighbors", "s:a.py#helper", "--depth", "3"]),
                       ("neighbors_depth4", ["map", "neighbors", "s:a.py#helper", "--depth", "4"])]:
        r = ctx.r([L.DCTX] + argv, "caps_" + name, keep=False)
        caps[name] = r["rc"] == 0 and r["stdout_bytes"] > 0
    shutil.rmtree(d, ignore_errors=True)
    shutil.rmtree(ctx.cache, ignore_errors=True)
    return caps


# ------------------------------------------------------------------ indexing
def index_steps(ctx, probe):
    t = ctx.tool
    if t == "dctx":
        # dctx has no explicit build verb; the first map query populates DCTX_CACHE_DIR (cold = empty cache dir).
        return [("index", [L.DCTX, "map", "find", probe["sym"], "--limit", "1"])]
    if t == "trail":
        return [("index", ["trail", "build", "."])]
    return [("extract", ["graphify", "extract", ".", "--code-only", "--max-workers", "8"]),
            ("report", ["graphify", "cluster-only", ".", "--no-viz", "--no-label"])]


def index_paths(ctx):
    if ctx.tool == "dctx":
        return [ctx.cache]
    if ctx.tool == "trail":
        return [os.path.join(ctx.dir, "graft"), os.path.join(ctx.dir, ".trail", "cache")]
    return [os.path.join(ctx.dir, "graphify-out")]


def do_index(tool, corpus, probe, reps):
    rows, ctx = [], None
    nreps = reps
    rep = 0
    while rep < nreps:
        d = fresh_copy(tool, corpus)
        ctx = Ctx(tool, corpus, d)
        shutil.rmtree(ctx.cache, ignore_errors=True)
        os.makedirs(ctx.cache, exist_ok=True) if tool == "dctx" else None
        steps, ok = [], True
        for label, argv in index_steps(ctx, probe):
            r = ctx.r(argv, "cold_" + label, rep=rep, sample=True, timeout=3600, keep=(rep == 0))
            steps.append(r)
            if r["rc"] != 0:
                ok = False
                break
        row = {"rep": rep, "ok": ok, "rc": [s["rc"] for s in steps], "wall_s": round(sum(s["wall_s"] for s in steps), 4),
               "user_s": round(sum(s["user_s"] for s in steps), 3), "sys_s": round(sum(s["sys_s"] for s in steps), 3),
               "maxrss_kb": max(s["maxrss_kb"] for s in steps),
               "peak_tree_rss_kb": max(s.get("peak_tree_rss_kb", 0) for s in steps),
               "peak_tree_pss_kb": max(s.get("peak_tree_pss_kb", 0) for s in steps),
               "step_walls": {s["label"]: s["wall_s"] for s in steps},
               "stderr_tail": steps[-1]["_err"][-300:].decode("utf-8", "replace") if not ok else ""}
        rows.append(row)
        if rep == 0 and ok and row["wall_s"] > 30:
            nreps = 1
        if not ok:
            break
        rep += 1
    ctx_ok = rows[-1]["ok"]
    size = {"bytes": 0, "files": 0}
    for p in index_paths(ctx):
        if os.path.isdir(p):
            b, n = dir_bytes(p)
            size["bytes"] += b
            size["files"] += n
    extra = {}
    if tool == "graphify" and ctx_ok:
        g = ctx.graph()
        extra = {"nodes": len(g["nodes"]), "edges": len(g["links"])}
    if tool == "trail" and ctx_ok:
        m = re.search(rb"wiring: (\d+) nodes.*?, (\d+) edges", steps[-1]["_out"] + steps[-1]["_err"])
        if m:
            extra = {"nodes": int(m.group(1)), "edges": int(m.group(2))}
    summ = {"tool": tool, "corpus": corpus, "ok": ctx_ok, "n_reps": len(rows), "reps": rows,
            "cold_wall_s_median": median([r["wall_s"] for r in rows if r["ok"]]),
            "maxrss_kb_median": median([r["maxrss_kb"] for r in rows if r["ok"]]),
            "peak_tree_pss_kb_median": median([r["peak_tree_pss_kb"] for r in rows if r["ok"]]),
            "peak_tree_rss_kb_median": median([r["peak_tree_rss_kb"] for r in rows if r["ok"]]),
            "index_bytes": size["bytes"], "index_files": size["files"], "repo_touched": git_touched(ctx.dir), **extra}
    append_jsonl("index.jsonl", summ)
    return ctx, summ


# ------------------------------------------------------------------ id resolution
def norm_label(s):
    return re.sub(r"^[.]|\(\)$", "", s or "").strip()


def gfy_ids(ctx, name, file, line, name_level):
    g = ctx.graph()
    cands = []
    for n in g["nodes"]:
        if norm_label(n.get("label")) != name:
            continue
        loc = n.get("source_location") or ""
        m = re.match(r"L(\d+)", loc)
        ln = int(m.group(1)) if m else 10 ** 6
        cands.append((n.get("source_file") == file, abs(ln - (line or 0)), n["id"], n.get("source_file"), ln))
    if name_level:
        return [c[2] for c in cands][:12]
    best = [c for c in cands if c[0]]
    best.sort(key=lambda c: c[1])
    return [best[0][2]] if best and best[0][1] <= 5 else []


def gfy_file_node(ctx, file):
    g = ctx.graph()
    base = os.path.basename(file)
    for n in g["nodes"]:
        if n.get("source_file") == file and norm_label(n.get("label")) in (base, file):
            return n["id"]
    return None


def dctx_id(ctx, name, file, line):
    r = ctx.r([L.DCTX, "map", "find", name, "--json", "--limit", "40", "--budget", "2000"], f"resolve_{name}", keep=False,
              log="resolve.jsonl")
    best = None
    try:
        recs = json.loads(r["_out"]).get("records", [])
    except ValueError:
        recs = []
    for rec in recs:
        rid = rec.get("id", "")
        tail = rid.split("#", 1)[-1].split("::")[-1].split("~")[0]
        if rec.get("path") == file and tail == name:
            dist = abs((rec.get("lines") or [0])[0] - (line or 0))
            if best is None or dist < best[0]:
                best = (dist, rid)
    return best[1] if best and (line is None or best[0] <= 5) else None


# ------------------------------------------------------------------ query phase
def build_queries(ctx, probe, caps):
    t, sym, f, lit = ctx.tool, probe["sym"], probe["file"], probe["lit"]
    q = {}
    na = lambda why: {"supported": False, "why": why}
    if t == "dctx":
        did = dctx_id(ctx, sym, f, probe["line"]) if probe["callers"] else None
        q["find"] = {"argv": [L.DCTX, "map", "find", sym, "--limit", "10"]}
        if did:
            q["callers1"] = {"argv": [L.DCTX, "map", "neighbors", did, "--dir", "in", "--edge", "calls"]}
            q["callers2"] = ({"argv": [L.DCTX, "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--depth", "2"]}
                             if caps.get("neighbors_depth2") else na("neighbors --depth 2 unsupported by this build"))
        else:
            q["callers1"] = q["callers2"] = na("no callers target for this corpus" if not probe["callers"] else "dctx id not resolved")
        if caps.get("skeleton"):
            q["skeleton"] = {"argv": [L.DCTX, "map", "skeleton", f]}
        else:
            q["skeleton"] = {"argv": [L.DCTX, "map", "map", f, "--depth", "2", "--limit", "40"],
                             "via": "fallback: map map FILE --depth 2 (no `map skeleton` in this build)"}
        q["overview"] = ({"argv": [L.DCTX, "map", "hubs"], "via": "dctx map hubs (bounded overview: most-called symbols + per-directory counts)"}
                         if caps.get("hubs") else
                         {"argv": [L.DCTX, "pack", "."], "via": "dctx pack . (full snapshot, not a summary; no tree-only/overview verb in this build)"})
        q["grep"] = ({"argv": [L.DCTX, "map", "grep", lit]} if caps.get("grep") else na("`map grep` unsupported by this build"))
    elif t == "trail":
        q["find"] = {"argv": ["trail", "ask", sym]}
        if probe["callers"]:
            q["callers1"] = {"argv": ["trail", "callers", sym]}
            q["callers2"] = {"argv": ["trail", "callers", sym, "-d", "2"]}
            q["callers1_norefresh"] = {"argv": ["trail", "callers", sym, "--no-refresh"]}
        else:
            q["callers1"] = q["callers2"] = na("no callers target for this corpus")
        q["skeleton"] = {"argv": ["trail", "skeleton", f]}
        q["overview"] = {"argv": ["trail", "map"]}
        q["grep"] = {"argv": ["trail", "grep", lit, "--fixed"]}
    else:
        if not os.path.exists(os.path.join(ctx.dir, "graphify-out", "graph.json")):
            return {k: na("no graph.json (nothing extracted in code-only mode)") for k in
                    ("find", "callers1", "callers2", "skeleton", "overview", "grep")}
        q["find"] = {"argv": ["graphify", "query", sym, "--budget", "2000"]}
        if probe["callers"]:
            ids = gfy_ids(ctx, sym, f, probe["line"], probe.get("name_level"))
            if ids:
                q["callers1"] = {"argv": ["graphify", "affected", ids[0], "--relation", "calls", "--depth", "1"]}
                q["callers2"] = {"argv": ["graphify", "affected", ids[0], "--relation", "calls", "--depth", "2"]}
            else:
                q["callers1"] = q["callers2"] = na("graphify node id not resolved")
        else:
            q["callers1"] = q["callers2"] = na("no callers target for this corpus")
        fid = gfy_file_node(ctx, f)
        q["skeleton"] = ({"argv": ["graphify", "explain", fid], "via": "approximate: explain <file node> lists its connections, not signatures"}
                         if fid else na("file node not in graph"))
        rp = os.path.join(ctx.dir, "graphify-out", "GRAPH_REPORT.md")
        q["overview"] = ({"argv": ["cat", "graphify-out/GRAPH_REPORT.md"], "via": "reads GRAPH_REPORT.md written by cluster-only"}
                         if os.path.exists(rp) else na("no GRAPH_REPORT.md"))
        q["grep"] = na("no literal-search verb")
        q["god_nodes"] = {"argv": ["graphify", "god-nodes", "--top", "10"], "via": "extra: hubs list"}
    for k, v in q.items():
        v.setdefault("supported", True)
    return q


def query_phase(ctx, probe, caps):
    qs = build_queries(ctx, probe, caps)
    summary = {}
    for label, spec in qs.items():
        row = {"tool": ctx.tool, "corpus": ctx.corpus, "label": label, "supported": spec["supported"],
               "via": spec.get("via"), "why": spec.get("why")}
        if spec["supported"]:
            ctx.r(spec["argv"], f"warmup_{label}", rep=-1, keep=False, log="warmup.jsonl")
            rs = [ctx.r(spec["argv"], f"warm_{label}", rep=i, keep=(i == 0)) for i in range(WARM)]
            row.update(rc=rs[0]["rc"], all_rc_equal=len({r["rc"] for r in rs}) == 1,
                       wall_s_median=median([r["wall_s"] for r in rs]), walls=[r["wall_s"] for r in rs],
                       maxrss_kb_median=median([r["maxrss_kb"] for r in rs]), stdout_bytes=rs[0]["stdout_bytes"],
                       est_tokens=rs[0]["est_tokens"], identical_across_runs=len({r["stdout_sha256"] for r in rs}) == 1,
                       sha=rs[0]["stdout_sha256"], sha_nohdr=rs[0]["stdout_sha256_nohdr"], argv=" ".join(spec["argv"])[:200])
            if label == "find" or label == "skeleton":
                row["head"] = rs[0]["_out"][:300].decode("utf-8", "replace")
        append_jsonl("queries.jsonl", row)
        summary[label] = row
    return qs, summary


def edit_phase(ctx, probe, qs, summary):
    """Unrelated edit: append a comment line to a file in another directory; first query after the edit (refresh cost),
    then every query once more; compare stdout hashes with the pre-edit warm runs."""
    p = os.path.join(ctx.dir, probe["edit"])
    bak = p + ".b3bak"
    shutil.copy2(p, bak)
    with open(p, "a") as f:
        f.write("\n" + comment_for(probe["edit"]) + "\n")
    out = {"tool": ctx.tool, "corpus": ctx.corpus, "edit_file": probe["edit"]}
    if ctx.tool == "graphify" and os.path.exists(os.path.join(ctx.dir, "graphify-out", "graph.json")):
        r = ctx.r(["graphify", "update", "."], "post_edit_update", timeout=1800)
        out["post_edit_update_s"] = r["wall_s"]
        out["post_edit_update_rc"] = r["rc"]
        ctx._g = None
    first = qs.get("find")
    if first and first["supported"]:
        r = ctx.r(first["argv"], "post_edit_first_find", keep=False)
        out["post_edit_first_find_s"] = r["wall_s"]
    same, diff, same_nh = [], [], []
    for label, spec in qs.items():
        if not spec["supported"] or not summary[label].get("sha"):
            continue
        r = ctx.r(spec["argv"], f"post_edit_{label}", keep=False)
        (same if r["stdout_sha256"] == summary[label]["sha"] else diff).append(label)
        if r["stdout_sha256_nohdr"] == summary[label]["sha_nohdr"]:
            same_nh.append(label)
    out["identical_after_edit"] = sorted(same)
    out["changed_after_edit"] = sorted(diff)
    out["identical_after_edit_ignoring_header_line"] = sorted(same_nh)
    shutil.move(bak, p)
    append_jsonl("determinism.jsonl", out)
    return out


# ------------------------------------------------------------------ accuracy phase (gold corpora)
def span(s):
    m = re.match(r"L(\d+)(?:-L(\d+))?", s or "")
    if not m:
        return (0, 10 ** 9)
    a = int(m.group(1))
    return (a, int(m.group(2) or a))


def judge(hits, gold_sites, all_sites, name_level):
    tp, fp, unj = [], [], []
    for h in hits:
        path, a, b = h
        inside = [s for s in all_sites if s["file"] == path and a <= s["line"] <= b]
        good = [s for s in inside if s["_cls"] == "assigned" or (name_level and s["_cls"] in ("other", "ambiguous_self"))]
        if good:
            tp.append(h)
        elif any(s["_cls"] in ("ambiguous", "ambiguous_self") for s in inside):
            unj.append(h)
        else:
            fp.append(h)
    found = [s for s in gold_sites if any(h[0] == s["file"] and h[1] <= s["line"] <= h[2] for h in hits)]
    return tp, fp, unj, found


GFY_LINE = re.compile(r"^- .* \[(\w+)\] (.+?):L(\d+)\s*$")


def accuracy_phase(ctxs, corpus):
    targets = json.load(open(os.path.join(L.R, "targets", f"{corpus}.targets.json")))["targets"]
    for i, t in enumerate(targets):
        D = t["def"]
        name = D["name"]
        name_level = t["category"] == "interface"
        g = t["gold"]
        all_sites = ([dict(s, _cls="assigned") for s in g["assigned"]] +
                     [dict(s, _cls="ambiguous_self" if s.get("recv") in ("self", "this", "Self") else "ambiguous") for s in g["ambiguous"]] +
                     [dict(s, _cls="other") for s in g["other_def"]])
        gold = ([s for s in all_sites if s["_cls"] in ("assigned", "other", "ambiguous_self")] if name_level
                else [s for s in all_sites if s["_cls"] == "assigned"])
        row = {"corpus": corpus, "i": i, "category": t["category"], "name": name, "file": D["file"], "line": D["line"],
               "name_level": name_level, "gold_n": len(gold), "n_defs_same_name": t["n_defs_same_name"]}
        # ---- trail
        c = ctxs.get("trail")
        if c:
            r = c.r(["trail", "callers", name, "--json"], f"acc_callers_{i}", log="acc_runs.jsonl", keep=False)
            try:
                matches = json.loads(r["_out"]).get("matches", [])
            except ValueError:
                matches = []
            hits, sel_n = [], 0
            for m in matches:
                s = m.get("symbol", {})
                a, b = span(s.get("span"))
                if name_level or (s.get("path") == D["file"] and a - 3 <= D["line"] <= b):
                    sel_n += 1
                    for h in m.get("hits", []):
                        if h.get("relation") == "calls":
                            ha, hb = span(h.get("span"))
                            hits.append((h.get("path"), ha, hb))
            tp, fp, unj, found = judge(hits, gold, all_sites, name_level)
            row["trail"] = {"def_matched": sel_n > 0, "hits": len(hits), "tp": len(tp), "fp": len(fp), "unjudged": len(unj),
                            "found": len(found), "tokens": r["est_tokens"], "wall_s": r["wall_s"]}
        # ---- dctx
        c = ctxs.get("dctx")
        if c:
            did = dctx_id(c, name, D["file"], D["line"])
            ent = {"id_resolved": did is not None}
            if did:
                hits1, hits_all, cursor, pages, tok, wall = [], [], None, 0, 0, 0.0
                while pages < 5:
                    argv = [L.DCTX, "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--limit", "40", "--json", "--budget", "2000"]
                    if cursor:
                        argv += ["--cursor", cursor]
                    r = c.r(argv, f"acc_neighbors_{i}_p{pages}", log="acc_runs.jsonl", keep=False)
                    pages += 1
                    tok += r["est_tokens"]
                    wall += r["wall_s"]
                    try:
                        js = json.loads(r["_out"])
                    except ValueError:
                        break
                    for rec in js.get("records", []):
                        if rec.get("edge") not in (None, "calls"):
                            continue
                        ln = rec.get("lines") or [0, 10 ** 9]
                        h = (rec.get("path"), ln[0], ln[1])
                        hits_all.append(h)
                        if pages == 1:
                            hits1.append(h)
                    cursor = js.get("next")
                    if not cursor or not js.get("more"):
                        break
                tp, fp, unj, found = judge(hits1, gold, all_sites, name_level)
                _, _, _, found_all = judge(hits_all, gold, all_sites, name_level)
                tpa, fpa, _, _ = judge(hits_all, gold, all_sites, name_level)
                ent.update(hits=len(hits1), tp=len(tp), fp=len(fp), unjudged=len(unj), found=len(found), tokens=tok,
                           wall_s=round(wall, 4), pages=pages, hits_allpages=len(hits_all), tp_allpages=len(tpa),
                           fp_allpages=len(fpa), found_allpages=len(found_all))
            row["dctx"] = ent
        # ---- graphify
        c = ctxs.get("graphify")
        if c:
            ids = gfy_ids(c, name, D["file"], D["line"], name_level)
            ent = {"id_resolved": bool(ids)}
            if ids:
                hits, tok, wall = [], 0, 0.0
                for j, nid in enumerate(ids):
                    r = c.r(["graphify", "affected", nid, "--relation", "calls", "--depth", "1"], f"acc_affected_{i}_{j}",
                            log="acc_runs.jsonl", keep=False)
                    tok += r["est_tokens"]
                    wall += r["wall_s"]
                    for line in r["_out"].decode("utf-8", "replace").splitlines():
                        m = GFY_LINE.match(line)
                        if m:
                            ln = int(m.group(3))
                            hits.append((m.group(2), ln - 2, ln + 2))  # call-site line, +-2 for multi-line calls
                tp, fp, unj, found = judge(hits, gold, all_sites, name_level)
                ent.update(hits=len(hits), tp=len(tp), fp=len(fp), unjudged=len(unj), found=len(found), tokens=tok,
                           wall_s=round(wall, 4))
            row["graphify"] = ent
        append_jsonl("accuracy.jsonl", row)


# ------------------------------------------------------------------ coverage (mixed corpus)
def lang_files(lang):
    root = os.path.join(L.CORP, "mixed")
    exts = tuple(CFG["mixed_exts"][lang])
    fs = []
    for dp, dns, fns in os.walk(os.path.join(root, lang)):
        for fn in fns:
            if fn.endswith(exts):
                p = os.path.join(dp, fn)
                fs.append((os.path.getsize(p), os.path.relpath(p, root)))
    fs.sort(key=lambda x: (-x[0], x[1]))
    return [f for _, f in fs]


def coverage_phase(ctxs):
    out = []
    for lang in CFG["mixed_exts"]:
        files_all = lang_files(lang)
        files = files_all[:6]
        for tool, c in ctxs.items():
            ent = {"tool": tool, "lang": lang, "files_total": len(files_all), "files_sampled": len(files), "symbols": 0,
                   "call_edges": 0}
            if not files:
                out.append(ent)
                continue
            syms = []  # (name, id)
            ent["probe"] = {"dctx": "map skeleton FILE --json" if CAPS.get("skeleton") else "map map FILE --depth 2 --limit 40 --json (fallback, no `map skeleton`)",
                            "trail": "skeleton FILE --json", "graphify": "graph.json read directly"}.get(tool)
            if tool == "dctx":
                for f in files:
                    # Same probe shape as Trail's `trail skeleton FILE`: every declaration of the file when the build has `map skeleton`.
                    argv = ([L.DCTX, "map", "skeleton", f, "--json"] if CAPS.get("skeleton")
                            else [L.DCTX, "map", "map", f, "--depth", "2", "--limit", "40", "--json"])
                    # Trail's skeleton answers in one complete listing; dctx pages at its 2,000-token cap, so follow `next` (max 20 pages).
                    recs = []
                    for _page in range(20):
                        r = c.r(argv, "cov_outline", keep=False, log="cov_runs.jsonl")
                        try:
                            js = json.loads(r["_out"])
                        except ValueError:
                            break
                        recs.extend(js.get("records", []))
                        if not (CAPS.get("skeleton") and js.get("next") and js.get("more")):
                            break
                        argv = [L.DCTX, "map", "skeleton", f, "--json", "--cursor", js["next"]]
                    for rec in recs:
                        if rec.get("kind") not in ("module", "doc") and rec.get("id", "").startswith("s:"):
                            syms.append((rec["id"].split("#", 1)[-1], rec["id"]))
                ent["symbols"] = len(syms)
                for nm, sid in syms[:25]:
                    r = c.r([L.DCTX, "map", "neighbors", sid, "--dir", "both", "--edge", "calls", "--json", "--limit", "5"], "cov_edges",
                            keep=False, log="cov_runs.jsonl")
                    try:
                        recs = [x for x in json.loads(r["_out"]).get("records", []) if x.get("edge") == "calls"]
                    except ValueError:
                        recs = []
                    if recs:
                        ent["call_edges"] = len(recs)
                        break
            elif tool == "trail":
                for f in files:
                    r = c.r(["trail", "skeleton", f, "--json"], "cov_outline", keep=False, log="cov_runs.jsonl")
                    try:
                        es = json.loads(r["_out"]).get("entries", [])
                    except ValueError:
                        es = []
                    for e in es:
                        syms.append((e["name"], None))
                ent["symbols"] = len(syms)
                seen = set()
                for nm, _ in syms:
                    if nm in seen:
                        continue
                    seen.add(nm)
                    if len(seen) > 25:
                        break
                    for d in ("in", "out"):
                        r = c.r(["trail", "callers", nm, "--direction", d, "--json"], "cov_edges", keep=False, log="cov_runs.jsonl")
                        try:
                            hs = [h for m in json.loads(r["_out"]).get("matches", []) for h in m.get("hits", []) if h.get("relation") == "calls"]
                        except ValueError:
                            hs = []
                        if hs:
                            ent["call_edges"] = len(hs)
                            break
                    if ent["call_edges"]:
                        break
            else:
                g = c.graph()
                fset = set(files)
                nodes = {n["id"]: n for n in g["nodes"]}
                ent["symbols"] = sum(1 for n in g["nodes"] if n.get("source_file") in fset and n.get("file_type") == "code"
                                     and (n.get("_callable") or n.get("_callable_class") or str(n.get("label", "")).endswith("()")))
                ent["call_edges"] = sum(1 for e in g["links"] if e.get("relation") == "calls" and
                                        (nodes.get(e["source"], {}).get("source_file") in fset or nodes.get(e["target"], {}).get("source_file") in fset))
                ent["note"] = "graph.json read directly (graphify has no per-file listing verb)"
            out.append(ent)
            append_jsonl("coverage.jsonl", ent)
    return out


# ------------------------------------------------------------------ per-corpus pipeline
def corpus_pipeline(corpus, caps, cold_reps):
    probe = probe_for(corpus)
    append_jsonl("probes.jsonl", dict(probe, corpus=corpus))
    ctxs = {}
    for tool in TOOLS:
        print(f"[{time.strftime('%H:%M:%S')}] {corpus}/{tool}: index", flush=True)
        ctx, summ = do_index(tool, corpus, probe, cold_reps)
        print(f"   cold={summ['cold_wall_s_median']}s ok={summ['ok']} idx={summ['index_bytes']}B", flush=True)
        if not summ["ok"] and tool == "graphify" and corpus != "docs":
            pass
        if tool == "graphify" and not os.path.exists(os.path.join(ctx.dir, "graphify-out", "graph.json")):
            ctxs[tool] = ctx  # queries will be reported unsupported
        elif not summ["ok"]:
            append_jsonl("queries.jsonl", {"tool": tool, "corpus": corpus, "label": "ALL", "supported": False, "why": "index failed"})
            continue
        ctxs[tool] = ctx
    # queries + determinism per tool (index state still on disk)
    plans = {}
    for tool, ctx in ctxs.items():
        print(f"[{time.strftime('%H:%M:%S')}] {corpus}/{tool}: queries", flush=True)
        qs, summary = query_phase(ctx, probe, caps)
        plans[tool] = (qs, summary)
    if corpus in GOLD:
        print(f"[{time.strftime('%H:%M:%S')}] {corpus}: accuracy", flush=True)
        accuracy_phase({t: c for t, c in ctxs.items() if t != "graphify" or os.path.exists(os.path.join(c.dir, "graphify-out", "graph.json"))}, corpus)
    if corpus == "mixed":
        print(f"[{time.strftime('%H:%M:%S')}] mixed: coverage", flush=True)
        coverage_phase({t: c for t, c in ctxs.items() if t != "graphify" or os.path.exists(os.path.join(c.dir, "graphify-out", "graph.json"))})
    for tool, ctx in ctxs.items():
        print(f"[{time.strftime('%H:%M:%S')}] {corpus}/{tool}: edit/determinism", flush=True)
        edit_phase(ctx, probe, *plans[tool])
    for tool, ctx in ctxs.items():
        shutil.rmtree(ctx.dir, ignore_errors=True)
        shutil.rmtree(ctx.cache, ignore_errors=True)


# ------------------------------------------------------------------ privacy canary
def canary_phase():
    for fx in ("canary-tracked", "canary-ignored"):
        probe = {"sym": "helper", "file": "a.py", "line": 1, "lit": "CANARY", "edit": "a.py", "callers": False}
        for tool in TOOLS:
            ctx, summ = do_index(tool, fx, probe, 1)
            outs = {}
            qs = []
            if tool == "dctx":
                qs = [("find_canary", [L.DCTX, "map", "find", "CANARY", "--limit", "40"]), ("find_apikey", [L.DCTX, "map", "find", "API_KEY", "--limit", "40"]),
                      ("find_secret", [L.DCTX, "map", "find", "SECRET", "--limit", "40"]), ("find_helper", [L.DCTX, "map", "find", "helper"]),
                      ("outline_a", [L.DCTX, "map", "map", "a.py", "--depth", "2"]), ("pack", [L.DCTX, "pack", "."]),
                      ("read_env", [L.DCTX, "map", "read", "m:.env"]), ("read_settings", [L.DCTX, "map", "read", "m:settings.env"])]
            elif tool == "trail":
                qs = [("ask_canary", ["trail", "ask", "CANARY", "--source"]), ("ask_apikey", ["trail", "ask", "API_KEY", "--source"]),
                      ("ask_secret", ["trail", "ask", "SECRET", "--source"]), ("grep_canary", ["trail", "grep", "CANARY", "--fixed"]),
                      ("map", ["trail", "map"]), ("skeleton_a", ["trail", "skeleton", "a.py"]), ("callers_helper", ["trail", "callers", "helper"])]
            else:
                qs = [("query_canary", ["graphify", "query", "CANARY", "--budget", "2000"]), ("query_apikey", ["graphify", "query", "API_KEY", "--budget", "2000"]),
                      ("query_secret", ["graphify", "query", "SECRET", "--budget", "2000"]), ("god_nodes", ["graphify", "god-nodes", "--top", "20"])]
                if os.path.exists(os.path.join(ctx.dir, "graphify-out", "GRAPH_REPORT.md")):
                    qs.append(("report", ["cat", "graphify-out/GRAPH_REPORT.md"]))
            leaks = {}
            names = {}
            for label, argv in qs:
                r = ctx.r(argv, "canary_" + label, keep=True)
                blob = r["_out"] + r["_err"]
                hit = sorted({m.decode() for m in re.findall(rb"CANARY_[A-Za-z0-9]+", blob)})  # query echo "CANARY" alone does not count
                if hit:
                    leaks[label] = hit
                if not any(".env" in a or "settings" in a for a in argv):
                    names[label] = any(x in blob for x in (b".env", b"settings.env", b"prod.env"))
            art = {}
            for p in index_paths(ctx):
                if not os.path.isdir(p):
                    continue
                for dp, dns, fns in os.walk(p):
                    for fn in fns:
                        fp = os.path.join(dp, fn)
                        try:
                            data = open(fp, "rb").read()
                        except OSError:
                            continue
                        found = sorted({m.decode() for m in re.findall(rb"CANARY_[A-Za-z0-9]+", data)})
                        if found:
                            art[os.path.relpath(fp, ctx.dir)[:120]] = found
            append_jsonl("canary.jsonl", {"tool": tool, "fixture": fx, "index_ok": summ["ok"], "queries": [q[0] for q in qs],
                                          "leak_in_output": bool(leaks), "leaks_by_query": leaks, "leak_in_index": bool(art),
                                          "index_artifacts_with_canary": art, "env_file_names_in_outputs": sorted(k for k, v in names.items() if v)})
            shutil.rmtree(ctx.dir, ignore_errors=True)
            shutil.rmtree(ctx.cache, ignore_errors=True)


# ------------------------------------------------------------------ network audit
def net_phase(caps):
    probe = probe_for("flask")
    sym, f = probe["sym"], probe["file"]
    for variant, dnt in (("dnt1", True), ("default_env", False)):
        home = os.path.join(L.R, f"home-net-{variant}")
        shutil.rmtree(home, ignore_errors=True)
        os.makedirs(home)
        for tool in TOOLS:
            d = fresh_copy(tool, "flask", "-net")
            ctx = Ctx(tool, "flask-net", d)
            shutil.rmtree(ctx.cache, ignore_errors=True)
            if tool == "dctx":
                steps = [("index_find", [L.DCTX, "map", "find", sym, "--limit", "5"]), ("pack", [L.DCTX, "pack", "."])]
            elif tool == "trail":
                steps = [("build", ["trail", "build", "."]), ("callers", ["trail", "callers", sym]), ("ask", ["trail", "ask", sym])]
            else:
                steps = [("extract", ["graphify", "extract", ".", "--code-only", "--max-workers", "2"]), ("query", ["graphify", "query", sym, "--budget", "500"])]
            for label, argv in steps:
                tj = os.path.join(L.TMP, f"net-{tool}-{variant}-{label}.json")
                if os.path.exists(tj):
                    os.unlink(tj)
                r = run(["python3", os.path.join(L.SCRIPTS, "nettrace.py"), tj, "--"] + argv, d, tool=tool, corpus="net", label=f"{variant}_{label}",
                        env_extra=ctx.env, dnt=dnt, home=home, timeout=900, keep=False, log="net_runs.jsonl")
                try:
                    js = json.load(open(tj))
                except (OSError, ValueError):
                    js = {"rc": None, "summary": {"error": "tracer produced no output"}}
                append_jsonl("network.jsonl", {"tool": tool, "variant": variant, "step": label, "rc_traced": js.get("rc"),
                                               "unshare_netns": bool(L.UNSHARE), **js.get("summary", {})})
            shutil.rmtree(d, ignore_errors=True)
            shutil.rmtree(ctx.cache, ignore_errors=True)


# ------------------------------------------------------------------ environment
def collect_env(caps):
    def cmd(a):
        try:
            return subprocess.run(a, capture_output=True, text=True, timeout=30).stdout.strip()
        except Exception as e:
            return f"err {e}"
    env = L.base_env()
    py = os.path.join(L.R, "venv", "bin", "python3")
    d = {
        "date_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host_kernel": cmd(["uname", "-sr"]), "cpu_model": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?"),
        "cpus_pinned": L.CPUS, "nice": 10, "unshare_Urn_net_isolation": bool(L.UNSHARE),
        "load1_at_start": L.loadavg(), "node": cmd(["node", "-v"]), "python": cmd(["python3", "-V"]),
        "dctx_binary": L.DCTX, "dctx_sha256": cmd(["sha256sum", L.DCTX]).split()[0], "dctx_caps": caps,
        "trail_version": cmd([os.path.join(L.R, "trail/node_modules/.bin/trail"), "-v"]),
        "graphify_version": cmd([os.path.join(L.R, "venv/bin/graphify"), "--version"]),
        "graphify_mode": "extract . --code-only --max-workers 8; cluster-only . --no-viz --no-label; GRAPHIFY_BACKEND=none; LLM doc/semantic pass NOT run (no key supplied)",
        "trail_mode": "build . (no --deep, no --lsp), DO_NOT_TRACK=1",
        "env_vars_set_for_tools": sorted(env.keys()),
        "token_estimator": "ceil(stdout_bytes/3.5)",
        "corpora_shas": json.load(open(os.path.join(L.R, "corpora-shas.json"))),
        "pip_freeze": cmd([os.path.join(L.R, "venv/bin/pip"), "freeze"]).splitlines(),
        "npm_trail_pkg_version": json.load(open(os.path.join(L.R, "trail/node_modules/@trailhq/trail-code/package.json")))["version"],
    }
    # wrapper overhead (nice+taskset+unshare+env + process spawn) for a no-op
    ov = [run(["true"], L.R, tool="none", corpus="overhead", label="true", rep=i, keep=False, log="overhead.jsonl")["wall_s"] for i in range(5)]
    d["wrapper_overhead_s_median"] = median(ov)
    json.dump(d, open(os.path.join(L.RES, "env.json"), "w"), indent=1)
    return d


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpora", default=",".join(ALL_CORPORA))
    ap.add_argument("--phases", default="corpus,canary,net")
    ap.add_argument("--cold-reps", type=int, default=int(os.environ.get("B3_COLD_REPS", "3")))
    a = ap.parse_args()
    os.makedirs(L.RES, exist_ok=True)
    for d in (L.WORK, L.CACHE, L.TMP, os.path.join(L.R, "home")):
        os.makedirs(d, exist_ok=True)
    L.detect_unshare()
    print("unshare -Urn usable:", L.UNSHARE, flush=True)
    caps = dctx_caps()
    CAPS.update(caps)
    print("dctx caps:", caps, flush=True)
    collect_env(caps)
    phases = a.phases.split(",")
    if "corpus" in phases:
        for c in a.corpora.split(","):
            L.PHASE = "corpus"
            corpus_pipeline(c, caps, a.cold_reps)
    if "canary" in phases:
        L.PHASE = "canary"
        canary_phase()
    if "net" in phases:
        L.PHASE = "net"
        net_phase(caps)
    print("done", flush=True)


if __name__ == "__main__":
    main()
