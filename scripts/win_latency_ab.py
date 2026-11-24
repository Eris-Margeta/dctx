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
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
from fractions import Fraction
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import tempfile

try:
    from scripts import win_latency as latency
except ImportError:  # run as a plain script: python scripts/win_latency_ab.py
    import win_latency as latency  # type: ignore[no-redef]

MIN_ROUNDS = 5
NS_PER_MS = 1_000_000
ARMS = ("baseline", "candidate")


def positive_decimal(value) -> Decimal:
    try:
        number = Decimal(str(value))
    except InvalidOperation:
        raise ValueError("timing or limit must be numeric") from None
    if not number.is_finite() or not 0 < float(number) < math.inf:
        raise ValueError("timings and limits must be finite and positive")
    return number


def sample_vector(samples_ms: list, runs: int) -> list[int]:
    if len(samples_ms) != runs:
        raise ValueError("sample count differs from configured runs")
    result = []
    for ms in samples_ms:
        # measure() divides perf_counter_ns deltas by 10**6. At its 120 s
        # subprocess timeout the shortest decimal ms recovers every ns digit.
        ns = Fraction(positive_decimal(ms)) * NS_PER_MS
        if ns.denominator != 1 or math.ulp(float(ms)) >= 1 / NS_PER_MS:
            raise ValueError("sample cannot be recovered at nanosecond precision")
        result.append(int(ns))
    return result


def row_stats(cold: list[int], warm: list[int]) -> dict[str, Fraction]:
    return {
        "cold_p95_ms": Fraction(latency.percentile(cold, 0.95), NS_PER_MS),
        "cold_p50_ms": Fraction(latency.percentile(cold, 0.50), NS_PER_MS),
        "warm_p95_ms": Fraction(latency.percentile(warm, 0.95), NS_PER_MS),
    }


def run_rounds(baseline: str, candidate: str, root: Path, rounds: int, runs: int) -> dict:
    if rounds < MIN_ROUNDS:
        raise ValueError(f"rounds must be at least {MIN_ROUNDS}")
    if runs < latency.MIN_RUNS:
        raise ValueError(f"runs must be at least {latency.MIN_RUNS}")
    binaries = {"baseline": baseline, "candidate": candidate}
    results: dict[str, list[dict]] = {"baseline": [], "candidate": []}
    order = []
    for index in range(rounds):
        names = ("baseline", "candidate") if index % 2 == 0 else ("candidate", "baseline")
        for name in names:
            order.append(name)
            report = latency.measure(binaries[name], root, runs, cache_root=root / f"round-{index}-{name}")
            cold = sample_vector(report["cold"]["samples_ms"], runs)
            warm = sample_vector(report["warm"]["samples_ms"], runs)
            results[name].append({
                "round": index,
                "cold_samples_ns": cold,
                "warm_samples_ns": warm,
                **{key: float(ms) for key, ms in row_stats(cold, warm).items()},
            })
    return {"order": order, "rounds": results}


def paired_medians(rounds: dict, count: int | None = None, runs: int | None = None,
                   require_samples: bool = False) -> dict:
    if not isinstance(rounds, dict) or set(rounds) != set(ARMS):
        raise ValueError("expected baseline and candidate rounds")
    for rows in rounds.values():
        if not isinstance(rows, (list, tuple)) or not all(isinstance(row, dict) for row in rows):
            raise ValueError("each arm must contain a sequence of round records")
    expected = len(rounds["baseline"]) if count is None else count
    if type(expected) is not int or expected < MIN_ROUNDS:
        raise ValueError("invalid round count")
    if runs is not None and (type(runs) is not int or runs < latency.MIN_RUNS):
        raise ValueError("invalid runs per mode")

    # Legacy verdict callers passed just two ordered lists of ms summaries.
    # Reports and any explicitly indexed input must identify every pair.
    indexed = count is not None or any("round" in row for rows in rounds.values() for row in rows)
    vectors = require_samples or any("cold_samples_ns" in row or "warm_samples_ns" in row
                                     for rows in rounds.values() for row in rows)
    medians = {}
    for name in ARMS:
        if len(rounds[name]) != expected:
            raise ValueError("incomplete paired rounds")
        p95 = {"cold_p95_ms": [], "warm_p95_ms": []}
        for index, row in enumerate(rounds[name]):
            if indexed or vectors:
                if type(row.get("round")) is not int or row["round"] != index:
                    raise ValueError("round IDs must be contiguous zero-based pairs")
            if vectors:
                cold, warm = row.get("cold_samples_ns"), row.get("warm_samples_ns")
                for samples in (cold, warm):
                    if not isinstance(samples, list) or len(samples) < latency.MIN_RUNS:
                        raise ValueError("missing or incomplete sample vector")
                    if runs is None:
                        runs = len(samples)
                    if len(samples) != runs or any(type(ns) is not int or ns <= 0 for ns in samples):
                        raise ValueError("invalid sample vector or count")
                stats = row_stats(cold, warm)
                if any(row.get(key) != float(value) for key, value in stats.items()):
                    raise ValueError("millisecond summary differs from retained samples")
            else:
                if "cold_p50_ms" in row:
                    positive_decimal(row["cold_p50_ms"])
                stats = {key: Fraction(positive_decimal(row.get(key))) for key in p95}
            for key in p95:
                p95[key].append(stats[key])
        medians[name] = {key: statistics.median(values) for key, values in p95.items()}
    return medians


def verdict(rounds: dict, max_regression: float, ceiling_ms: float, target_ms: float) -> dict:
    regression, ceiling, target = map(positive_decimal, (max_regression, ceiling_ms, target_ms))
    if regression >= 10:
        raise ValueError("--max-regression must be a fraction such as 0.20")
    medians = paired_medians(rounds)
    base = medians["baseline"]["cold_p95_ms"]
    cand = medians["candidate"]["cold_p95_ms"]
    relative_limit = base * (1 + Fraction(regression))
    reasons = []
    if cand > relative_limit:
        reasons.append("candidate cold p95 is slower than baseline "
                       f"({cand * NS_PER_MS} ns exceeds relative limit {relative_limit * NS_PER_MS} ns)")
    if cand > ceiling:
        reasons.append(f"candidate cold p95 {cand * NS_PER_MS} ns exceeds the {ceiling} ms ceiling")
    return {
        "medians": {arm: {key: float(value) for key, value in stats.items()}
                    for arm, stats in medians.items()},
        "ratio": float(cand / base),
        "max_regression": float(regression),
        "ceiling_ms": float(ceiling),
        "target_ms": float(target),
        "limits_exact": {"max_regression": str(regression), "ceiling_ms": str(ceiling),
                         "target_ms": str(target)},
        "target_missed": cand > target,
        "reasons": reasons,
        "verdict": "FAIL" if reasons else "PASS",
    }


def markdown_literal(quoted: str) -> str:
    fence = "`" * (1 + max((len(run) for run in re.findall(r"`+", quoted)), default=0))
    return f"{fence} {quoted} {fence}"


def render(report: dict) -> tuple[str, str]:
    """One-line verdict and a markdown table for the job summary."""
    for field in ("rounds_count", "runs_per_mode", "files"):
        if type(report.get(field)) is not int or report[field] <= 0:
            raise ValueError("report counts must be positive integers")
    if report["gate"]["verdict"] not in ("PASS", "FAIL"):
        raise ValueError("invalid verdict")
    paired_medians(report["rounds"], report["rounds_count"], report["runs_per_mode"],
                   require_samples=report.get("schema", 1) >= 2)
    v = report["gate"]
    m = v["medians"]

    def heading(platform_name: str, label: str) -> str:
        return (f"{v['verdict']} dctx map find A/B ({platform_name}, {report['files']} files, "
                f"{report['rounds_count']} rounds x {report['runs_per_mode']} runs) median cold p95 ms: "
                f"candidate {m['candidate']['cold_p95_ms']:.1f} vs baseline {label} "
                f"{m['baseline']['cold_p95_ms']:.1f} ({(v['ratio'] - 1) * 100:+.1f}%, limit "
                f"+{v['max_regression'] * 100:g}%, ceiling {v['ceiling_ms']:g} ms, target {v['target_ms']:g} ms)")

    platform_name, label = (json.dumps(report[key]) for key in ("platform", "baseline_label"))
    line = heading(platform_name, label)
    markdown_heading = heading(markdown_literal(platform_name), markdown_literal(label))
    rows = ["| round | baseline cold p95 | candidate cold p95 | baseline warm p95 | candidate warm p95 |",
            "|---|---|---|---|---|"]
    for b, c in zip(report["rounds"]["baseline"], report["rounds"]["candidate"]):
        rows.append(f"| {b['round']} | {b['cold_p95_ms']:.1f} | {c['cold_p95_ms']:.1f} | "
                    f"{b['warm_p95_ms']:.1f} | {c['warm_p95_ms']:.1f} |")
    rows.append(f"| **median** | **{m['baseline']['cold_p95_ms']:.1f}** | **{m['candidate']['cold_p95_ms']:.1f}** | "
                f"{m['baseline']['warm_p95_ms']:.1f} | {m['candidate']['warm_p95_ms']:.1f} |")
    notes = [f"- {markdown_literal(json.dumps(reason))}" for reason in v["reasons"]]
    if v["target_missed"]:
        notes.append(f"- warning: candidate median cold p95 misses the {v['target_ms']:g} ms target "
                     "(informational, not a failure)")
    return line, "\n".join([f"### {markdown_heading}", "", *rows, "", *notes, ""])


class ArgumentParser(argparse.ArgumentParser):
    def error(self, message):
        # argparse errors can include arbitrary argv text (e.g. unknown flags).
        super().error(json.dumps(message))


def main(argv: list[str] | None = None) -> int:
    output_parser = ArgumentParser(prog="win_latency_ab.py", add_help=False, allow_abbrev=False)
    output_parser.add_argument("--output", type=Path, default=Path("target/win-latency/ab-report.json"))
    parser = ArgumentParser(prog="win_latency_ab.py", description=__doc__, parents=[output_parser],
                            allow_abbrev=False, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--baseline", required=True, help="baseline dctx executable")
    parser.add_argument("--baseline-label", default="baseline")
    parser.add_argument("--candidate", required=True, help="candidate dctx executable")
    parser.add_argument("--rounds", type=int, default=7)
    parser.add_argument("--runs", type=latency.run_count, default=latency.MIN_RUNS)
    parser.add_argument("--files", type=latency.positive_int, default=1500)
    parser.add_argument("--max-regression", type=positive_decimal, default=Decimal("0.20"))
    parser.add_argument("--ceiling-ms", type=positive_decimal, default=1000)
    parser.add_argument("--target-ms", type=positive_decimal, default=500)
    try:
        args = parser.parse_args(argv)
    except SystemExit as error:
        if error.code:
            output, _ = output_parser.parse_known_args(argv)
            try:
                output.output.unlink(missing_ok=True)
            except OSError as cleanup_error:
                parser.error(str(cleanup_error))
        raise
    try:
        args.output.unlink(missing_ok=True)
        if args.rounds < MIN_ROUNDS:
            parser.error(f"--rounds must be at least {MIN_ROUNDS}")
        if not 0 < args.max_regression < 10:
            parser.error("--max-regression must be a fraction such as 0.20")
        return compare(args)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        parser.error(str(error))


def compare(args: argparse.Namespace) -> int:
    paths = {}
    for name in ARMS:
        found = shutil.which(getattr(args, name))
        if found is None:
            raise ValueError(f"{name} executable not found")
        paths[name] = str(Path(found).resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    hashes = {}
    for name, path in paths.items():
        with open(path, "rb") as binary:
            hashes[name] = hashlib.file_digest(binary, "sha256").hexdigest()
    report = {
        "schema": 2,
        "binary_sha256": hashes,
        "timestamp_utc": datetime.now(timezone.utc).isoformat(),
        "run_id": os.environ.get("GITHUB_RUN_ID"),
        "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "checkout_sha": os.environ.get("GITHUB_SHA"),
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
        line, markdown = render(report)
        # Stage on the output filesystem. The temporary directory also cleans
        # up partial writes if serialization or replacement fails.
        staged = root / "report.json"
        staged.write_text(json.dumps(report, indent=2, allow_nan=False) + "\n", encoding="utf-8")
        os.replace(staged, args.output)
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
