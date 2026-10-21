#!/usr/bin/env python3
"""Phase 2b: incremental freshness after edits (unstaged edit, new untracked file, staged file, deletion).

usage: phase_fresh.py CORPUS [CORPUS...]
"""
import json
import os
import sys

from benchlib import W, WORK, RES, run, as_bench, append_jsonl, loadavg

PROBE_TMPL = {
    ".py": "\n\ndef {probe}():\n    return {target}()\n",
    ".rs": "\n\npub fn {probe}() {{\n    let _ = {target}();\n}}\n",
    ".go": "\n\nfunc {probe}() {{\n\t{target}()\n}}\n",
    ".ts": "\n\nexport function {probe}() {{\n  return {target}();\n}}\n",
    ".js": "\n\nexport function {probe}() {{\n  return {target}();\n}}\n",
}
NEWFILE_TMPL = {
    ".py": "def {probe}():\n    return {target}()\n",
    ".rs": "pub fn {probe}() {{\n    let _ = {target}();\n}}\n",
    ".go": "package {pkg}\n\nfunc {probe}() {{\n\t{target}()\n}}\n",
    ".ts": "export function {probe}() {{\n  return {target}();\n}}\n",
    ".js": "export function {probe}() {{\n  return {target}();\n}}\n",
}


def pick_target(corpus):
    with open(os.path.join(RES, "targets", f"{corpus}.targets.json")) as f:
        ts = json.load(f)["targets"]
    funcs = [t for t in ts if t["def"]["kind"] == "function" and t["category"] in ("hub", "rare", "hub-fill")]
    return (funcs or [t for t in ts if t["category"] == "hub"])[0]


def found(row, needle, tool=None, mode="def"):
    """Structural presence check that does not match an echoed query string."""
    text = row["_stdout"].decode("utf-8", "replace")
    if tool == "dctx":
        return f"#{needle}\t" in text or f"::{needle}\t" in text
    if tool == "trail":
        return (f"{needle} \u00b7" in text) if mode == "def" else (f"\u2190 {needle} " in text)
    return needle in text


def q_find(tool, d, corpus, probe, label, env):
    if tool == "dctx":
        return run(["dctx", "map", "find", probe, "--limit", "10"], d, tool=tool, corpus=corpus, label=label,
                   extra_env=env, log="fresh_runs.jsonl")
    return run(["trail", "ask", probe], d, tool=tool, corpus=corpus, label=label, log="fresh_runs.jsonl")


def q_callers(tool, d, corpus, name, did, label, env):
    if tool == "dctx":
        return run(["dctx", "map", "neighbors", did or name, "--dir", "in", "--edge", "calls"], d, tool=tool,
                   corpus=corpus, label=label, extra_env=env, log="fresh_runs.jsonl")
    return run(["trail", "callers", name], d, tool=tool, corpus=corpus, label=label, log="fresh_runs.jsonl")


def main(corpus):
    t = pick_target(corpus)
    name, f = t["def"]["name"], t["def"]["file"]
    ext = os.path.splitext(f)[1]
    ext = ".ts" if ext in (".tsx", ".mts", ".cts") else (".js" if ext in (".mjs", ".cjs", ".jsx") else ext)
    probe = f"dctxbench_probe_{corpus}" if ext != ".go" else f"DctxbenchProbe{corpus.capitalize()}"
    probe2 = probe + "_newfile" if ext != ".go" else probe + "Newfile"
    did = None
    with open(os.path.join(RES, "query_targets.jsonl")) as fh:
        for line in fh:
            r = json.loads(line)
            if r.get("corpus") == corpus and r.get("dctx_id") and r.get("name") == name:
                did = r["dctx_id"]
    if did is None:
        from phase_query import dctx_id
        did = dctx_id(corpus, name, f, t["def"]["line"])
    for tool in ("dctx", "trail"):
        d = os.path.join(WORK, tool, corpus)
        env = {"DCTX_CACHE_DIR": os.path.join(W, "dctx-cache", corpus)} if tool == "dctx" else None
        res = {"tool": tool, "corpus": corpus, "target": name, "file": f, "probe": probe, "dctx_id": did}
        r0 = q_callers(tool, d, corpus, name, did, "base_callers", env)
        res["base_callers_has_probe"] = found(r0, probe, tool, "caller")
        # 1. unstaged edit appended to an existing tracked file
        body = PROBE_TMPL[ext].format(probe=probe, target=name)
        as_bench(f"cp -p '{d}/{f}' '{d}/{f}.dctxbench.bak'")
        with open(os.path.join(RES, "probe_body.tmp"), "w") as fh:
            fh.write(body)
        as_bench(f"cat '{RES}/probe_body.tmp' >> '{d}/{f}'")
        r1 = q_find(tool, d, corpus, probe, "edit_find", env)
        res.update(edit_find_found=found(r1, probe, tool), edit_find_wall=r1.get("wall_s"), edit_find_rc=r1["rc"])
        r2 = q_callers(tool, d, corpus, name, did, "edit_callers", env)
        res.update(edit_callers_has_probe=found(r2, probe, tool, "caller"), edit_callers_wall=r2.get("wall_s"))
        r1b = q_find(tool, d, corpus, probe, "edit_find_again", env)
        res.update(edit_find_again_wall=r1b.get("wall_s"))
        # 2. new untracked file
        nf = os.path.join(os.path.dirname(f), "dctxbench_newfile" + ext)
        pkg = os.path.basename(os.path.dirname(os.path.join(d, f))) if ext == ".go" else ""
        if ext == ".go":
            pkgline = as_bench(f"grep -m1 '^package ' '{d}/{f}' || true").strip()
            pkg = pkgline.split()[1] if pkgline else pkg
        with open(os.path.join(RES, "probe_body.tmp"), "w") as fh:
            fh.write(NEWFILE_TMPL[ext].format(probe=probe2, target=name, pkg=pkg))
        as_bench(f"cp '{RES}/probe_body.tmp' '{d}/{nf}'")
        r3 = q_find(tool, d, corpus, probe2, "untracked_find", env)
        res.update(untracked_find_found=found(r3, probe2, tool), untracked_find_wall=r3.get("wall_s"))
        # 3. staged
        as_bench(f"cd '{d}' && git -c safe.directory='*' add '{nf}'")
        r4 = q_find(tool, d, corpus, probe2, "staged_find", env)
        res.update(staged_find_found=found(r4, probe2, tool), staged_find_wall=r4.get("wall_s"))
        # 4. revert everything: stale results must disappear
        as_bench(f"cd '{d}' && git -c safe.directory='*' reset -q -- '{nf}' && rm -f '{nf}' && "
                 f"mv '{f}.dctxbench.bak' '{f}'")
        r5 = q_find(tool, d, corpus, probe, "reverted_find", env)
        r6 = q_find(tool, d, corpus, probe2, "reverted_find2", env)
        res.update(reverted_stale_probe=found(r5, probe, tool), reverted_stale_probe2=found(r6, probe2, tool),
                   reverted_find_wall=r5.get("wall_s"))
        r7 = q_callers(tool, d, corpus, name, did, "reverted_callers", env)
        res["reverted_callers_has_probe"] = found(r7, probe, tool, "caller")
        append_jsonl("freshness.jsonl", res)
        print(json.dumps(res), flush=True)
    os.unlink(os.path.join(RES, "probe_body.tmp"))


if __name__ == "__main__":
    for c in sys.argv[1:]:
        append_jsonl("phases.jsonl", {"phase": "fresh", "corpus": c, "event": "start", "load1": loadavg()})
        main(c)
        append_jsonl("phases.jsonl", {"phase": "fresh", "corpus": c, "event": "end", "load1": loadavg()})
