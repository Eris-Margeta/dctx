#!/usr/bin/env python3
"""Task-level rerun (70 agent questions, October protocol) for dctx, Trail-Code and graphify.

usage: run_tasks.py [--corpora a,b] [--tools dctx,trail,graphify,dctx-ext] [--keep-work]
Env: B3_ROOT (bench3 root), B3_DCTX (dctx binary), B3_RESULTS (output dir). One heavy process at a time; every
command goes through bench3lib.run: nice -n 10, taskset -c 24-31, unshare -Urn, env -i, scratch HOME, DO_NOT_TRACK=1.
Questions come from the October generator (tasks/oct/scripts/phase_tasks.py build_questions) run on the pinned
October corpora (corpora-oct/); scoring is tasks_score.py (October logic).
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import bench3lib as L  # noqa: E402
import tasks_score as S  # noqa: E402

ROOT = L.R
OCT = os.path.join(ROOT, "tasks", "oct")
sys.path.insert(0, os.path.join(OCT, "scripts"))
import phase_tasks as PT  # noqa: E402  (October question generator; imports goldlib, benchlib)
from goldlib import Corpus  # noqa: E402

CORP_OCT = os.path.join(ROOT, "corpora-oct")
WORKT = os.path.join(ROOT, "work", "tasks")
ALL_CORPORA = S.CORPUS_ORDER
EXPECTED_FACTS = {"flask": 48, "ripgrep": 32, "pocketbase": 36, "trailcode": 64, "dctx": 48, "django": 70, "core": 56}
L.PHASE = "tasks"


# ------------------------------------------------------------------ questions
def questions(corpus):
    c = Corpus(os.path.join(CORP_OCT, corpus))
    tp = os.path.join(OCT, "targets", f"{corpus}.targets.json")
    if corpus == "core":
        tp = os.path.join(ROOT, "tasks", "private", "core.targets.json")
    targets = json.load(open(tp))["targets"]
    qs = PT.build_questions(corpus, c, targets)
    n = sum(len(q["facts"]) for q in qs)
    assert n == EXPECTED_FACTS[corpus], (corpus, n, EXPECTED_FACTS[corpus])
    return c, qs


# ------------------------------------------------------------------ graphify helpers (same conventions as bench3.py)
def norm_label(s):
    return re.sub(r"^[.]|\(\)$", "", s or "").strip()


class G:
    def __init__(self, d):
        p = os.path.join(d, "graphify-out", "graph.json")
        self.g = json.load(open(p)) if os.path.exists(p) else {"nodes": [], "links": []}

    def ids(self, name, file, line):
        cands = []
        for n in self.g["nodes"]:
            if norm_label(n.get("label")) != name:
                continue
            m = re.match(r"L(\d+)", n.get("source_location") or "")
            ln = int(m.group(1)) if m else 10 ** 6
            cands.append((n.get("source_file") == file, abs(ln - (line or 0)), n["id"]))
        best = sorted([c for c in cands if c[0]], key=lambda c: c[1])
        return best[0][2] if best and best[0][1] <= 5 else None

    def file_node(self, file):
        base = os.path.basename(file)
        for n in self.g["nodes"]:
            if n.get("source_file") == file and norm_label(n.get("label")) in (base, file):
                return n["id"]
        return None


# ------------------------------------------------------------------ per-tool command mappings
class Runner:
    def __init__(self, tool, corpus, d, env, gfy=None):
        self.tool, self.corpus, self.d, self.env, self.gfy = tool, corpus, d, env, gfy
        self.outs, self.labels, self.cmds = [], [], []

    def go(self, argv, label, qid):
        r = L.run(argv, self.d, tool=self.tool, corpus=self.corpus, label=f"{qid}_{label}", env_extra=self.env,
                  timeout=300, log="task_runs.jsonl", meta={"qid": qid})
        self.outs.append(r["_out"])
        self.labels.append(label)
        self.cmds.append({"cmd": " ".join(a if a != L.DCTX else "dctx" for a in argv)[:200], "rc": r["rc"],
                          "bytes": r["stdout_bytes"], "wall_s": r["wall_s"]})
        return r


def dctx_find_id(out, t):
    try:
        recs = json.loads(out).get("records", [])
    except ValueError:
        return None
    for rec in recs:
        lines = rec.get("lines") or [0, 0]
        if rec.get("path") == t["def"]["file"] and lines[0] - 3 <= t["def"]["line"] <= lines[1]:
            return rec["id"]
    return recs[0]["id"] if recs else None


def run_dctx(R, q, ext):
    """October mapping. Differences: overview = `dctx map hubs` (October used the unbounded legacy tree).
    ext=True is the supplementary variant that also uses the verbs added since October (skeleton, grep) and --json
    on neighbors (call-line numbers)."""
    D, k, qid = L.DCTX, q["kind"], q["id"]
    js = ["--json"] if ext else []
    if k in ("def_and_callers", "impact", "source", "callees", "collision"):
        t = q["t"]
        r = R.go([D, "map", "find", t["def"]["name"], "--json", "--limit", "10"], "find", qid)
        did = dctx_find_id(r["_out"], t)
        if did:
            if k in ("def_and_callers", "collision"):
                R.go([D, "map", "neighbors", did, "--dir", "in", "--edge", "calls"] + js, "callers", qid)
            elif k == "impact":
                R.go([D, "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--depth", "2"] + js, "impact", qid)
            elif k == "source":
                R.go([D, "map", "read", did, "--json"], "read", qid)
            else:
                R.go([D, "map", "neighbors", did, "--dir", "out", "--edge", "calls"] + js, "callees", qid)
    elif k in ("outline", "exports"):
        if ext:
            R.go([D, "map", "skeleton", q["file"]], "skeleton", qid)
        else:
            R.go([D, "map", "map", q["file"], "--depth", "2", "--limit", "40"], "map", qid)
    elif k == "config":
        if ext:
            R.go([D, "map", "grep", q["key"]], "grep", qid)
        else:
            R.go([D, "map", "find", q["key"], "--limit", "10"], "find", qid)
    elif k == "overview":
        R.go([D, "map", "hubs"], "hubs", qid)
    elif k == "concept":
        R.go([D, "map", "find", q["words"], "--limit", "10"], "find", qid)


def run_trail(R, q):
    k, qid = q["kind"], q["id"]
    if k == "def_and_callers":
        R.go(["trail", "callers", q["t"]["def"]["name"]], "callers", qid)
    elif k == "impact":
        R.go(["trail", "callers", q["t"]["def"]["name"], "-d", "2"], "callers_d2", qid)
    elif k in ("outline", "exports"):
        R.go(["trail", "skeleton", q["file"]], "skeleton", qid)
    elif k == "config":
        R.go(["trail", "grep", q["key"]], "grep", qid)
    elif k == "source":
        R.go(["trail", "ask", q["t"]["def"]["name"], "--source", "--full", "-n", "3"], "ask_source", qid)
    elif k == "callees":
        R.go(["trail", "callers", q["t"]["def"]["name"], "--direction", "out"], "callees", qid)
    elif k == "overview":
        R.go(["trail", "map"], "map", qid)
    elif k == "collision":
        R.go(["trail", "callers", q["qual"]], "callers_qualified", qid)
    elif k == "concept":
        R.go(["trail", "ask", q["words"]], "ask", qid)


def run_graphify(R, q):
    """Closest LLM-free graphify command per question type (documented in the README).
    Node ids are resolved from graphify-out/graph.json by file + line of the gold definition (as bench3.py does);
    when no node matches, the bare name is passed to the command instead."""
    k, qid, g = q["kind"], q["id"], R.gfy
    gf = ["graphify"]
    if k in ("def_and_callers", "impact", "collision", "source", "callees"):
        t = q["t"]
        name, f, ln = t["def"]["name"], t["def"]["file"], t["def"]["line"]
        nid = g.ids(name, f, ln) or name
        if k in ("def_and_callers", "collision"):
            R.go(gf + ["explain", nid], "find", qid)  # definition site; "find"-class: caller facts must come from `affected`
            R.go(gf + ["affected", nid, "--relation", "calls", "--depth", "1"], "callers", qid)
        elif k == "impact":
            R.go(gf + ["explain", nid], "find", qid)
            R.go(gf + ["affected", nid, "--relation", "calls", "--depth", "2"], "impact", qid)
        elif k == "source":
            R.go(gf + ["explain", nid], "explain", qid)  # graphify has no source-reading verb
        else:
            R.go(gf + ["explain", nid, "--all"], "callees", qid)  # outgoing `calls` edges appear as `-->` lines
    elif k in ("outline", "exports"):
        nid = g.file_node(q["file"]) or q["file"]
        R.go(gf + ["explain", nid, "--all"], "explain_file", qid)
    elif k == "config":
        R.go(gf + ["query", q["key"], "--budget", "2000"], "query", qid)  # no literal-search verb
    elif k == "overview":
        R.go(["cat", "graphify-out/GRAPH_REPORT.md"], "report", qid)
    elif k == "concept":
        R.go(gf + ["query", q["words"], "--budget", "2000"], "query", qid)


# ------------------------------------------------------------------ setup per tool/corpus
def fresh(tool, corpus):
    dst = os.path.join(WORKT, tool, corpus)
    shutil.rmtree(dst, ignore_errors=True)
    os.makedirs(dst)
    subprocess.run(["cp", "-a", os.path.realpath(os.path.join(CORP_OCT, corpus)) + "/.", dst], check=True)
    return dst


def setup(tool, corpus):
    """Returns (dir, env, setup_info). Setup time is recorded but not part of the task score."""
    if tool in ("dctx", "dctx-ext"):
        shared = os.path.join(WORKT, "dctx", corpus)
        d = shared if (tool == "dctx-ext" and os.path.isdir(shared)) else fresh("dctx", corpus)
    else:
        d = fresh(tool, corpus)
    env, info = {}, {}
    if tool in ("dctx", "dctx-ext"):
        env = {"DCTX_CACHE_DIR": os.path.join(L.CACHE, f"tasks-dctx-{corpus}")}
        if tool == "dctx":
            shutil.rmtree(env["DCTX_CACHE_DIR"], ignore_errors=True)
            os.makedirs(env["DCTX_CACHE_DIR"])
    elif tool == "trail":
        r = L.run(["trail", "build", "."], d, tool=tool, corpus=corpus, label="setup_build", timeout=3600, log="task_setup.jsonl")
        info = {"setup_rc": r["rc"], "setup_wall_s": r["wall_s"]}
    elif tool == "graphify":
        steps = [["graphify", "extract", ".", "--code-only", "--max-workers", "8"],
                 ["graphify", "cluster-only", ".", "--no-viz", "--no-label"]]
        wall, rcs = 0.0, []
        for i, a in enumerate(steps):
            r = L.run(a, d, tool=tool, corpus=corpus, label=f"setup_{i}", timeout=3600, log="task_setup.jsonl")
            wall += r["wall_s"]
            rcs.append(r["rc"])
            if r["rc"] != 0:
                break
        info = {"setup_rc": rcs, "setup_wall_s": round(wall, 2)}
    return d, env, info


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpora", default=",".join(ALL_CORPORA))
    ap.add_argument("--tools", default="dctx,dctx-ext,trail,graphify")
    ap.add_argument("--keep-work", action="store_true")
    a = ap.parse_args()
    L.detect_unshare()
    os.makedirs(L.RES, exist_ok=True)
    L.append_jsonl("tasks_env.jsonl", {"unshare": L.UNSHARE, "dctx": L.DCTX, "cpus": L.CPUS,
                                       "dctx_version": subprocess.run([L.DCTX, "--version"], capture_output=True, text=True).stdout.strip()})
    tools = a.tools.split(",")
    for corpus in a.corpora.split(","):
        if not os.path.isdir(os.path.join(CORP_OCT, corpus)):
            print("SKIP (corpus missing)", corpus, flush=True)
            continue
        c, qs = questions(corpus)
        for tool in tools:
            d, env, info = setup(tool, corpus)
            gfy = G(d) if tool == "graphify" else None
            ok_graph = tool != "graphify" or bool(gfy.g["nodes"])
            if info:
                L.append_jsonl("tasks_setup.jsonl", {"corpus": corpus, "tool": tool, **info, "graph_nodes": len(gfy.g["nodes"]) if gfy else None})
            for q in qs:
                R = Runner(tool, corpus, d, env, gfy)
                if ok_graph:
                    if tool in ("dctx", "dctx-ext"):
                        run_dctx(R, q, tool == "dctx-ext")
                    elif tool == "trail":
                        run_trail(R, q)
                    else:
                        run_graphify(R, q)
                present, frac, score = S.score_question(q["facts"], R.outs, R.labels)
                nbytes = sum(len(o) for o in R.outs)
                row = {"corpus": corpus, "qid": q["id"], "kind": q["kind"], "question": q["q"], "tool": tool,
                       "calls": len(R.cmds), "bytes": nbytes, "est_tokens": S.est_tokens_total(nbytes),
                       "facts_n": len(present), "facts_found": sum(present), "fraction": round(frac, 3), "score": score,
                       "missing": [f for f, p in zip(q["facts"], present) if not p][:12], "cmds": R.cmds}
                L.append_jsonl("tasks.jsonl", row)
            print("done", corpus, tool, flush=True)
            if not a.keep_work and tool != "dctx":  # the dctx copy is reused by dctx-ext
                shutil.rmtree(d, ignore_errors=True)
        if not a.keep_work:
            shutil.rmtree(os.path.join(WORKT, "dctx", corpus), ignore_errors=True)
            shutil.rmtree(os.path.join(L.CACHE, f"tasks-dctx-{corpus}"), ignore_errors=True)


if __name__ == "__main__":
    main()
