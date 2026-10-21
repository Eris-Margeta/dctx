#!/usr/bin/env python3
"""Footprint of `trail init` (repo + HOME writes) and of `trail uninstall`; dctx has no init step.

usage: phase_init.py CORPUS
"""
import os
import sys

from benchlib import W, WORK, run, as_bench, fresh_copy, manifest, manifest_diff, append_jsonl

HOME = os.path.join(W, "home")


def snap(d):
    return manifest(d), manifest(HOME)


def record(step, d, before, row):
    after = snap(d)
    rd, hd = manifest_diff(before[0], after[0]), manifest_diff(before[1], after[1])
    append_jsonl("init_writes.jsonl", {"step": step, "rc": row["rc"], "wall_s": row.get("wall_s"),
                                       "repo": rd, "home": hd,
                                       "stdout_tail": row["_stdout"].decode("utf-8", "replace")[-1500:],
                                       "stderr_tail": row["_stderr"].decode("utf-8", "replace")[-800:]})
    return after


def main(corpus):
    d = fresh_copy(corpus, "trail", "-init")
    s = snap(d)
    r = run(["trail", "init", "--dry-run", "."], d, tool="trail", corpus=corpus + "-init", label="init_dry_run")
    s = record("init --dry-run", d, s, r)
    r = run(["trail", "init", "-y", "."], d, tool="trail", corpus=corpus + "-init", label="init_yes", timeout=1800)
    s = record("init -y", d, s, r)
    diff = as_bench(f"cd '{d}' && git -c safe.directory='*' status --short && git -c safe.directory='*' diff --stat")
    append_jsonl("init_writes.jsonl", {"step": "git status after init", "status": diff})
    r = run(["trail", "uninstall", "-y", "."], d, tool="trail", corpus=corpus + "-init", label="uninstall_yes")
    s = record("uninstall -y", d, s, r)
    diff = as_bench(f"cd '{d}' && git -c safe.directory='*' status --short --ignored")
    append_jsonl("init_writes.jsonl", {"step": "git status after uninstall", "status": diff})


if __name__ == "__main__":
    main(sys.argv[1])
