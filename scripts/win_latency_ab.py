#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare map-find latency of a candidate dctx against a baseline on the same machine.

Both binaries run interleaved rounds (order alternates each round) over one shared
fixture, using the cold/warm protocol of win_latency.measure. The gate uses the
median over rounds of each binary's cold p95:
  FAIL if candidate > baseline * (1 + --max-regression), or candidate > --ceiling-ms.
  The absolute --target-ms is reported and warned about, never failed on.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import tempfile

try:
    from scripts import win_latency as latency
except ImportError:  # run as a plain script: python scripts/win_latency_ab.py
    import win_latency as latency  # type: ignore[no-redef]

MIN_ROUNDS = 5


def run_rounds(baseline: str, candidate: str, root: Path, rounds: int, runs: int) -> dict:
    if rounds < MIN_ROUNDS:
        raise ValueError(f"rounds must be at least {MIN_ROUNDS}")
    binaries = {"baseline": baseline, "candidate": candidate}
    results: dict[str, list[dict]] = {"baseline": [], "candidate": []}
    order = []
    for index in range(rounds):
        names = ("baseline", "candidate") if index % 2 == 0 else ("candidate", "baseline")
        for name in names:
            order.append(name)
            report = latency.measure(binaries[name], root, runs, cache_root=root / f"round-{index}-{name}")
            results[name].append({
                "round": index,
                "cold_p95_ms": report["cold"]["p95_ms"],
                "cold_p50_ms": report["cold"]["p50_ms"],
                "warm_p95_ms": report["warm"]["p95_ms"],
            })
    return {"order": order, "rounds": results}


def verdict(rounds: dict, max_regression: float, ceiling_ms: float, target_ms: float) -> dict:
    medians = {
        name: {
            "cold_p95_ms": statistics.median(r["cold_p95_ms"] for r in rows),
            "warm_p95_ms": statistics.median(r["warm_p95_ms"] for r in rows),
        }
        for name, rows in rounds.items()
    }
    base = medians["baseline"]["cold_p95_ms"]
    cand = medians["candidate"]["cold_p95_ms"]
    ratio = cand / base if base > 0 else float("inf")
    reasons = []
    if ratio > 1 + max_regression:
        reasons.append(f"candidate cold p95 is {(ratio - 1) * 100:.1f}% slower than baseline "
                       f"(limit {max_regression * 100:g}%)")
    if cand > ceiling_ms:
        reasons.append(f"candidate cold p95 {cand:.1f} ms exceeds the {ceiling_ms:g} ms ceiling")
    return {
        "medians": medians,
        "ratio": ratio,
        "max_regression": max_regression,
        "ceiling_ms": ceiling_ms,
        "target_ms": target_ms,
        "target_missed": cand > target_ms,
        "reasons": reasons,
        "verdict": "FAIL" if reasons else "PASS",
    }


def render(report: dict) -> tuple[str, str]:
    """One-line verdict and a markdown table for the job summary."""
    v = report["gate"]
    m = v["medians"]
    line = (f"{v['verdict']} dctx map find A/B ({report['platform']}, {report['files']} files, "
            f"{report['rounds_count']} rounds x {report['runs_per_mode']} runs) median cold p95 ms: "
            f"candidate {m['candidate']['cold_p95_ms']:.1f} vs baseline {report['baseline_label']} "
            f"{m['baseline']['cold_p95_ms']:.1f} ({(v['ratio'] - 1) * 100:+.1f}%, limit "
            f"+{v['max_regression'] * 100:g}%, ceiling {v['ceiling_ms']:g} ms, target {v['target_ms']:g} ms)")
    rows = ["| round | baseline cold p95 | candidate cold p95 | baseline warm p95 | candidate warm p95 |",
            "|---|---|---|---|---|"]
    for b, c in zip(report["rounds"]["baseline"], report["rounds"]["candidate"]):
        rows.append(f"| {b['round']} | {b['cold_p95_ms']:.1f} | {c['cold_p95_ms']:.1f} | "
                    f"{b['warm_p95_ms']:.1f} | {c['warm_p95_ms']:.1f} |")
    rows.append(f"| **median** | **{m['baseline']['cold_p95_ms']:.1f}** | **{m['candidate']['cold_p95_ms']:.1f}** | "
                f"{m['baseline']['warm_p95_ms']:.1f} | {m['candidate']['warm_p95_ms']:.1f} |")
    notes = [f"- {reason}" for reason in v["reasons"]]
    if v["target_missed"]:
        notes.append(f"- warning: candidate median cold p95 misses the {v['target_ms']:g} ms target "
                     "(informational, not a failure)")
    return line, "\n".join([f"### {line}", "", *rows, "", *notes, ""])


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--baseline", required=True, help="baseline dctx executable")
    parser.add_argument("--baseline-label", default="baseline")
    parser.add_argument("--candidate", required=True, help="candidate dctx executable")
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--runs", type=latency.run_count, default=latency.MIN_RUNS)
    parser.add_argument("--files", type=latency.positive_int, default=1500)
    parser.add_argument("--max-regression", type=float, default=0.20)
    parser.add_argument("--ceiling-ms", type=latency.positive_ms, default=1000)
    parser.add_argument("--target-ms", type=latency.positive_ms, default=500)
    parser.add_argument("--output", type=Path, default=Path("target/win-latency/ab-report.json"))
    args = parser.parse_args(argv)
    if args.rounds < MIN_ROUNDS:
        parser.error(f"--rounds must be at least {MIN_ROUNDS}")
    if not 0 < args.max_regression < 10:
        parser.error("--max-regression must be a fraction such as 0.20")
    paths = {}
    for name in ("baseline", "candidate"):
        found = shutil.which(getattr(args, name))
        if found is None:
            parser.error(f"{name} executable not found")
        paths[name] = str(Path(found).resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.unlink(missing_ok=True)
    report = {
        "schema": 1,
        "platform": platform.system(),
        "files": args.files,
        "runs_per_mode": args.runs,
        "rounds_count": args.rounds,
        "baseline_label": args.baseline_label,
        "statistic": "median over rounds of nearest-rank cold p95",
    }
    with tempfile.TemporaryDirectory(prefix="ab-", dir=args.output.parent.resolve()) as tmp:
        root = Path(tmp)
        latency.generate_fixture(root / "fixture", args.files)
        report.update(run_rounds(paths["baseline"], paths["candidate"], root, args.rounds, args.runs))
    report["gate"] = verdict(report["rounds"], args.max_regression, args.ceiling_ms, args.target_ms)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    line, markdown = render(report)
    print(line)
    if report["gate"]["target_missed"] and os.environ.get("GITHUB_ACTIONS"):
        print(f"::warning title=Windows map find latency target::median cold p95 "
              f"{report['gate']['medians']['candidate']['cold_p95_ms']:.1f} ms misses the "
              f"{args.target_ms:g} ms target (informational)")
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as output:
            output.write(markdown)
    return 1 if report["gate"]["verdict"] == "FAIL" else 0


if __name__ == "__main__":
    raise SystemExit(main())
