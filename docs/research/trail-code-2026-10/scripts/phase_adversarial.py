#!/usr/bin/env python3
"""Phase 10: robustness and privacy on the synthetic adversarial corpus (git and no-git variants).

usage: phase_adversarial.py
"""
import json
import os
import subprocess
import sys

from benchlib import W, WORK, RES, run, as_bench, append_jsonl, loadavg

ADV = os.path.join(W, "adv")
OUTSIDE = os.path.join(ADV, "outside")
T = 300


def markers():
    out = as_bench(f"cat '{ADV}/markers.tsv'")
    return dict(line.split("\t", 1) for line in out.strip().split("\n"))


def leak_scan(paths, mk):
    found = {}
    for k, v in mk.items():
        if k in ("env_name",):
            continue
        cmd = " ".join(f"'{p}'" for p in paths)
        out = as_bench(f"grep -a -r -l -F -- '{v}' {cmd} 2>/dev/null | head -20 || true")
        if out.strip():
            found[k] = out.strip().split("\n")
    return found


def main():
    if not os.path.exists(os.path.join(ADV, "git")):
        as_bench(f"mkdir -p '{ADV}' && python3 -I '{W}/scripts/gen_adversarial.py' '{ADV}/git' '{OUTSIDE}' > '{ADV}/markers.tsv'"
                 f" && python3 -I '{W}/scripts/gen_adversarial.py' '{ADV}/nogit' '{OUTSIDE}' --no-git > /dev/null")
    mk = markers()
    fn_markers = {k: v for k, v in mk.items() if k.endswith("_fn")}
    fn_markers.update({"main_py": "dctxbench_main", "rs_fn": "dctxbench_rs_fn", "ts_fn": "dctxbench_ts_fn",
                       "huge_last": "dctxbench_huge_f49999"})
    for ext in ("zig", "ex", "hs", "lua", "scala", "dart", "ml", "erl", "jl", "sol", "pl", "r", "vue"):
        fn_markers[f"unsupported_{ext}"] = f"dctxbench_{ext}_fn"
    for variant in ("git", "nogit"):
        corpus = f"adv-{variant}"
        for tool in ("trail", "dctx"):
            d = os.path.join(WORK, tool, corpus)
            as_bench(f"rm -rf '{d}' && mkdir -p '{os.path.dirname(d)}' && cp -a '{ADV}/{variant}' '{d}'")
            cache = os.path.join(W, "dctx-cache", corpus)
            env = {"DCTX_CACHE_DIR": cache} if tool == "dctx" else None
            if tool == "dctx":
                as_bench(f"rm -rf '{cache}' && mkdir -p '{cache}'")
            res = {"tool": tool, "corpus": corpus, "steps": {}}
            rows = []

            def step(label, cmd):
                r = run(cmd, d, tool=tool, corpus=corpus, label=label, timeout=T, extra_env=env, log="adv_runs.jsonl")
                res["steps"][label] = {"rc": r["rc"], "timed_out": r["timed_out"], "wall_s": r.get("wall_s"),
                                       "maxrss_kb": r.get("maxrss_kb"), "stdout_bytes": r["stdout_bytes"],
                                       "stderr_tail": r["_stderr"].decode("utf-8", "replace")[-300:]}
                rows.append(r)
                return r

            if tool == "trail":
                step("build", ["trail", "build", "."])
                step("map", ["trail", "map"])
                step("grep_marker", ["trail", "grep", "DCTXBENCH"])
                step("grep_secret", ["trail", "grep", "sk-test"])
                step("skeleton_crlf", ["trail", "skeleton", "src/crlf.py"])
                step("callers_crlf_target", ["trail", "callers", "dctxbench_lf_target"])
                for k, v in fn_markers.items():
                    step(f"find_{k}", ["trail", "ask", v, "-n", "3"])
            else:
                step("map_find_main", ["dctx", "map", "find", "dctxbench_main"])
                step("find_marker", ["dctx", "map", "find", "DCTXBENCH", "--limit", "40"])
                step("find_secret", ["dctx", "map", "find", "sk-test", "--limit", "40"])
                step("outline_crlf", ["dctx", "map", "map", "src/crlf.py", "--depth", "2"])
                step("read_crlf", ["dctx", "map", "read", "s:src/crlf.py#dctxbench_crlf_fn", "--json"])
                step("pack", ["dctx", "pack", "."])
                step("tree", ["dctx", ".", "-t"])
                for k, v in fn_markers.items():
                    step(f"find_{k}", ["dctx", "map", "find", v, "--limit", "5"])
            # which planted symbols were visible
            vis = {}
            for k, v in fn_markers.items():
                r = res["steps"].get(f"find_{k}")
                out = next((x["_stdout"] for x in rows if x["label"] == f"find_{k}"), b"").decode("utf-8", "replace")
                vis[k] = (f"{v} ·" in out) if tool == "trail" else (f"#{v}\t" in out)
            res["visible_symbols"] = vis
            # leaks in outputs (all steps) and in index/cache/home
            outdir = os.path.join(RES, "outputs", corpus, tool)
            res["leaks_in_outputs"] = {}
            for k, v in mk.items():
                if k == "env_name":
                    continue
                hits = [x["label"] for x in rows if v.encode() in x["_stdout"]]
                if hits:
                    res["leaks_in_outputs"][k] = hits
            stores = [os.path.join(d, "graft")] if tool == "trail" else [cache]
            stores.append(os.path.join(d, ".dctx")) if tool == "dctx" else None
            res["leaks_in_index"] = leak_scan([p for p in stores if os.path.exists(p) or True], mk)
            res["index_bytes"] = int(as_bench(f"du -sb {' '.join(repr(p) for p in stores)} 2>/dev/null | awk '{{s+=$1}} END {{print s+0}}'").strip() or 0)
            res["repo_status"] = as_bench(f"cd '{d}' && (git -c safe.directory='*' status --short 2>/dev/null | head -20 || true)")
            append_jsonl("adversarial.jsonl", res)
            print(json.dumps({k: v for k, v in res.items() if k != "steps"})[:2000], flush=True)
    res = {"home_leaks": leak_scan([os.path.join(W, "home")], mk)}
    append_jsonl("adversarial.jsonl", res)
    print(res)


if __name__ == "__main__":
    append_jsonl("phases.jsonl", {"phase": "adversarial", "event": "start", "load1": loadavg()})
    main()
    append_jsonl("phases.jsonl", {"phase": "adversarial", "event": "end", "load1": loadavg()})
