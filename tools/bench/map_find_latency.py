#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure 50 warm map-find subprocess calls, including process startup.

    python tools/bench/map_find_latency.py --binary target/release/dctx-cli.exe \
        --output target/map-find-latency.json

One unmeasured call builds the index. Each measured call verifies the unchanged
fixture and queries that warm index. This is an informational latency sample,
not a pass/fail performance threshold. Works on Windows and Unix; stdlib only.
"""
from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time

CALLS = 50
SYMBOL = "latency_probe"
FIXTURE = "pub fn latency_probe() -> u32 { helper() }\nfn helper() -> u32 { 42 }\n"


def percentile(samples: list[float], fraction: float) -> float:
    """Nearest-rank percentile (p50 = sample 25, p95 = sample 48 of 50)."""
    return sorted(samples)[math.ceil(len(samples) * fraction) - 1]


def measure(binary: Path, root: Path) -> dict:
    place = root / "fixture"
    place.mkdir()
    (place / "lib.rs").write_text(FIXTURE, encoding="utf-8", newline="\n")
    env = dict(os.environ)
    env.update({
        "DCTX_AUTO_CLEAN": "0",
        "DCTX_CACHE_DIR": str(root / "cache"),
        "XDG_CONFIG_HOME": str(root / "config"),
        "APPDATA": str(root / "config"),
    })
    command = [str(binary), "map", "find", SYMBOL, "--place", str(place)]

    def query() -> None:
        result = subprocess.run(command, env=env, capture_output=True, check=True)
        if SYMBOL.encode() not in result.stdout:
            raise RuntimeError("map find did not return the fixture symbol")

    query()  # Build the index; excluded from the latency distribution.
    samples = []
    for _ in range(CALLS):
        start = time.perf_counter_ns()
        query()
        samples.append((time.perf_counter_ns() - start) / 1_000_000)
    return {
        "schema": 1,
        "platform": platform.system(),
        "architecture": platform.machine(),
        "command": f"dctx map find {SYMBOL}",
        "fixture": FIXTURE,
        "calls": CALLS,
        "warmup_calls": 1,
        "percentile_method": "nearest-rank",
        "p50_ms": percentile(samples, 0.50),
        "p95_ms": percentile(samples, 0.95),
        "samples_ms": samples,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.unlink(missing_ok=True)  # Never upload a previous run after a failure.
    binary = args.binary.resolve(strict=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="map-find-", dir=args.output.parent.resolve()) as tmp:
        report = measure(binary, Path(tmp))
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"map find: N={CALLS} warm per-call latency (process startup included)")
    print(f"p50={report['p50_ms']:.3f} ms p95={report['p95_ms']:.3f} ms")


if __name__ == "__main__":
    main()
