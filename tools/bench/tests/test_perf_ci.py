#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Regression policy, fail-closed counters and reproducible corpus checks."""
import contextlib
import copy
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("perf_ci", Path(__file__).parents[1] / "perf_ci.py")
perf = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(perf)


class PerformanceGuard(unittest.TestCase):
    def baseline(self):
        return {"schema_version": 1, "corpus_sha256": "fixed", "runner_sha256": "fixed",
                "platform": "Linux-x86_64",
                "rustc": "pinned", "rustflags": "-C target-cpu=x86-64", "policy": perf.POLICY,
                "cases": {case: {"wall_seconds_median": 0.01, "instructions_median": 1000}
                          for case in perf.CASES}}

    def compare(self, old, new, metric="instructions"):
        with contextlib.redirect_stdout(io.StringIO()):
            return perf.compare(old, new, metric)

    def test_boundaries_and_regression_fail_every_case(self):
        old = self.baseline()
        new = copy.deepcopy(old)
        for case in perf.CASES:
            new["cases"][case].update(instructions_median=1250, wall_seconds_median=0.16)
        self.assertEqual(self.compare(old, new), [])
        for case in perf.CASES:
            for field in ("instructions_median", "wall_seconds_median"):
                broken = copy.deepcopy(new)
                broken["cases"][case][field] += 1
                self.assertEqual(self.compare(old, broken), [f"{case}/{field}"])

    def test_missing_or_incomparable_measurements_fail_closed(self):
        old = self.baseline()
        for key in ("schema_version", "corpus_sha256", "runner_sha256", "platform", "rustc", "rustflags", "policy"):
            broken = copy.deepcopy(old)
            broken[key] = None
            with self.assertRaises(ValueError):
                self.compare(old, broken)
        broken = copy.deepcopy(old)
        del broken["cases"]["pack"]
        with self.assertRaises(ValueError):
            self.compare(old, broken)
        for value in (float("nan"), float("inf"), 0, -1, True, "100"):
            broken = copy.deepcopy(old)
            broken["cases"]["pack"]["instructions_median"] = value
            with self.assertRaises(ValueError):
                self.compare(old, broken)

    def test_explicit_wall_fallback_still_gates_wall(self):
        old = self.baseline()
        new = copy.deepcopy(old)
        for row in new["cases"].values():
            del row["instructions_median"]
        self.assertEqual(self.compare(old, new, "wall"), [])
        new["cases"]["pack"]["wall_seconds_median"] = 1
        self.assertEqual(self.compare(old, new, "wall"), ["pack/wall_seconds_median"])

    def test_corpus_content_and_stamps_are_reproducible(self):
        with tempfile.TemporaryDirectory() as temporary:
            a, b = Path(temporary) / "a", Path(temporary) / "b"
            self.assertEqual(perf.generate(a), perf.generate(b))
            files = list(a.rglob("*.rs"))
            self.assertEqual(len(files), 256)
            self.assertTrue(all(path.stat().st_mtime == 1_600_000_000 for path in files))
            self.assertGreater((a / "src00/module0000.rs").stat().st_size, 434_000)

    def test_counter_cannot_silently_use_another_event(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "counter"
            path.write_text("events: Ir\nsummary: 1234\n")
            self.assertEqual(perf.instruction_count(path), 1234)
            for bad in ("", "events: Dr\nsummary: 1234\n", "events: Ir\nsummary: 0\n"):
                path.write_text(bad)
                with self.assertRaises(ValueError):
                    perf.instruction_count(path)

    def test_output_requires_real_results_and_warm_reuse(self):
        for case in perf.CASES:
            with self.assertRaises(ValueError):
                perf.validate_output(case, b"{}", b"")
        with self.assertRaises(ValueError):
            perf.validate_output("warm_read", b'{"records":["perf_target"]}', b"")
        page = b'{"records":["perf_target", "wrapping_add(7)"]}'
        perf.validate_output("warm_read", page, b"generation built\ncontent_reads 1\n")
        with self.assertRaises(ValueError):
            perf.validate_output("warm_read", page, b"content_reads 256\n")
        with self.assertRaises(ValueError):
            perf.validate_output("warm_find", page, b"generation built\n")

    def test_check_exits_one_and_preserves_baseline_on_regression(self):
        import json
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = root / "baseline.json"
            perf.write_json(baseline, self.baseline())
            original = baseline.read_bytes()
            binary = root / "binary"
            binary.touch()
            report = copy.deepcopy(self.baseline())
            report["cases"]["warm_read"]["wall_seconds_median"] = 1
            argv = ["perf_ci.py", "check", "--baseline", str(baseline),
                    "--report", str(root / "report.json"), "--scratch", str(root / "scratch"),
                    "--dctx", str(binary), "--catalog", str(binary), "--metric", "wall"]
            with patch.object(perf, "ROOT", root), patch.object(perf, "measure", return_value=report), \
                    patch("sys.argv", argv), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(perf.main(), 1)
            self.assertEqual(baseline.read_bytes(), original)
            self.assertEqual(json.loads((root / "report.json").read_text())["failures"],
                             ["warm_read/wall_seconds_median"])

    def test_failed_measurement_cannot_publish_a_stale_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = root / "baseline.json"
            perf.write_json(baseline, self.baseline())
            original = baseline.read_bytes()
            report = root / "report.json"
            report.write_text('{"stale": true}')
            binary = root / "binary"
            binary.touch()
            argv = ["perf_ci.py", "check", "--baseline", str(baseline),
                    "--report", str(report), "--scratch", str(root / "scratch"),
                    "--dctx", str(binary), "--catalog", str(binary), "--metric", "wall"]
            with patch.object(perf, "ROOT", root), \
                    patch.object(perf, "measure", side_effect=ValueError("measurement failed")), \
                    patch("sys.argv", argv), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(perf.main(), 2)
            self.assertFalse(report.exists())
            self.assertEqual(baseline.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
