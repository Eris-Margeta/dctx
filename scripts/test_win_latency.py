# SPDX-License-Identifier: Apache-2.0
"""Deterministic fixture and subprocess accounting tests; no timing assertions."""
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import win_latency as latency


def process_result():
    return subprocess.CompletedProcess([], 0, stdout=f"s:fixture.rs#{latency.TERM}".encode())


def clock_ticks(durations_ms):
    return [tick for ms in durations_ms for tick in (0, ms * 1_000_000)]


class PercentileTests(unittest.TestCase):
    def test_nearest_rank_and_unsorted_input(self):
        samples = list(range(30, 0, -1))
        self.assertEqual(latency.percentile(samples, 0.5), 15)
        self.assertEqual(latency.percentile(samples, 0.95), 29)
        self.assertEqual(latency.percentile(samples, 1), 30)
        self.assertEqual(samples[0], 30)

    def test_singleton_and_repeated_samples(self):
        self.assertEqual(latency.percentile([7], 0.95), 7)
        self.assertEqual(latency.percentile([4] * 30, 0.5), 4)

    def test_invalid_input(self):
        for samples, fraction in (([], 0.5), ([1], 0), ([1], -1), ([1], 1.1)):
            with self.subTest(samples=samples, fraction=fraction):
                with self.assertRaises(ValueError):
                    latency.percentile(samples, fraction)


class FixtureTests(unittest.TestCase):
    def test_default_1500_sources_and_one_match(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            root = Path(tmp) / "fixture"
            now = 1_800_000_000_000_000_000
            with patch.object(latency.time, "time_ns", return_value=now):
                latency.generate_fixture(root)
            sources = sorted(root.rglob("*.rs"))
            self.assertEqual(len(sources), 1500)
            self.assertEqual(len([p for p in root.rglob("*") if p.is_file()]), 1500)
            self.assertEqual(sum(latency.TERM in p.read_text() for p in sources), 1)
            self.assertEqual(len(list(root.iterdir())), 60)
            self.assertTrue(all(p.stat().st_mtime_ns <= now - 5_000_000_000 for p in sources))

    def test_custom_fixture_is_deterministic(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            roots = [Path(tmp) / name for name in ("a", "b")]
            for root in roots:
                latency.generate_fixture(root, 51)
            contents = [{p.relative_to(root).as_posix(): p.read_bytes()
                         for p in root.rglob("*.rs")} for root in roots]
            self.assertEqual(contents[0], contents[1])
            self.assertEqual(len(contents[0]), 51)
            self.assertIn("group_0002/item_0050.rs", contents[0])

    def test_nonpositive_count_rejected(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            root = Path(tmp) / "fixture"
            for count in (0, -1):
                with self.assertRaises(ValueError):
                    latency.generate_fixture(root, count)
            self.assertFalse(root.exists())


class MeasurementTests(unittest.TestCase):
    def test_separate_processes_and_exact_accounting(self):
        root = Path.cwd() / "fixture-root"
        cold = list(range(30, 0, -1))
        warm = list(range(2, 61, 2))
        with patch.object(latency.subprocess, "run", return_value=process_result()) as run, patch.object(
            latency.time, "perf_counter_ns", side_effect=clock_ticks([*cold, 999, *warm])
        ):
            report = latency.measure("dctx", root, 30)
        self.assertEqual(run.call_count, 61)
        self.assertEqual(report["runs_per_mode"], 30)
        self.assertEqual(report["warmup_runs"], 1)
        self.assertEqual(report["cold"], dict(samples_ms=cold, p50_ms=15, p95_ms=29, max_ms=30))
        self.assertEqual(report["warm"], dict(samples_ms=warm, p50_ms=30, p95_ms=58, max_ms=60))
        caches = [call.kwargs["env"]["DCTX_CACHE_DIR"] for call in run.call_args_list]
        self.assertEqual(len(set(caches[:30])), 30)
        self.assertEqual(caches[30:], [caches[30]] * 31)
        self.assertNotIn(caches[30], caches[:30])
        for call in run.call_args_list:
            self.assertEqual(call.args[0], ["dctx", "map", "find", latency.TERM, "--place", "."])
            self.assertEqual(call.kwargs["cwd"], root / "fixture")
            self.assertTrue(call.kwargs["check"])

    def test_missing_match_is_an_error(self):
        result = subprocess.CompletedProcess([], 0, stdout=b"no matches")
        with patch.object(latency.subprocess, "run", return_value=result):
            with self.assertRaises(RuntimeError):
                latency.measure("dctx", Path("fixture-root"), 30)

    def test_failed_process_is_an_error(self):
        with patch.object(latency.subprocess, "run", side_effect=subprocess.CalledProcessError(1, [])):
            with self.assertRaises(subprocess.CalledProcessError):
                latency.measure("dctx", Path("fixture-root"), 30)

    def test_dry_report_has_platform_and_no_fabricated_samples(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            output = Path(tmp) / "report.json"
            with patch.dict(latency.os.environ, {"GITHUB_STEP_SUMMARY": ""}), patch.object(
                latency.subprocess, "run"
            ) as run, redirect_stdout(StringIO()):
                status = latency.main(["--dry", "--files", "50", "--output", str(output)])
            report = json.loads(output.read_text())
            self.assertEqual(status, 0)
            self.assertTrue(report["dry"])
            self.assertEqual(report["files"], 50)
            self.assertEqual(report["platform"], latency.platform.system())
            self.assertNotIn("cold", report)
            self.assertNotIn("warm", report)
            run.assert_not_called()


class GateTests(unittest.TestCase):
    def run_gate(self, cold, warm, *flags):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            output = Path(tmp) / "report.json"
            summary = Path(tmp) / "summary.md"
            stdout = StringIO()
            with patch.dict(latency.os.environ, {"GITHUB_STEP_SUMMARY": str(summary)}), patch.object(
                latency.shutil, "which", return_value="dctx"
            ), patch.object(latency.subprocess, "run", return_value=process_result()), patch.object(
                latency.time, "perf_counter_ns", side_effect=clock_ticks([*cold, 999, *warm])
            ), redirect_stdout(stdout):
                status = latency.main(["--files", "1", "--output", str(output), *flags])
            report = json.loads(output.read_text())
            verdict = stdout.getvalue().splitlines()
            self.assertEqual(len(verdict), 1)
            self.assertEqual(verdict, summary.read_text().splitlines())
        return status, report, verdict[0]

    def test_default_limit_passes_at_p95_even_with_higher_max(self):
        status, report, verdict = self.run_gate([500] * 29 + [501], [499] * 30)
        self.assertEqual(status, 0)
        self.assertEqual(report["max_p95_ms"], 500)
        self.assertEqual(report["cold"]["p95_ms"], 500)
        self.assertEqual(report["cold"]["max_ms"], 501)
        self.assertEqual(report["verdict"], "PASS")
        self.assertTrue(verdict.startswith("PASS "))

    def test_custom_limit_fails_for_either_mode_and_preserves_report(self):
        for cold_ms, warm_ms in ((101, 100), (100, 101)):
            with self.subTest(cold_ms=cold_ms, warm_ms=warm_ms):
                status, report, verdict = self.run_gate([cold_ms] * 30, [warm_ms] * 30,
                                                        "--max-p95-ms", "100")
                self.assertEqual(status, 1)
                self.assertEqual(report["max_p95_ms"], 100)
                self.assertEqual(report["cold"]["p95_ms"], cold_ms)
                self.assertEqual(report["warm"]["p95_ms"], warm_ms)
                self.assertEqual(report["verdict"], "FAIL")
                self.assertTrue(verdict.startswith("FAIL "))

    def test_rejects_fewer_than_30_runs(self):
        with redirect_stderr(StringIO()), self.assertRaises(SystemExit) as error:
            latency.main(["--dry", "--runs", "29"])
        self.assertEqual(error.exception.code, 2)

    def test_rejects_invalid_limits(self):
        for limit in ("0", "-1", "nan", "inf"):
            with self.subTest(limit=limit), redirect_stderr(StringIO()), self.assertRaises(SystemExit) as error:
                latency.main(["--dry", "--max-p95-ms", limit])
            self.assertEqual(error.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
