#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
umask 022

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
python3 -B - "$repo" "$@" <<'PY'
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

repo = Path(sys.argv[1])
sys.path.insert(0, str(repo / "tools" / "bench"))
from map_find_latency import SYMBOL, generate_fixture, phase_timings, percentile

parser = argparse.ArgumentParser(
    prog="map-latency.sh", description="Measure map find on 1,500 generated files.")
parser.add_argument("binary", type=Path, help="dctx CLI binary (baseline if comparing)")
parser.add_argument("candidate", type=Path, nargs="?", help="compare with this CLI binary")
args = parser.parse_args(sys.argv[2:])

runs = 20
binaries = [("baseline" if args.candidate else "binary", args.binary.resolve(strict=True))]
if args.candidate:
    binaries.append(("candidate", args.candidate.resolve(strict=True)))
samples = {label: [] for label, _ in binaries}

with tempfile.TemporaryDirectory(prefix=".map-latency-", dir=repo) as temp:
    root = Path(temp)
    fixture = root / "fixture"
    shape = generate_fixture(fixture, 1500)
    env = {key: value for key, value in os.environ.items() if not key.startswith("DCTX_")}
    env.update({
        "XDG_CONFIG_HOME": str(root / "config"),
        "APPDATA": str(root / "config"),
        "NO_COLOR": "1",
        "DCTX_AUTO_CLEAN": "0",
        "DCTX_REUSE_REPORT": "1",
    })
    expected = None

    def query(label, binary, state, profile=False):
        run_env = dict(env, DCTX_CACHE_DIR=str(root / f"cache-{label}"))
        command = [str(binary), "map", "find", SYMBOL, "--place", str(fixture)]
        if profile:
            command.append("--profile")
            run_env["DCTX_TIMINGS"] = "1"
        start = time.perf_counter_ns()
        output = subprocess.run(command, cwd=fixture, env=run_env, capture_output=True)
        elapsed_ms = (time.perf_counter_ns() - start) / 1_000_000
        if output.returncode or f"generation {state}".encode() not in output.stderr.splitlines():
            raise RuntimeError(f"{label}: expected generation {state}: {output.stderr!r}")
        if SYMBOL.encode() not in output.stdout:
            raise RuntimeError(f"{label}: fixture symbol missing from stdout")
        if profile:
            timing = phase_timings(output.stderr)
            stages = {}
            for line in output.stderr.decode().splitlines():
                if line.startswith("stage "):
                    stage, ns = line.removeprefix("stage ").rsplit(" ", 1)
                    stages[stage] = int(ns) / 1_000
            if not {"walk", "parse/lex", "reuse load", "reuse decode"} <= stages.keys():
                raise RuntimeError(f"{label}: missing profile stages: {stages}")
            print(f"{label} {state} stages (us):", stages)
            print(f"{label} {state} CLI phases (us):", timing)
        return elapsed_ms, output.stdout

    print(f"Fixture: {shape['files']} files, {shape['dirs']} directories, depth {shape['depth']}")
    for label, binary in binaries:
        for state in ("built", "reused"):
            _, stdout = query(label, binary, state, profile=True)
            if expected is None:
                expected = stdout
            elif stdout != expected:
                raise RuntimeError(f"{label}: {state} stdout differs from the first cold query")

    for run in range(runs):
        order = binaries if run % 2 == 0 else reversed(binaries)
        for label, binary in order:
            elapsed_ms, stdout = query(label, binary, "reused")
            if stdout != expected:
                raise RuntimeError(f"{label}: timed run {run + 1} stdout differs")
            samples[label].append(elapsed_ms)

    print(f"Warm index, fresh CLI process, {runs} runs per binary")
    for label, elapsed in samples.items():
        print(f"{label}: p50={percentile(elapsed, 0.50):.3f} ms "
              f"p95={percentile(elapsed, 0.95):.3f} ms")
    if args.candidate:
        improvement = 1 - percentile(samples["candidate"], 0.50) / percentile(samples["baseline"], 0.50)
        print(f"p50 improvement: {improvement:.1%}")
    print(f"Byte-identical stdout across {len(binaries) * (runs + 2)} calls")
PY
