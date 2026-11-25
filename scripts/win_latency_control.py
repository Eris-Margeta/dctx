#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Run the hosted A/B measurement, optionally recording a candidate cold regression.

Offline: python scripts/win_latency_control.py verify job.json report.json
Supply the completed latency job object from the Actions REST jobs API for the
specific attempt, and report.json from that attempt's win-latency artifact.
"""
from __future__ import annotations

import argparse
from contextlib import ExitStack
from functools import partial
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from unittest.mock import patch

if __package__:
    from . import win_latency_ab as ab
else:
    import win_latency_ab as ab

NS_PER_MS = 1_000_000
INCREMENT_NS = 80 * NS_PER_MS
ROUNDS, RUNS, FILES = 7, 30, 1500
REGRESSION, CEILING_MS, TARGET_MS = 0.20, 1000, 500
BRANCH_PREFIX = "latency-control/"
MEASURE_STEP = ("A/B against the latest release, 7 interleaved rounds of 30 cold "
                "and 30 warm processes over 1500 files")
UPLOAD_STEP = "Upload latency JSON"


def require(condition, message):
    if not condition:
        raise ValueError(message)


def control_identity(env):
    prefix = "refs/heads/" + BRANCH_PREFIX
    ref = env.get("GITHUB_REF", "")
    require(env.get("GITHUB_EVENT_NAME") == "workflow_dispatch"
            and ref.startswith(prefix) and len(ref) > len(prefix),
            "control requires manual dispatch on a disposable latency-control/* branch")
    return {"run_id": int(env["GITHUB_RUN_ID"]), "run_attempt": int(env["GITHUB_RUN_ATTEMPT"]),
            "head_sha": env["GITHUB_SHA"], "head_branch": ref.removeprefix("refs/heads/")}


def recorded_rounds(run_rounds, identity, baseline, candidate, root, rounds, runs):
    require(baseline != candidate, "control requires distinct executable paths")
    measure = ab.latency.measure
    arms = {baseline: "baseline", candidate: "candidate"}
    samples = []
    counts = {"baseline": 0, "candidate": 0}

    def record(binary, root, runs, cache_root):
        result = measure(binary, root, runs, cache_root=cache_root)
        arm = arms[binary]
        row = {"arm": arm, "round": counts[arm]}
        for mode in ("cold", "warm"):
            # The public sampler reports perf_counter_ns differences divided by 1e6.
            raw = [round(ms * NS_PER_MS) for ms in result[mode]["samples_ms"]]
            delta = INCREMENT_NS if (arm, mode) == ("candidate", "cold") else 0
            gated = [ns + delta for ns in raw]
            row[mode] = {"raw_ns": raw, "gated_ns": gated}
            result[mode] = ab.latency.summarize([ns / NS_PER_MS for ns in gated])
        samples.append(row)
        counts[arm] += 1
        return result

    with patch.object(ab.latency, "measure", record):
        result = run_rounds(baseline, candidate, root, rounds, runs)
    result["control"] = {**identity, "increment_ns": INCREMENT_NS, "samples": samples}
    return result


def measure(args):
    with ExitStack() as scope:
        output = Path(args.output)
        gate_output = output
        if os.environ.get("INJECT_REGRESSION", "false").lower() == "true":
            identity = control_identity(os.environ)
            record = partial(recorded_rounds, ab.run_rounds, identity)
            scope.enter_context(patch.object(ab, "run_rounds", record))
            # A failed summary write also makes Actions mark the step red. Only
            # expose a receipt once the production entry point returned its verdict.
            output.parent.mkdir(parents=True, exist_ok=True)
            output.unlink(missing_ok=True)
            tmp = scope.enter_context(tempfile.TemporaryDirectory(dir=output.parent))
            gate_output = Path(tmp) / "report.json"
        try:
            status = ab.main(["--baseline", args.baseline, "--baseline-label=" + args.baseline_label,
                              "--candidate", args.candidate, "--output", str(gate_output),
                              "--rounds", str(ROUNDS), "--runs", str(RUNS), "--files", str(FILES),
                              "--max-regression", str(REGRESSION), "--ceiling-ms", str(CEILING_MS),
                              "--target-ms", str(TARGET_MS)])
        except SystemExit as error:
            if error.code != 2:
                raise
            return 2
        if gate_output != output:
            gate_output.replace(output)
        return status


def check_job(job, control):
    for key in ("run_id", "run_attempt"):
        require(type(job[key]) is int and type(control[key]) is int
                and job[key] == control[key] > 0, "run ID or attempt mismatch")
    require(re.fullmatch(r"[0-9a-f]{40}", job["head_sha"]) and job["head_sha"] == control["head_sha"],
            "candidate SHA mismatch")
    require(job["head_branch"] == control["head_branch"] and job["head_branch"].startswith(BRANCH_PREFIX)
            and len(job["head_branch"]) > len(BRANCH_PREFIX), "disposable candidate branch mismatch")
    require(job["name"] == "latency" and job["status"] == "completed" and job["conclusion"] == "failure",
            "expected a completed, failed latency job")
    steps = job["steps"]
    numbers = []
    for name, conclusion in ((MEASURE_STEP, "failure"), (UPLOAD_STEP, "success")):
        matches = [step for step in steps if step["name"] == name]
        require(len(matches) == 1 and matches[0]["status"] == "completed"
                and matches[0]["conclusion"] == conclusion, "measurement or upload step mismatch")
        numbers.append(matches[0]["number"])
    require(all(type(n) is int for n in numbers) and 0 < numbers[0] < numbers[1],
            "upload must follow measurement")
    require(all(step["conclusion"] in ("success", "skipped") for step in steps
                if step["name"] != MEASURE_STEP), "another step failed")


def replay_samples(mode_samples, delta):
    raw, gated = mode_samples["raw_ns"], mode_samples["gated_ns"]
    require(isinstance(raw, list) and isinstance(gated, list) and len(raw) == len(gated) == RUNS,
            "wrong number of timed samples")
    for before, after in zip(raw, gated):
        require(type(before) is int and type(after) is int and before > 0 and after - before == delta,
                "invalid nanosecond sample or unexpected delta")
    return gated, raw


def verify(job, report):
    control = report["control"]
    check_job(job, control)
    require(report["platform"] == "Windows" and report["files"] == FILES
            and report["runs_per_mode"] == RUNS and report["rounds_count"] == ROUNDS,
            "wrong measurement protocol")
    require(type(control["increment_ns"]) is int and control["increment_ns"] == INCREMENT_NS,
            "wrong control increment")
    order = [arm for i in range(ROUNDS) for arm in
             (("baseline", "candidate") if i % 2 == 0 else ("candidate", "baseline"))]
    samples = control["samples"]
    require(report["order"] == order and isinstance(samples, list) and len(samples) == len(order),
            "missing or unordered rounds")
    injected = {"baseline": [], "candidate": []}
    restored = {"baseline": [], "candidate": []}
    for index, (arm, row) in enumerate(zip(order, samples)):
        number = index // 2
        require(row["arm"] == arm and type(row["round"]) is int and row["round"] == number,
                "sample arm or round mismatch")
        cold = replay_samples(row["cold"], INCREMENT_NS if arm == "candidate" else 0)
        warm = replay_samples(row["warm"], 0)
        for rounds, c, w in zip((injected, restored), cold, warm):
            rounds[arm].append({
                "round": number,
                "cold_samples_ns": c,
                "warm_samples_ns": w,
                **{key: float(value) for key, value in ab.row_stats(c, w).items()},
            })
    gate = ab.verdict(injected, REGRESSION, CEILING_MS, TARGET_MS)
    require(report["rounds"] == injected and report["gate"] == gate, "production report disagrees with replay")
    require(gate["verdict"] == "FAIL" and gate["medians"]["candidate"]["cold_p95_ms"] <= CEILING_MS,
            "injected gate must fail on the relative limit alone")
    require(ab.verdict(restored, REGRESSION, CEILING_MS, TARGET_MS)["verdict"] == "PASS",
            "removing the increment did not restore PASS")


class Parser(argparse.ArgumentParser):
    def error(self, message):
        super().error(json.dumps(message, ensure_ascii=True))


def main(argv=None):
    parser = Parser(prog="win_latency_control.py", description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("measure")
    for flag in ("baseline", "baseline-label", "candidate", "output"):
        run.add_argument("--" + flag, required=True)
    check = commands.add_parser("verify")
    check.add_argument("job", type=Path)
    check.add_argument("report", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "measure":
            return measure(args)
        verify(json.loads(args.job.read_text(encoding="utf-8")),
               json.loads(args.report.read_text(encoding="utf-8")))
    except (OSError, ValueError, TypeError, KeyError, AttributeError, ArithmeticError,
            RecursionError, RuntimeError, subprocess.SubprocessError) as exc:
        print("Windows latency error " + json.dumps(str(exc), ensure_ascii=True), file=sys.stderr)
        return 2
    print("Verified relative-only FAIL with counterfactual PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
