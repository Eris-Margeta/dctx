#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure cold and warm map-find latency including process startup.

Cold queries each use a fresh dctx cache. Warm queries reuse the cache built by
one excluded warmup. Each query runs in a new process.
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
MIN_RUNS = 30


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


def measure(binary: str, root: Path, runs: int, cache_root: Path | None = None) -> dict:
    """Run cold then warm queries against root/fixture; caches live under cache_root (default root)."""
    if runs < MIN_RUNS:
        raise ValueError(f"runs must be at least {MIN_RUNS}")
    env = {key: value for key, value in os.environ.items() if not key.startswith("DCTX_")}
    env.update({
        "DCTX_AUTO_CLEAN": "0",
        "XDG_CONFIG_HOME": str(root / "config"),
        "APPDATA": str(root / "config"),
    })
    fixture = root / "fixture"
    command = [binary, "map", "find", TERM, "--place", "."]

    def query(cache: Path) -> float:
        query_env = {**env, "DCTX_CACHE_DIR": str(cache)}
        start = time.perf_counter_ns()
        result = subprocess.run(command, cwd=fixture, env=query_env, capture_output=True, check=True,
                                timeout=120)
        elapsed = (time.perf_counter_ns() - start) / 1_000_000
        if f"#{TERM}".encode() not in result.stdout:
            raise RuntimeError("map find did not return the fixture symbol")
        return elapsed

    caches = root if cache_root is None else cache_root
    cold = [query(caches / f"cold-{index}") for index in range(runs)]
    warm_cache = caches / "warm"
    query(warm_cache)
    warm = [query(warm_cache) for _ in range(runs)]
    return {
        "runs_per_mode": runs,
        "warmup_runs": 1,
        "percentile_method": "nearest-rank",
        "cold": summarize(cold),
        "warm": summarize(warm),
    }


def summarize(samples: list[float]) -> dict:
    return {
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


def run_count(value: str) -> int:
    number = int(value)
    if number < MIN_RUNS:
        raise argparse.ArgumentTypeError(f"must be at least {MIN_RUNS}")
    return number


def positive_ms(value: str) -> float:
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("must be a finite positive number")
    return number


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", default="dctx", help="CLI executable or path")
    parser.add_argument("--runs", type=run_count, default=MIN_RUNS, help="processes per cache mode")
    parser.add_argument("--files", type=positive_int, default=1500)
    parser.add_argument("--max-p95-ms", type=positive_ms, default=500,
                        help="p95 limit in ms for each cache mode (default: %(default)s)")
    parser.add_argument("--output", type=Path, default=Path("target/win-latency/report.json"))
    parser.add_argument("--dry", action="store_true")
    args = parser.parse_args(argv)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    binary = None if args.dry else shutil.which(args.binary)
    if not args.dry and binary is None:
        parser.error("dctx executable not found; supply --binary")
    report = {
        "schema": 2,
        "platform": platform.system(),
        "architecture": platform.machine(),
        "files": args.files,
        "command": f"dctx map find {TERM}",
        "dry": args.dry,
        "max_p95_ms": args.max_p95_ms,
    }
    with tempfile.TemporaryDirectory(prefix="fixture-", dir=args.output.parent.resolve()) as tmp:
        root = Path(tmp)
        generate_fixture(root / "fixture", args.files)
        if not args.dry:
            report.update(measure(str(Path(binary).resolve()), root, args.runs))
    failed = False
    if args.dry:
        summary = f"DRY RUN dctx map find ({report['platform']}, {args.files} files)"
    else:
        modes = ("cold", "warm")
        failed = any(report[mode]["p95_ms"] > args.max_p95_ms for mode in modes)
        report["verdict"] = "FAIL" if failed else "PASS"
        stats = " | ".join(
            f"{mode} {report[mode]['p50_ms']:.3f}/{report[mode]['p95_ms']:.3f}/"
            f"{report[mode]['max_ms']:.3f}" for mode in modes
        )
        summary = (f"{report['verdict']} dctx map find ({report['platform']}, {args.files} files, "
                   f"{args.runs} runs per mode) p50/p95/max ms | {stats} | "
                   f"p95 limit {args.max_p95_ms:g} ms")
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(summary)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(summary + "\n")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
