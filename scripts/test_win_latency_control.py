# SPDX-License-Identifier: Apache-2.0
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch

from scripts import win_latency_control as control

ab = control.ab
TAG = 'v1"rc1'
HOSTILE = 'vX\n\x1b"'
BRANCH = "latency-control/test"
SHA = "0123456789abcdef" * 2 + "01234567"


def completed_job():
    steps = [("Test latency harness", "success"), (control.MEASURE_STEP, "failure"),
             (control.UPLOAD_STEP, "success")]
    return {"run_id": 12345, "run_attempt": 2, "head_sha": SHA, "head_branch": BRANCH,
            "name": "latency", "status": "completed", "conclusion": "failure",
            "steps": [{"name": name, "number": i, "status": "completed", "conclusion": conclusion}
                      for i, (name, conclusion) in enumerate(steps, start=5)]}


class ControlTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.report = self.root / "report.json"
        self.summary = self.root / "summary.md"
        self.job = self.root / "job.json"
        self.baseline = self.root / "baseline.exe"
        self.candidate = self.root / "candidate.exe"
        for binary in (self.baseline, self.candidate):
            binary.write_bytes(binary.stem.encode())
            binary.chmod(0o755)

    def run_gate(self, base=400, cand=410, enabled=True, label=TAG):
        env = {"INJECT_REGRESSION": str(enabled), "GITHUB_EVENT_NAME": "workflow_dispatch",
               "GITHUB_REF": "refs/heads/" + BRANCH, "GITHUB_SHA": SHA,
               "GITHUB_RUN_ID": "12345", "GITHUB_RUN_ATTEMPT": "2",
               "GITHUB_STEP_SUMMARY": str(self.summary)}
        # Ordinary production measure() reports. Use distinct, unsorted sub-ms
        # samples, with the 29th nearest rank exactly at the named duration.
        def fake_measure(binary, root, runs, cache_root):
            ms = {self.baseline.name: base, self.candidate.name: cand}[Path(binary).name]
            cold = [ms + (1 - i) / control.NS_PER_MS for i in range(runs)]
            return {"runs_per_mode": runs, "warmup_runs": 1, "percentile_method": "nearest-rank",
                    "cold": ab.latency.summarize(cold),
                    "warm": ab.latency.summarize(list(range(runs, 0, -1)))}

        out = StringIO()
        with patch.dict(os.environ, env), patch.object(ab.platform, "system", return_value="Windows"), \
                patch.object(ab.latency, "generate_fixture") as fixture, \
                patch.object(ab.latency, "measure", side_effect=fake_measure) as sampler, redirect_stdout(out):
            status = control.main(["measure", "--baseline", str(self.baseline), "--baseline-label", label,
                                   "--candidate", str(self.candidate), "--output", str(self.report)])
        fixture.assert_called_once()
        self.assertEqual(fixture.call_args.args[1], 1500)
        self.assertEqual(sampler.call_count, 14)
        report = json.loads(self.report.read_text(encoding="utf-8")) if self.report.exists() else None
        return status, report, out.getvalue()

    def test_production_gate_and_offline_verifier(self):
        status, report, _ = self.run_gate()
        self.assertEqual(status, 1)
        self.assertEqual(report["baseline_label"], TAG)
        self.assertEqual(report["gate"]["medians"]["candidate"]["cold_p95_ms"], 490)
        self.assertEqual(report["control"]["samples"][1]["cold"]["raw_ns"][:2], [410000001, 410000000])
        self.assertEqual(report["control"]["samples"][1]["cold"]["gated_ns"][:2], [490000001, 490000000])
        self.job.write_text(json.dumps(completed_job()), encoding="utf-8")
        with redirect_stdout(StringIO()):
            self.assertEqual(control.main(["verify", str(self.job), str(self.report)]), 0)

    def test_normal_gate_preserves_pass_and_failure_without_receipt(self):
        for candidate, expected in ((410, 0), (490, 1)):
            with self.subTest(candidate=candidate):
                status, report, _ = self.run_gate(cand=candidate, enabled=False)
                self.assertEqual(status, expected)
                self.assertEqual(report["gate"]["medians"]["candidate"]["cold_p95_ms"], candidate)
                self.assertNotIn("control", report)
                with self.assertRaises(KeyError):
                    control.verify(completed_job(), report)

    def test_error_after_verdict_does_not_leave_an_uploadable_receipt(self):
        self.report.write_text("previous report", encoding="utf-8")
        self.summary = self.root / "missing" / "summary.md"
        with redirect_stderr(StringIO()):
            status, report, _ = self.run_gate()
        self.assertEqual(status, 2)
        self.assertIsNone(report)

    def test_already_failing_candidate(self):
        status, report, _ = self.run_gate(cand=490)
        self.assertEqual(status, 1)
        with self.assertRaisesRegex(ValueError, "restore PASS"):
            control.verify(completed_job(), report)

    def test_ceiling_failure_is_not_relative_only(self):
        status, report, _ = self.run_gate(base=800, cand=950)
        self.assertEqual(status, 1)
        with self.assertRaisesRegex(ValueError, "relative limit alone"):
            control.verify(completed_job(), report)

    def test_rejects_unchanged_candidate_and_changes_to_other_samples(self):
        _, report, _ = self.run_gate()
        for i, mode in ((1, "cold"), (1, "warm"), (0, "cold"), (0, "warm")):
            with self.subTest(i=i, mode=mode):
                samples = report["control"]["samples"][i][mode]
                changed = samples["raw_ns"][0] + (0 if (i, mode) == (1, "cold") else 1)
                with patch.dict(samples, gated_ns=[changed, *samples["gated_ns"][1:]]), \
                        self.assertRaisesRegex(ValueError, "delta"):
                    control.verify(completed_job(), report)
        with patch.dict(report["control"], increment_ns=0), self.assertRaisesRegex(ValueError, "increment"):
            control.verify(completed_job(), report)

    def test_rejects_wrong_shape_order_or_report(self):
        _, report, _ = self.run_gate()
        rows = report["control"]["samples"]
        for obj, key, value in ((report, "order", list(reversed(report["order"]))),
                                (report["control"], "samples", rows[:-1]),
                                (rows[0], "round", 1), (rows[0], "arm", "candidate"),
                                (rows[0]["warm"], "gated_ns", [1] * 29),
                                (rows[1]["cold"], "raw_ns", [410000000.0] * 30),
                                (report["rounds"]["candidate"][0], "cold_samples_ns",
                                 list(reversed(rows[1]["cold"]["gated_ns"]))),
                                (report["rounds"]["candidate"][0], "cold_p95_ms", 410),
                                (report["gate"], "ratio", 1.0)):
            with self.subTest(key=key), patch.dict(obj, {key: value}), self.assertRaises(ValueError):
                control.verify(completed_job(), report)

    def test_rejects_unrelated_failure_or_unsuccessful_upload(self):
        _, report, _ = self.run_gate()
        job = completed_job()
        harness, measure, upload = job["steps"]
        with patch.dict(harness, conclusion="failure"), patch.dict(measure, conclusion="success"), \
                self.assertRaises(ValueError):
            control.verify(job, report)
        for step, key, value in ((upload, "conclusion", "skipped"), (upload, "number", 1),
                                 (measure, "status", "in_progress"), (job, "conclusion", "success")):
            with self.subTest(key=key), patch.dict(step, {key: value}), self.assertRaises(ValueError):
                control.verify(job, report)

    def test_swapped_artifact(self):
        _, report, _ = self.run_gate()
        for key, value in (("run_id", 23456), ("run_attempt", 1), ("head_sha", "a" * 40)):
            with self.subTest(key=key), patch.dict(report["control"], {key: value}), self.assertRaises(ValueError):
                control.verify(completed_job(), report)

    def test_control_requires_manual_disposable_branch(self):
        for event, ref in (("push", "refs/heads/" + BRANCH), ("workflow_dispatch", "refs/heads/main"),
                           ("workflow_dispatch", "refs/tags/v1"), ("workflow_dispatch", "refs/heads/latency-control/")):
            with self.subTest(event=event, ref=ref), self.assertRaisesRegex(ValueError, "manual dispatch"):
                control.control_identity({"GITHUB_EVENT_NAME": event, "GITHUB_REF": ref})

    def test_escaped_label_on_both_display_surfaces(self):
        for label in (HOSTILE, "**rc** [notes](#details) `tag`"):
            self.summary.unlink(missing_ok=True)
            _, report, output = self.run_gate(label=label, enabled=False)
            heading = self.summary.read_text(encoding="utf-8").splitlines()[0]
            self.assertEqual(report["baseline_label"], label)
            self.assertEqual(len(output.splitlines()), 1)
            quoted = json.dumps(label)
            self.assertIn(quoted, output)
            fence = "``" if "`" in label else "`"
            self.assertIn(f"{fence} {quoted} {fence}", heading)
            self.assertNotIn("\x1b", heading + output)

    def test_hostile_verifier_error_is_one_escaped_line(self):
        errors = StringIO()
        with patch.object(Path, "read_text", side_effect=OSError(HOSTILE)), redirect_stderr(errors):
            self.assertEqual(control.main(["verify", str(self.job), str(self.report)]), 2)
        self.assertEqual(errors.getvalue().splitlines(), ["Windows latency error " + json.dumps(HOSTILE)])

    def test_workflow_passes_literal_tag_as_one_argument(self):
        path = Path(__file__).resolve().parents[1] / ".github" / "workflows" / "win-latency.yml"
        workflow = path.read_text(encoding="utf-8")
        step = workflow.split("        id: measure\n", 1)[1].split("\n      - name:", 1)[0]
        env, script = step.split("        run: |\n", 1)
        self.assertIn("BASELINE_TAG: ${{ steps.baseline.outputs.tag }}", env)
        self.assertIn("github.event_name == 'workflow_dispatch' && inputs.inject_regression", env)
        self.assertNotIn("${{", script)
        self.assertIn("      - name: " + control.MEASURE_STEP + "\n", workflow)
        self.assertIn("      - name: " + control.UPLOAD_STEP + "\n", workflow)
        recorder = r"""python() { printf '%s\0' "$@"; }"""
        for label in (TAG, HOSTILE):
            args = subprocess.run(["bash", "-e", "-c", recorder + "\n" + textwrap.dedent(script)],
                                  env={**os.environ, "BASELINE_TAG": label, "RUNNER_TEMP": "."},
                                  capture_output=True, check=True).stdout.split(b"\0")
            self.assertEqual(args[args.index(b"--baseline-label") + 1:args.index(b"--candidate")],
                             [label.encode()])


if __name__ == "__main__":
    unittest.main()
