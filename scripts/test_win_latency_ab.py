# SPDX-License-Identifier: Apache-2.0
from contextlib import redirect_stderr, redirect_stdout
from datetime import datetime, timezone
from hashlib import sha256
from io import StringIO
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import win_latency_ab as ab


def fake_measure(cold_by_binary):
    """measure() stub: cold p95 per binary from an iterator per binary, warm fixed."""
    iters = {name: iter(values) for name, values in cold_by_binary.items()}
    calls = []

    def measure(binary, root, runs, cache_root=None):
        calls.append((Path(binary).name, cache_root))
        cold = next(iters[Path(binary).stem])
        return {"cold": {"samples_ms": [cold] * runs}, "warm": {"samples_ms": [20.0] * runs}}

    return measure, calls


class RoundTests(unittest.TestCase):
    def test_interleaved_alternating_order_and_fresh_caches(self):
        measure, calls = fake_measure({"base": [400] * 5, "cand": [410] * 5})
        with patch.object(ab.latency, "measure", side_effect=measure):
            result = ab.run_rounds("base", "cand", Path("root"), 5, 30)
        self.assertEqual(result["order"], ["baseline", "candidate", "candidate", "baseline",
                                           "baseline", "candidate", "candidate", "baseline",
                                           "baseline", "candidate"])
        self.assertEqual(len({cache for _, cache in calls}), 10)
        self.assertEqual(len(result["rounds"]["baseline"]), 5)

    def test_rejects_fewer_than_five_rounds(self):
        with self.assertRaises(ValueError):
            ab.run_rounds("base", "cand", Path("root"), 4, 30)


class GateTests(unittest.TestCase):
    def gate(self, base, cand, **kw):
        rounds = {"baseline": [{"cold_p95_ms": v, "warm_p95_ms": 1} for v in base],
                  "candidate": [{"cold_p95_ms": v, "warm_p95_ms": 1} for v in cand]}
        return ab.verdict(rounds, kw.get("max_regression", 0.20), kw.get("ceiling", 1000), 500)

    def test_runner_noise_passes(self):
        # Identical code on a noisy runner: both sides move together.
        v = self.gate([430, 540, 700, 520, 610], [450, 530, 690, 560, 600])
        self.assertEqual(v["verdict"], "PASS")
        self.assertTrue(v["target_missed"])

    def test_one_outlier_round_does_not_fail(self):
        v = self.gate([450] * 5, [460, 470, 1400, 455, 465])
        self.assertEqual(v["verdict"], "PASS")

    def test_relative_regression_fails(self):
        v = self.gate([400] * 5, [490] * 5)
        self.assertEqual(v["verdict"], "FAIL")
        self.assertIn("slower than baseline", v["reasons"][0])

    def test_exactly_twenty_percent_passes(self):
        self.assertEqual(self.gate([400] * 5, [480] * 5)["verdict"], "PASS")

    def test_absolute_ceiling_fails_even_if_baseline_is_slow(self):
        v = self.gate([1000] * 5, [1001] * 5)
        self.assertEqual(v["verdict"], "FAIL")
        self.assertIn("ceiling", v["reasons"][0])

    def test_nanosecond_boundaries(self):
        for cand, status in ((480.000012, "PASS"), (480.000013, "FAIL")):
            with self.subTest(cand=cand):
                v = self.gate([400.000010] * 5, [cand] * 5)
                self.assertEqual(v["verdict"], status)
                if status == "FAIL":
                    self.assertIn("480000013 ns", v["reasons"][0])
                    self.assertIn("480000012 ns", v["reasons"][0])

        for ms, status, missed in ((500, "PASS", False), (500.000001, "PASS", True),
                                   (1000, "PASS", True), (1000.000001, "FAIL", True)):
            with self.subTest(ms=ms):
                gate = self.gate([ms] * 5, [ms] * 5)
                self.assertEqual((gate["verdict"], gate["target_missed"]), (status, missed))

    def test_even_medians(self):
        # Six rounds. The exact baseline median is 400000012.5 ns.
        baseline = [400.000012, 400.000013] * 3
        for high, status in ((480.000016, "PASS"), (480.000017, "FAIL")):
            v = self.gate(baseline, [480.000014, high] * 3)
            self.assertEqual(v["verdict"], status)
        v = self.gate([1000] * 6, [1000, 1000.000001] * 3)
        self.assertEqual(v["verdict"], "FAIL")
        self.assertIn("2000000001/2 ns", v["reasons"][0])

    def test_invalid_pairs(self):
        rows = [dict(round=i, cold_p95_ms=400, warm_p95_ms=20) for i in range(5)]
        for candidate in (None, [], rows[:-1], [*rows[:-1], rows[-2]], rows[1:] + rows[:1],
                          [*rows[:-1], dict(cold_p95_ms=400, warm_p95_ms=20)]):
            with self.subTest(candidate=candidate), self.assertRaises(ValueError):
                ab.verdict(dict(baseline=rows, candidate=candidate), .2, 1000, 500)
        with self.assertRaises(ValueError):
            ab.verdict(dict(baseline=rows), .2, 1000, 500)

    def test_invalid_timings(self):
        for value in (0, -1, float("nan"), float("inf"), -float("inf")):
            for field in ("cold_p95_ms", "warm_p95_ms", "cold_p50_ms"):
                row = dict(cold_p95_ms=400, warm_p95_ms=20)
                row[field] = value
                with self.subTest(value=value, field=field), self.assertRaises(ValueError):
                    ab.verdict(dict(baseline=[row] * 5, candidate=[row] * 5), .2, 1000, 500)


class MainTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(dir=Path.cwd())
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.output = self.root / "ab.json"
        self.summary = self.root / "summary.md"
        self.binaries = [self.root / name for name in ("base.exe", "cand.exe")]
        for binary in self.binaries:
            binary.write_bytes(binary.stem.encode())
            binary.chmod(0o755)
        self.args = ["--baseline", str(self.binaries[0]), "--candidate", str(self.binaries[1]),
                     "--rounds", "5", "--files", "1", "--output", str(self.output)]
        self.stdout = StringIO()
        self.enterContext(redirect_stdout(self.stdout))
        self.stderr = StringIO()
        self.enterContext(redirect_stderr(self.stderr))
        self.enterContext(patch.dict(ab.os.environ, {"GITHUB_STEP_SUMMARY": str(self.summary),
                                                     "GITHUB_ACTIONS": "true"}))

    def read_report(self, path):
        def reject_constant(token):
            self.fail("nonfinite JSON number " + json.dumps(token))
        return json.loads(path.read_text(encoding="utf-8"), parse_constant=reject_constant)

    def test_main_writes_report_summary_and_exit_code(self):
        replace = ab.os.replace

        def observed_replace(src, dst):
            self.assertEqual(dst, self.output)
            self.assertFalse(dst.exists())
            self.assertEqual(self.read_report(src)["gate"]["verdict"], expected)
            replace(src, dst)

        for cand, status in ((480.000012, 0), (480.000013, 1), (700, 1)):
            expected = "FAIL" if status else "PASS"
            measure, _ = fake_measure({"base": [400.000010] * 5, "cand": [cand] * 5})
            with self.subTest(cand=cand), patch.object(ab.latency, "measure", side_effect=measure), \
                    patch.object(ab.os, "replace", side_effect=observed_replace) as publish:
                self.assertEqual(ab.main(self.args), status)
                publish.assert_called_once()
                self.assertEqual(self.read_report(self.output)["gate"]["verdict"], expected)
                self.assertIn("median", self.summary.read_text())
        self.assertIn("::warning", self.stdout.getvalue())

    def test_report_provenance_and_sample_replay(self):
        utc = datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc)
        actions = {"GITHUB_SHA": "a" * 40, "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2"}
        ticks = []
        evidence = {"baseline": [], "candidate": []}
        for i in range(5):
            for arm in (("candidate", "baseline") if i % 2 else ("baseline", "candidate")):
                start = (480_000_000 if arm == "candidate" else 400_000_000) + i
                cold = [start + j for j in reversed(range(30))]
                warm = [start // 2 + j for j in range(30)]
                evidence[arm].append((cold, warm))
                for ns in [*cold, 12345, *warm]:
                    ticks.extend((1_000_000, 1_000_000 + ns))
        result = subprocess.CompletedProcess([], 0, stdout=b"fixture#win_latency_probe")
        with patch.dict(ab.os.environ, actions), patch.object(ab, "datetime") as clock, \
                patch.object(ab.latency.time, "perf_counter_ns", side_effect=ticks), \
                patch.object(ab.latency.subprocess, "run", return_value=result):
            clock.now.return_value = utc
            self.assertEqual(ab.main(self.args), 0)
        clock.now.assert_called_once_with(timezone.utc)
        report = self.read_report(self.output)
        self.assertEqual(datetime.fromisoformat(report["timestamp_utc"]), utc)
        self.assertEqual((report["checkout_sha"], report["run_id"], report["run_attempt"]),
                         ("a" * 40, "123", "2"))
        self.assertEqual(report["binary_sha256"], {arm: sha256(binary.read_bytes()).hexdigest()
                                                   for arm, binary in zip(evidence, self.binaries)})
        medians = {}
        for arm, rounds in report["rounds"].items():
            p95s = {"cold": [], "warm": []}
            self.assertEqual(len(rounds), report["rounds_count"])
            for i, row in enumerate(rounds):
                self.assertEqual(row["round"], i)
                for mode, expected in zip(p95s, evidence[arm][i]):
                    samples = row[mode + "_samples_ns"]
                    self.assertTrue(all(type(ns) is int for ns in samples))
                    self.assertEqual(samples, expected)
                    self.assertEqual(len(samples), report["runs_per_mode"])
                    p95 = sorted(samples)[28]
                    p95s[mode].append(p95)
                    self.assertEqual(row[mode + "_p95_ms"], p95 / 1_000_000)
                self.assertEqual(row["cold_p50_ms"], sorted(row["cold_samples_ns"])[14] / 1_000_000)
            for mode, values in p95s.items():
                median = sorted(values)[2]
                self.assertEqual(report["gate"]["medians"][arm][mode + "_p95_ms"], median / 1_000_000)
            medians[arm] = sorted(p95s["cold"])[2]
        passed = medians["candidate"] * 5 <= medians["baseline"] * 6 and medians["candidate"] <= 10**9
        self.assertEqual(report["gate"]["verdict"], "PASS" if passed else "FAIL")
        self.assertEqual(ab.verdict(report["rounds"], **report["gate"]["limits_exact"]), report["gate"])

    def test_decimal_cli_limit_is_replayable(self):
        measure, _ = fake_measure({"base": [400] * 5, "cand": [480] * 5})
        with patch.object(ab.latency, "measure", side_effect=measure):
            self.assertEqual(ab.main(self.args + ["--max-regression", "0.199999999999999999"]), 1)
        report = self.read_report(self.output)
        self.assertEqual(ab.verdict(report["rounds"], **report["gate"]["limits_exact"]), report["gate"])

    def test_render_rejects_inconsistent_counts_and_samples(self):
        measure, _ = fake_measure({"base": [400] * 5, "cand": [410] * 5})
        with patch.object(ab.latency, "measure", side_effect=measure):
            self.assertEqual(ab.main(self.args), 0)
        for field, value in (("rounds_count", 6), ("runs_per_mode", 31), ("runs_per_mode", None)):
            with self.subTest(field=field), self.assertRaises(ValueError):
                ab.render(dict(self.read_report(self.output), **{field: value}))
        for field, value in (("round", 0), ("warm_samples_ns", [1] * 29),
                             ("cold_samples_ns", [1] * 29 + [float("nan")]), ("cold_p95_ms", 300)):
            report = self.read_report(self.output)
            report["rounds"]["candidate"][-1][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                ab.render(report)

    def test_failed_attempt_artifact(self):
        hostile = 'failed\n\x1b"'
        for flags in (["--baseline", str(self.root / "absent.exe")], ["--rounds", "4"],
                      ["--target-ms", "NaN"], ["--runs", "29"], []):
            with self.subTest(flags=flags):
                self.output.write_text('{"gate":{"verdict":"PASS"}}')
                first, _ = fake_measure({"base": [400]})
                with patch.object(ab.latency, "measure", side_effect=[
                    first("base", self.root, 30), RuntimeError(hostile)
                ]), self.assertRaises(SystemExit) as error:
                    ab.main(self.args + flags)
                self.assertNotEqual(error.exception.code, 0)
                self.assertFalse(self.output.exists())
        self.assertNotIn(hostile, self.stderr.getvalue())
        self.assertIn(json.dumps(hostile), self.stderr.getvalue())

    def test_escaped_baseline_label(self):
        label = 'rc\n\x1b" **bold** [link](#details) `` <img> |'
        measure, _ = fake_measure({"base": [400] * 5, "cand": [400] * 5})
        with patch.object(ab.latency, "measure", side_effect=measure):
            self.assertEqual(ab.main(self.args + ["--baseline-label", label]), 0)
        self.assertEqual(self.read_report(self.output)["baseline_label"], label)
        self.assertEqual(len(self.stdout.getvalue().splitlines()), 1)
        self.assertIn(json.dumps(label), self.stdout.getvalue())
        headings = [line for line in self.summary.read_text().splitlines() if line.startswith("### ")]
        self.assertEqual(len(headings), 1)
        # A longer matching CommonMark code-span delimiter keeps all label markup literal.
        self.assertIn("``` " + json.dumps(label) + " ```", headings[0])


if __name__ == "__main__":
    unittest.main()
