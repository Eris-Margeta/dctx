# SPDX-License-Identifier: Apache-2.0
"""Exercise both input positions of the instruction-mode sample gate."""
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

BENCH = Path(__file__).resolve().parents[1]
with patch.object(sys, "path", [str(BENCH), *sys.path]):
    import perf_ci
    import verify_perf_samples


class PerformanceSamples(unittest.TestCase):
    def setUp(self):
        self.baseline = perf_ci.BASELINE
        self.report = json.loads(self.baseline.read_text(encoding="utf-8"))
        self.row = self.report["cases"]["warm_read"]
        directory = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(directory.cleanup)
        self.fixture = Path(directory.name) / "report.json"

    def assert_rejected_in_either_input(self, reason):
        self.fixture.write_text(json.dumps(self.report), encoding="utf-8")
        for role, paths in (("baseline", [self.fixture, self.baseline]),
                            ("candidate", [self.baseline, self.fixture])):
            with self.subTest(role=role), contextlib.redirect_stderr(io.StringIO()) as errors:
                self.assertNotEqual(verify_perf_samples.main(paths), 0)
                self.assertIn(role + " ", errors.getvalue())
                self.assertIn(reason, errors.getvalue())

    def test_committed_baseline(self):
        self.assertEqual(verify_perf_samples.main([self.baseline, self.baseline]), 0)

    def test_empty_instructions_with_passing_summary(self):
        self.row["instructions"] = []
        self.report["failures"] = []
        original = json.loads(self.baseline.read_text(encoding="utf-8"))
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(perf_ci.compare(original, self.report, "instructions"), [])
        self.assert_rejected_in_either_input("warm_read.instructions")

    def test_independent_sample_counts_and_missing_vectors(self):
        for metric in ("wall_seconds", "instructions"):
            original = self.row.pop(metric)
            with self.subTest(metric=metric, missing=True):
                self.assert_rejected_in_either_input(metric)
            for samples in (original[:-1], original + original[:1], "x" * len(original)):
                with self.subTest(metric=metric, samples=samples):
                    self.row[metric] = samples
                    self.assert_rejected_in_either_input(metric)
            self.row[metric] = original

    def test_missing_case(self):
        del self.report["cases"]["pack"]
        self.assert_rejected_in_either_input("cases")

    def test_missing_or_inconsistent_medians(self):
        for field in ("wall_seconds_median", "instructions_median"):
            original = self.row.pop(field)
            with self.subTest(field=field, missing=True):
                self.assert_rejected_in_either_input(field)
            with self.subTest(field=field, inconsistent=True):
                self.row[field] = original / 2 if field == "wall_seconds_median" else original - 1
                self.assert_rejected_in_either_input(field)
            self.row[field] = original

    def test_invalid_samples_even_with_a_matching_median(self):
        invalid = (0, -1, True, None, "1", float("nan"), float("inf"), -float("inf"))
        for metric in ("wall_seconds", "instructions"):
            original = self.row[metric]
            for value in invalid + ((1.5,) if metric == "instructions" else ()):
                with self.subTest(metric=metric, value=value):
                    self.row[metric] = [self.row[metric + "_median"]] * (len(original) - 1) + [value]
                    self.assert_rejected_in_either_input(f"invalid sample at warm_read.{metric}")
            self.row[metric] = original

    def test_boolean_median_cannot_stand_for_one(self):
        for metric in ("wall_seconds", "instructions"):
            with self.subTest(metric=metric), patch.dict(self.row, {
                metric: [1] * len(self.row[metric]), metric + "_median": True
            }):
                self.assert_rejected_in_either_input(metric + "_median")

    def test_error_escapes_hostile_path(self):
        hostile = self.fixture.with_name('bad\n\x1b".json')
        hostile.write_text("{", encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertNotEqual(verify_perf_samples.main([self.baseline, hostile]), 0)
        diagnostic = errors.getvalue()
        self.assertEqual(len(diagnostic.splitlines()), 1)
        self.assertNotIn("\x1b", diagnostic)
        self.assertIn(json.dumps(str(hostile)), diagnostic)
