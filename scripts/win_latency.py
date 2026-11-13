#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure warm map-find latency including process startup, on Windows or Linux.

One excluded warmup builds the index; each measured query is a new process.
--dry generates the same fixture and reports its shape without invoking dctx.
All temporary files and caches stay beneath the output directory.
"""
from __future__ import annotations

import argparse
import json
import math
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import time

TERM = "win_latency_probe"


def percentile(samples: list[float], fraction: float) -> float:
    """Nearest rank: sorted sample ceil(N * fraction), using one-based ranks."""
    if not samples or not 0 < fraction <= 1:
        raise ValueError("percentile requires samples and 0 < fraction <= 1")
    return sorted(samples)[math.ceil(len(samples) * fraction) - 1]


def generate_fixture(root: Path, files: int = 1500) -> None:
    """Create exactly files deterministic Rust sources, with one query match."""
    if files < 1:
        raise ValueError("files must be positive")
    root.mkdir(parents=True)
    # Avoid the freshness racy window without sleeping between queries.
    settled_ns = time.time_ns() - 10_000_000_000
    for index in range(files):
        source = root / f"group_{index // 25:04d}" / f"item_{index:04d}.rs"
        source.parent.mkdir(exist_ok=True)
        name = TERM if index == 0 else f"synthetic_item_{index:04d}"
        source.write_text(f"pub fn {name}() -> usize {{ {index} }}\n", encoding="utf-8")
        os.utime(source, ns=(settled_ns, settled_ns))


def measure(binary: str, root: Path, runs: int) -> dict:
    if runs < 1:
        raise ValueError("runs must be positive")
    env = dict(os.environ)
    env.update({
        "DCTX_AUTO_CLEAN": "0",
        "DCTX_CACHE_DIR": str(root / "cache"),
        "XDG_CONFIG_HOME": str(root / "config"),
        "APPDATA": str(root / "config"),
    })
    command = [binary, "map", "find", TERM, "--place", str(root / "fixture")]

    def query() -> None:
        result = subprocess.run(command, env=env, capture_output=True, check=True,
                                timeout=120)
        if TERM.encode() not in result.stdout:
            raise RuntimeError("map find did not return the fixture symbol")

    query()
    samples = []
    for _ in range(runs):
        start = time.perf_counter_ns()
        query()
        samples.append((time.perf_counter_ns() - start) / 1_000_000)
    return {
        "runs": runs,
        "warmup_runs": 1,
        "percentile_method": "nearest-rank",
        "samples_ms": samples,
        "p50_ms": percentile(samples, 0.50),
        "p95_ms": percentile(samples, 0.95),
        "max_ms": max(samples),
    }


def positive_int(value: str) -> int:
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="dctx", help="CLI executable or path")
    parser.add_argument("--runs", type=positive_int, default=30)
    parser.add_argument("--files", type=positive_int, default=1500)
    parser.add_argument("--output", type=Path, default=Path("target/win-latency/report.json"))
    parser.add_argument("--dry", action="store_true")
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    binary = None if args.dry else shutil.which(args.binary)
    if not args.dry and binary is None:
        parser.error("dctx executable not found; supply --binary")
    report = {
        "schema": 1,
        "platform": platform.system(),
        "architecture": platform.machine(),
        "files": args.files,
        "command": f"dctx map find {TERM}",
        "dry": args.dry,
    }
    with tempfile.TemporaryDirectory(prefix="fixture-", dir=args.output.parent.resolve()) as tmp:
        root = Path(tmp)
        generate_fixture(root / "fixture", args.files)
        if not args.dry:
            report.update(measure(str(Path(binary).resolve()), root, args.runs))
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.dry:
        summary = f"Dry run: generated {args.files} files ({report['platform']}); no measurements.\n"
    else:
        summary = (
            f"### dctx map find latency ({report['platform']})\n\n"
            f"{args.files} files; {args.runs} separate processes; one excluded warmup. "
            "Wall time includes process startup; nearest-rank percentiles.\n\n"
            "| p50 (ms) | p95 (ms) | max (ms) |\n"
            "| ---: | ---: | ---: |\n"
            f"| {report['p50_ms']:.3f} | {report['p95_ms']:.3f} | {report['max_ms']:.3f} |\n"
        )
    print(summary)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(summary)


if __name__ == "__main__":
    main()
