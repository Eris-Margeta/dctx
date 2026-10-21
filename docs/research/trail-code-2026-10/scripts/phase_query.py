#!/usr/bin/env python3
"""Phases 3, 4, 7: query latency (warm x5, cold x3), output size, determinism and stability.

usage: phase_query.py CORPUS [CORPUS...]
Requires phase_index (work copies + caches) and select_targets (targets file).
"""
import json
import os
import sys

from benchlib import W, WORK, RES, run, as_bench, append_jsonl, loadavg

WARM = 5
COLD = 3


def load_targets(corpus):
    with open(os.path.join(RES, "targets", f"{corpus}.targets.json")) as f:
        return json.load(f)["targets"]


def dctx_id(corpus, name, file, line, kind=None):
    d = os.path.join(WORK, "dctx", corpus)
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    r = run(["dctx", "map", "find", name, "--json", "--limit", "40", "--budget", "2000"], d, tool="dctx",
            corpus=corpus, label=f"resolve_{name}", log="resolve.jsonl", keep=False, extra_env=env)
    best = None
    try:
        recs = json.loads(r["_stdout"]).get("records", [])
    except ValueError:
        recs = []
    for rec in recs:
        rid = rec.get("id", "")
        tail = rid.split("#", 1)[-1].split("::")[-1].split("~")[0]
        if rec.get("path") == file and tail == name:
            dist = abs((rec.get("lines") or [0])[0] - line)
            if best is None or dist < best[0]:
                best = (dist, rid)
    return best[1] if best and best[0] <= 5 else None


def queries(corpus, t, did):
    name, file = t["def"]["name"], t["def"]["file"]
    q = {
        "find": (["dctx", "map", "find", name, "--limit", "10"], ["trail", "ask", name]),
        "find_json": (["dctx", "map", "find", name, "--limit", "10", "--json"], ["trail", "ask", name, "--json"]),
        "callers1": (["dctx", "map", "neighbors", did or name, "--dir", "in", "--edge", "calls"],
                     ["trail", "callers", name]),
        "callers2": (["dctx", "map", "neighbors", did or name, "--dir", "in", "--edge", "calls", "--depth", "2"],
                     ["trail", "callers", name, "-d", "2"]),
        "callees": (["dctx", "map", "neighbors", did or name, "--dir", "out", "--edge", "calls"],
                    ["trail", "callers", name, "--direction", "out"]),
        "outline": (["dctx", "map", "map", file, "--depth", "2", "--limit", "40"], ["trail", "skeleton", file]),
        "overview": (None, ["trail", "map"]),
        "grep": (None, ["trail", "grep", name + r"\("]),
        "read": (["dctx", "map", "read", did or name, "--json"], ["trail", "ask", name, "--source", "--full", "-n", "1"]),
        "read_text": (["dctx", "map", "read", did or name], None),
        "explain": (["dctx", "map", "explain", did or name, "--json"], None),
        "callers1_norefresh": (None, ["trail", "callers", name, "--no-refresh"]),
    }
    return q


def main(corpus):
    targets = load_targets(corpus)
    t = next(x for x in targets if x["category"] == "hub")
    did = dctx_id(corpus, t["def"]["name"], t["def"]["file"], t["def"]["line"])
    append_jsonl("query_targets.jsonl", {"corpus": corpus, "name": t["def"]["name"], "file": t["def"]["file"],
                                         "line": t["def"]["line"], "dctx_id": did})
    dd, td = os.path.join(WORK, "dctx", corpus), os.path.join(WORK, "trail", corpus)
    env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)}
    qs = queries(corpus, t, did)
    # warm
    for label, (dc, tc) in qs.items():
        for r in range(WARM):
            if dc:
                run(dc, dd, tool="dctx", corpus=corpus, label="warm_" + label, rep=r, extra_env=env)
            if tc:
                run(tc, td, tool="trail", corpus=corpus, label="warm_" + label, rep=r)
    # dctx overview: legacy tree snapshot writes .dctx/<place>.llmcontext.txt into the repo
    for r in range(WARM):
        row = run(["dctx", ".", "-t"], dd, tool="dctx", corpus=corpus, label="warm_overview", rep=r, extra_env=env)
        size = int(as_bench(f"cat '{dd}'/.dctx/*.llmcontext.txt | wc -c").strip())
        append_jsonl("overview_files.jsonl", {"corpus": corpus, "rep": r, "file_bytes": size,
                                             "stdout_bytes": row["stdout_bytes"]})
        content = as_bench(f"cat '{dd}'/.dctx/*.llmcontext.txt | head -c 262144")
        with open(os.path.join(RES, "outputs", corpus, "dctx", f"warm_overview_file.r{r}.out"), "w") as f:
            f.write(content)
    as_bench(f"rm -rf '{dd}/.dctx'")
    # cold dctx: fresh empty cache per run (includes indexing); trail has no cold query path (needs build)
    for label in ("find", "callers1", "outline", "read"):
        dc = qs[label][0]
        for r in range(COLD):
            cache = os.path.join(W, "dctx-cache", f"{corpus}.coldq")
            as_bench(f"rm -rf '{cache}' && mkdir -p '{cache}'")
            run(dc, dd, tool="dctx", corpus=corpus, label="cold_" + label, rep=r, extra_env={"DCTX_CACHE_DIR": cache})
    as_bench(f"rm -rf '{W}/dctx-cache/{corpus}.coldq'")
    # stability after an unrelated edit (append a comment line to an unrelated file in another directory)
    other = pick_unrelated(corpus, t["def"]["file"])
    for tool, d in (("dctx", dd), ("trail", td)):
        as_bench(f"cp -p '{d}/{other}' '{d}/{other}.bak' && printf '\\n{comment_for(other)} dctxbench unrelated edit\\n' >> '{d}/{other}'")
        for label, (dc, tc) in qs.items():
            c = dc if tool == "dctx" else tc
            if c:
                run(c, d, tool=tool, corpus=corpus, label="unrelated_" + label, rep=0,
                    extra_env=env if tool == "dctx" else None)
        as_bench(f"mv '{d}/{other}.bak' '{d}/{other}'")
    append_jsonl("query_targets.jsonl", {"corpus": corpus, "unrelated_file": other})


def comment_for(path):
    return "#" if path.endswith((".py", ".toml", ".yaml", ".yml", ".sh")) else "//"


def pick_unrelated(corpus, target_file):
    d = os.path.join(WORK, "dctx", corpus)
    tdir = os.path.dirname(target_file)
    ext = os.path.splitext(target_file)[1]
    out = as_bench(f"cd '{d}' && git -c safe.directory='*' ls-files '*{ext}' | head -400")
    for f in sorted(out.split("\n")):
        if f and os.path.dirname(f) != tdir and "test" not in f.lower() and "/" in f:
            return f
    return [f for f in out.split("\n") if f and f != target_file][0]


if __name__ == "__main__":
    for c in sys.argv[1:]:
        append_jsonl("phases.jsonl", {"phase": "query", "corpus": c, "event": "start", "load1": loadavg()})
        main(c)
        append_jsonl("phases.jsonl", {"phase": "query", "corpus": c, "event": "end", "load1": loadavg()})
        print("done", c, flush=True)
