#!/usr/bin/env python3
"""Independent benchmark driver: dctx vs Trail-Code vs graphify on the seeded repos (v1 card 6).

usage: run_indep.py SELECTION_JSON GOLD_DIR QUESTIONS_JSON CORPORA_DIR [--tools dctx,trail,graphify]
Env: B3_ROOT, B3_DCTX, B3_RESULTS (bench3lib). Every command goes through bench3lib.run, the 3-way harness's isolation
(nice, taskset, unshare -Urn, env -i, scratch HOME, DO_NOT_TRACK=1, GRAPHIFY_BACKEND=none), one at a time. Tokens:
tokens.Counter (cl100k_base) over raw stdout. Writes tasks.jsonl, callers.jsonl, setup.jsonl, env.json in B3_RESULTS.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(1, os.path.join(HERE, "..", "bench-3way-20261030", "scripts"))
import adapters as A  # noqa: E402
import bench3lib as L  # noqa: E402
import report  # noqa: E402
import tokens  # noqa: E402

L.PHASE = "independent"


class Session:
    """One tool on one repo copy: runs commands, keeps every stdout of the current question."""

    def __init__(self, tool, repo, d, env, counter):
        self.tool, self.repo, self.d, self.env, self.counter = tool, repo, d, env, counter
        self.outs, self.labels, self.cmds, self.qid = [], [], [], ""

    def start(self, qid):
        self.outs, self.labels, self.cmds, self.qid = [], [], [], qid

    def go(self, argv, label):
        r = L.run(argv, self.d, tool=self.tool, corpus=self.repo, label=f"{self.qid}_{label}", env_extra=self.env, timeout=300,
                  log="indep_runs.jsonl", meta={"qid": self.qid})
        self.outs.append(r["_out"])
        self.labels.append(label)
        self.cmds.append({"cmd": " ".join("dctx" if a == L.DCTX else a for a in argv)[:200], "rc": r["rc"], "bytes": r["stdout_bytes"]})
        return r["_out"]

    def tokens(self):
        return sum(self.counter.count(o) for o in self.outs)


def prepare(tool, repo, corpora):
    """Fresh copy of the pristine repo plus the tool's index; returns (dir, env, setup row)."""
    d = os.path.join(L.WORK, "indep", tool, repo)
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d)
    subprocess.run(["cp", "-a", os.path.join(corpora, repo) + "/.", d], check=True)
    env, steps = {}, []
    if tool == "dctx":
        env = {"DCTX_CACHE_DIR": os.path.join(L.CACHE, f"indep-dctx-{repo}")}
        shutil.rmtree(env["DCTX_CACHE_DIR"], ignore_errors=True)
        os.makedirs(env["DCTX_CACHE_DIR"])
        steps = [[L.DCTX, "map", "hubs"]]          # dctx indexes on the first map query
    elif tool == "trail":
        steps = [["trail", "build", "."]]
    else:
        steps = [["graphify", "extract", ".", "--code-only", "--max-workers", "8"], ["graphify", "cluster-only", ".", "--no-viz", "--no-label"]]
    rcs, wall = [], 0.0
    for i, argv in enumerate(steps):
        r = L.run(argv, d, tool=tool, corpus=repo, label=f"setup_{i}", env_extra=env, timeout=3600, sample=True, log="setup_runs.jsonl")
        rcs.append(r["rc"])
        wall += r["wall_s"]
        if r["rc"]:
            break
    return d, env, {"tool": tool, "repo": repo, "rc": rcs, "wall_s": round(wall, 3)}


def ask(tool, s, q, g):
    if tool == "dctx":
        return A.ask_dctx(s.go, q, L.DCTX)
    if tool == "trail":
        return A.ask_trail(s.go, q)
    return A.ask_graphify(s.go, q, g)


def callers(tool, s, t, g):
    if tool == "dctx":
        return A.callers_dctx(s.go, t, L.DCTX)
    if tool == "trail":
        return A.callers_trail(s.go, t)
    return A.callers_graphify(s.go, t, g)


def run_repo(tool, repo, corpora, gold, qs, counter):
    d, env, row = prepare(tool, repo, corpora)
    L.append_jsonl("setup.jsonl", row)
    g = A.G(d) if tool == "graphify" else None
    s = Session(tool, repo, d, env, counter)
    for q in qs:
        s.start(q["id"])
        ask(tool, s, q, g)
        present, frac, pts = A.score(q["facts"], s.outs, s.labels)
        L.append_jsonl("tasks.jsonl", {"tool": tool, "repo": repo, "qid": q["id"], "kind": q["kind"], "question": q["q"], "score": pts,
                                       "fraction": round(frac, 3), "facts_n": len(present), "facts_found": sum(present),
                                       "missing": [f for f, p in zip(q["facts"], present) if not p][:12],
                                       "tokens": s.tokens(), "calls": len(s.cmds), "cmds": s.cmds})
    for i, t in enumerate(gold["targets"]):
        s.start(f"callers{i:02d}")
        got = callers(tool, s, t, g)
        r = {"tool": tool, "repo": repo, "target": t["def"], "resolved": got is not None, "tokens": s.tokens()}
        first, every = got or ([], [])
        r.update(A.judge(first, t["callers"]))
        if tool == "dctx":
            r["found_all"] = A.judge(every, t["callers"])["found"]
        L.append_jsonl("callers.jsonl", r)
    shutil.rmtree(d, ignore_errors=True)
    print(f"done {repo} {tool}: setup rc {row['rc']} in {row['wall_s']} s", flush=True)


def versions():
    out = {}
    for k, argv in (("dctx", [L.DCTX, "--version"]), ("trail", [L.R + "/trail/node_modules/.bin/trail", "-v"]),
                    ("graphify", [L.R + "/venv/bin/graphify", "--version"])):
        try:
            out[k] = subprocess.run(argv, capture_output=True, text=True, env={"DO_NOT_TRACK": "1", "PATH": "/usr/bin:/bin"}).stdout.strip()
        except OSError as e:
            out[k] = f"missing ({e})"
    out["dctx_sha256"] = hashlib.sha256(open(L.DCTX, "rb").read()).hexdigest()
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("gold")
    ap.add_argument("questions")
    ap.add_argument("corpora")
    ap.add_argument("--tools", default="dctx,trail,graphify")
    a = ap.parse_args()
    sel = json.load(open(a.selection))
    counter = tokens.Counter(sel["pins"]["tokenizer"])
    L.detect_unshare()
    os.makedirs(L.RES, exist_ok=True)
    meta = {"seed": sel["seed"], "seed_source": sel["seed_source"], "selection": sel["selection"], "tokenizer": counter.info["tokenizer"],
            "tokenizer_info": counter.info, "versions": versions(), "unshare": L.UNSHARE, "cpus": L.CPUS,
            "gold_servers": {s["name"]: json.load(open(os.path.join(a.gold, f"{s['name']}.json")))["server"] for s in sel["selection"]}}
    json.dump(meta, open(os.path.join(L.RES, "env.json"), "w"), indent=1)
    allq = json.load(open(a.questions))["questions"]
    for s in sel["selection"]:
        gold = json.load(open(os.path.join(a.gold, f"{s['name']}.json")))
        qs = [q for q in allq if q["repo"] == s["name"]]
        for tool in a.tools.split(","):
            run_repo(tool, s["name"], a.corpora, gold, qs, counter)
    report.main_dir(L.RES)


if __name__ == "__main__":
    main()
