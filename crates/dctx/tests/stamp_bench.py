#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Twenty fresh-process warm queries against this checkout and 4,000 Rust files.

Run from the repository root with a release binary. State and raw measurements
stay under target/stamp-bench. Exit 1 means the 50 ms p50 requirement is unmet.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools/engine-contract"))
from parity import isolated_env  # noqa: E402

SAMPLES = 20
TARGET_MS = 50


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("label", choices=("before", "after"))
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    base = ROOT / "target/stamp-bench"
    synthetic = base / "synthetic"
    for index in range(4000):
        path = synthetic / f"src/group{index // 100:02}/file{index:04}.rs"
        if not path.exists():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                f"pub fn main_{index}() -> usize {{ helper_{index}() }}\n"
                f"fn helper_{index}() -> usize {{ {index} }}\n"
            )
            os.utime(path, (1577836800, 1577836800))

    results = {}
    for name, repo in [("repo", ROOT), ("synthetic", synthetic)]:
        env = isolated_env(base / f"{name}-{args.label}-state")
        env["DCTX_REUSE_REPORT"] = "1"
        command = [str(binary), "map", "find", "main", "--limit", "5"]

        def run(profile=False):
            start = time.perf_counter_ns()
            result = subprocess.run(
                command + (["--profile"] if profile else []),
                cwd=repo, env=env, capture_output=True, check=True,
            )
            return (time.perf_counter_ns() - start) / 1e6, result.stderr.decode()

        run()  # Populate this binary's persistent cache.
        time.sleep(2.1)  # Let newly created fixture stamps settle before warming.
        run()  # Exclude metadata refresh and page-cache population from samples.
        samples = []
        for _ in range(SAMPLES):
            elapsed, report = run()
            assert "generation reused\n" in report, report
            assert "content_reads 0\n" in report.split("cached_content_reads")[0], report
            assert "cached_content_reads 0\n" in report, report
            samples.append(elapsed)
        _, profile = run(profile=True)
        p50 = statistics.median(samples)
        results[name] = {
            "samples_ms": samples,
            "p50_ms": p50,
            # Nearest-rank percentile: ceil(.95 * 20) = sample 19.
            "p95_ms": sorted(samples)[18],
            "meets_target": p50 <= TARGET_MS,
            "reuse": report,
            "profile": profile,
        }
    output = json.dumps(results, indent=2) + "\n"
    (base / f"{args.label}.json").write_text(output)
    print(output, end="")
    return 0 if all(result["meets_target"] for result in results.values()) else 1


if __name__ == "__main__":
    sys.exit(main())
