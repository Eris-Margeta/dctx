#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure 50 warm map-find subprocess calls, including process startup.

    python tools/bench/map_find_latency.py --binary target/release/dctx-cli.exe \
        --output target/map-find-latency.json

One unmeasured call builds the index. Each measured call verifies the unchanged
fixture and queries that warm index. This is an informational latency sample,
not a pass/fail performance threshold. Works on Windows and Unix; stdlib only.
Use --scale-files 1551 for a deterministic four-level repository-scale fixture.
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


def generate_fixture(place: Path, scale_files: int = 0) -> dict[str, int]:
    """Write settled sources; count directories including the fixture root."""
    if scale_files < 0:
        raise ValueError("scale_files must be nonnegative")
    place.mkdir()
    directories = {Path(".")}
    depth = 0
    # Leave margin beyond the two-second racy window, including timestamp rounding.
    settled_ns = time.time_ns() - 10_000_000_000
    extensions = ("rs", "py", "md", "json")
    for index in range(scale_files or 1):
        if scale_files:
            # Base-four grouping gives 20 files per leaf and four directory levels.
            group = index // 20
            directory = Path(*(f"level_{level}_{group // (4 ** (3 - level)) % 4}"
                               for level in range(3)), f"group_{group:06d}")
            relative = directory / f"source_{index:06d}.{extensions[index % 4]}"
        else:
            relative = Path("lib.rs")
        source = place / relative
        source.parent.mkdir(parents=True, exist_ok=True)
        name = f"scale_item_{index:06d}"
        content = FIXTURE if index == 0 else {
            ".rs": f"pub fn {name}() -> u32 {{ {index} }}\n",
            ".py": f"def {name}():\n    return {index}\n",
            ".md": f"# {name}\n\nDeterministic fixture item {index}.\n",
            ".json": json.dumps({"name": name, "value": index}) + "\n",
        }[source.suffix]
        source.write_text(content, encoding="utf-8", newline="\n")
        os.utime(source, ns=(settled_ns, settled_ns))
        directories.update(relative.parents)
        depth = max(depth, len(relative.parent.parts))
    return {"files": scale_files or 1, "dirs": len(directories), "depth": depth}


def percentile(samples: list[float], fraction: float) -> float:
    """Nearest-rank percentile (p50 = sample 25, p95 = sample 48 of 50)."""
    return sorted(samples)[math.ceil(len(samples) * fraction) - 1]


def phase_timings(stderr: bytes) -> dict[str, int]:
    """Accept one timings object alongside any ordinary stderr diagnostics."""
    reports = []
    for line in stderr.splitlines():
        try:
            report = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if isinstance(report, dict) and "dctx_timings" in report:
            reports.append(report["dctx_timings"])
    if len(reports) != 1 or not isinstance(reports[0], dict) or not reports[0]:
        raise RuntimeError("expected exactly one nonempty dctx_timings object")
    phases = reports[0]
    if any(type(value) is not int or value < 0 for value in phases.values()):
        raise RuntimeError("dctx_timings values must be nonnegative integer microseconds")
    return phases


def measure(binary: Path, root: Path, scale_files: int = 0) -> dict:
    place = root / "fixture"
    shape = generate_fixture(place, scale_files)
    env = dict(os.environ)
    env.update({
        "DCTX_AUTO_CLEAN": "0",
        "DCTX_CACHE_DIR": str(root / "cache"),
        "XDG_CONFIG_HOME": str(root / "config"),
        "APPDATA": str(root / "config"),
    })
    command = [str(binary), "map", "find", SYMBOL, "--place", str(place)]

    timings_enabled = env.get("DCTX_TIMINGS") == "1"

    def query() -> bytes:
        result = subprocess.run(command, env=env, capture_output=True, check=True)
        if SYMBOL.encode() not in result.stdout:
            raise RuntimeError("map find did not return the fixture symbol")
        return result.stderr

    warmup = query()  # Build the index; excluded from both distributions.
    if timings_enabled:
        phase_timings(warmup)
    samples = []
    phases: dict[str, list[int]] = {}
    for _ in range(CALLS):
        start = time.perf_counter_ns()
        stderr = query()
        samples.append((time.perf_counter_ns() - start) / 1_000_000)
        if timings_enabled:
            measured = phase_timings(stderr)
            if phases and measured.keys() != phases.keys():
                raise RuntimeError("dctx_timings phase names changed between calls")
            for phase, micros in measured.items():
                phases.setdefault(phase, []).append(micros)
    report = {
        "schema": 1,
        "platform": platform.system(),
        "architecture": platform.machine(),
        "command": f"dctx map find {SYMBOL}",
        "fixture": FIXTURE,
        "fixture_shape": shape,
        "calls": CALLS,
        "warmup_calls": 1,
        "percentile_method": "nearest-rank",
        "p50_ms": percentile(samples, 0.50),
        "p95_ms": percentile(samples, 0.95),
        "samples_ms": samples,
    }
    if timings_enabled:
        report["phase_medians_us"] = {
            phase: percentile(values, 0.50) for phase, values in sorted(phases.items())
        }
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--scale-files", type=int, default=0,
                        help="source file count (0 keeps the one-file fixture)")
    args = parser.parse_args()
    if args.scale_files < 0:
        parser.error("--scale-files must be nonnegative")
    args.output.unlink(missing_ok=True)  # Never upload a previous run after a failure.
    binary = args.binary.resolve(strict=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="map-find-", dir=args.output.parent.resolve()) as tmp:
        report = measure(binary, Path(tmp), args.scale_files)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"map find: N={CALLS} warm per-call latency (process startup included)")
    print(f"p50={report['p50_ms']:.3f} ms p95={report['p95_ms']:.3f} ms")
    if "phase_medians_us" in report:
        print("phase                         median (us)")
        for phase, micros in report["phase_medians_us"].items():
            print(f"{phase:<29} {micros:>11}")


if __name__ == "__main__":
    main()
