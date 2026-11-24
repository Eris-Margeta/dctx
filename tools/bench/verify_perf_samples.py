#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Verify the raw samples and medians in two instruction-mode performance reports."""
import json
import math
from pathlib import Path
from statistics import median
import sys

from perf_ci import CASES, INSTRUCTION_SAMPLES, SAMPLES


def valid_value(metric, value):
    if type(value) is int:
        return value > 0
    return (metric == "wall_seconds" and type(value) is float
            and math.isfinite(value) and value > 0)


def verify(report):
    cases = report.get("cases") if isinstance(report, dict) else None
    if not isinstance(cases, dict) or set(cases) != set(CASES):
        raise ValueError("expected the runner's complete set of benchmark cases")
    for case in CASES:
        row = cases[case]
        if not isinstance(row, dict):
            raise ValueError(f"{case} must be an object")
        for metric, count in (("wall_seconds", SAMPLES), ("instructions", INSTRUCTION_SAMPLES)):
            samples = row.get(metric)
            if not isinstance(samples, list) or len(samples) != count:
                raise ValueError(f"{case}.{metric} must contain {count} samples")
            for index, sample in enumerate(samples):
                if not valid_value(metric, sample):
                    raise ValueError(f"invalid sample at {case}.{metric}[{index}]")
            recorded = row.get(f"{metric}_median")
            if not valid_value(metric, recorded) or median(samples) != recorded:
                raise ValueError(f"{case}.{metric}_median does not match the sample median")


def main(paths):
    if len(paths) != 2:
        print("usage: verify_perf_samples.py BASELINE CANDIDATE", file=sys.stderr)
        return 2
    for role, path in zip(("baseline", "candidate"), paths):
        try:
            verify(json.loads(Path(path).read_text(encoding="utf-8")))
        except (OSError, ValueError, RecursionError) as error:
            print(f"{role} {json.dumps(str(path))}: {json.dumps(str(error))}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
