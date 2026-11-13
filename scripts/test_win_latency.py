# SPDX-License-Identifier: Apache-2.0
"""Deterministic fixture and subprocess accounting tests; no timing assertions."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import win_latency as latency


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
        result = subprocess.CompletedProcess([], 0, stdout=latency.TERM.encode())
        ticks = [0, 1_000_000, 0, 3_000_000, 0, 2_000_000]
        with patch.object(latency.subprocess, "run", return_value=result) as run, patch.object(
            latency.time, "perf_counter_ns", side_effect=ticks
        ):
            report = latency.measure("dctx", Path("fixture-root"), 3)
        self.assertEqual(run.call_count, 4)
        self.assertEqual(report["samples_ms"], [1, 3, 2])
        self.assertEqual((report["p50_ms"], report["p95_ms"], report["max_ms"]), (2, 3, 3))
        self.assertEqual(report["warmup_runs"], 1)
        self.assertEqual(run.call_args.args[0][1:4], ["map", "find", latency.TERM])
        self.assertTrue(run.call_args.kwargs["check"])

    def test_missing_match_is_an_error(self):
        result = subprocess.CompletedProcess([], 0, stdout=b"no matches")
        with patch.object(latency.subprocess, "run", return_value=result):
            with self.assertRaises(RuntimeError):
                latency.measure("dctx", Path("fixture-root"), 3)

    def test_failed_process_is_an_error(self):
        with patch.object(latency.subprocess, "run", side_effect=subprocess.CalledProcessError(1, [])):
            with self.assertRaises(subprocess.CalledProcessError):
                latency.measure("dctx", Path("fixture-root"), 3)

    def test_dry_report_has_platform_and_no_fabricated_samples(self):
        with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
            output = Path(tmp) / "report.json"
            with patch("sys.argv", ["win_latency.py", "--dry", "--files", "50",
                                    "--runs", "3", "--output", str(output)]), patch.dict(
                latency.os.environ, {"GITHUB_STEP_SUMMARY": ""}
            ), patch.object(latency.subprocess, "run") as run:
                latency.main()
            report = json.loads(output.read_text())
            self.assertTrue(report["dry"])
            self.assertEqual(report["files"], 50)
            self.assertEqual(report["platform"], latency.platform.system())
            self.assertNotIn("samples_ms", report)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
