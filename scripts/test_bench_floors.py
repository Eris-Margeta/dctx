# SPDX-License-Identifier: Apache-2.0
"""Exercise the benchmark floors CLI with committed inputs and mutated copies."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import bench_floors as checker


MISSING = object()
NETWORK = ("network", 0, "inet_attempt_count")
CHANGES = ("determinism", "flask", "dctx", "changed_after_edit")
COLD_WALL = ("index", "flask", "dctx", "cold_wall_s_median")
WARM_FIND = ("query", "flask", "dctx", "find")
INVALID_TIMINGS = (None, True, "0.1", -0.1, float("nan"), float("inf"), -float("inf"))


class BenchFloorsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.aggregate = Path(checker.DEFAULT_AGG).read_text(encoding="utf-8")
        cls.tasks = Path(checker.DEFAULT_TASKS).read_text(encoding="utf-8")

    def cli(self, *args):
        result = subprocess.run(
            [sys.executable, "-B", checker.__file__, *args],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.stderr, "", result.stderr)
        return result

    def run_changed(self, path, value):
        aggregate = json.loads(self.aggregate)
        row = aggregate
        for key in path[:-1]:
            row = row[key]
        if value is MISSING:
            del row[path[-1]]
        else:
            row[path[-1]] = value
        with tempfile.TemporaryDirectory(dir=checker.ROOT) as directory:
            aggregate_path = Path(directory) / "aggregate.json"
            tasks_path = Path(directory) / "tasks_aggregate.json"
            aggregate_path.write_text(json.dumps(aggregate), encoding="utf-8")
            tasks_path.write_text(self.tasks, encoding="utf-8")
            return self.cli(aggregate_path, tasks_path)

    def assert_passed(self, result):
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(result.stdout.splitlines()[-1], "BENCH-FLOORS PASS")

    def assert_failed(self, result, floor, detail):
        self.assertEqual(result.returncode, 1, result.stdout)
        lines = result.stdout.splitlines()
        failures = [line for line in lines if line.startswith("FAIL ")]
        self.assertEqual(len(failures), 1, result.stdout)
        self.assertTrue(failures[0].startswith(f"FAIL {floor}"), failures[0])
        self.assertIn(detail, failures[0])
        self.assertEqual(lines[-1], "BENCH-FLOORS FAIL (1 failed)")

    def test_real_default_inputs_pass(self):
        result = self.cli()
        self.assert_passed(result)
        self.assertIn("PASS network attempts", result.stdout)

    def test_filenames_are_escaped_in_header_and_read_errors(self):
        suffix = "\nforged\r\x1b\u2028.json"
        with tempfile.TemporaryDirectory(dir=checker.ROOT) as directory:
            aggregate = Path(directory) / ("aggregate" + suffix)
            tasks = Path(directory) / ("tasks" + suffix)
            aggregate.write_text(self.aggregate, encoding="utf-8")
            tasks.write_text(self.tasks, encoding="utf-8")
            result = self.cli(aggregate, tasks)
            self.assert_passed(result)
            header = (f"bench floors: {str(aggregate.relative_to(checker.ROOT))!a} + "
                      f"{str(tasks.relative_to(checker.ROOT))!a}")
            self.assertEqual(result.stdout.splitlines()[0], header)

            tasks.unlink()
            result = self.cli(aggregate, tasks)
            self.assertEqual(result.returncode, 2)
            self.assertEqual(len(result.stdout.splitlines()), 1)
            self.assertIn(f"cannot read {str(tasks)!a}:", result.stdout)

    def test_invalid_metric_payloads_are_omitted(self):
        payload = "fixture-marker\nPASS forged\r\x1b\u2028\ud800"
        cases = (
            (("index", "flask", "dctx", "ok"), payload, "cold index ok flask", "expected a boolean"),
            (("index", "flask", "dctx", "peak_rss_mb_largest_process"), payload,
             "cold index RSS flask", "expected a number"),
            (NETWORK, payload, "network attempts", "inet_attempt_count must be a nonnegative integer"),
            (CHANGES, [payload, {}], "unrelated edit changes only hubs flask",
             "changed_after_edit must be a list of query kind strings"),
            (COLD_WALL, payload, "cold index wall flask", "cold_wall_s_median must be a finite nonnegative number"),
            ((*WARM_FIND, "walls", 0), {"value": payload}, "warm query p50 flask",
             "'find'.walls[0] must be a finite nonnegative number"),
            ((*WARM_FIND, "wall_s_median"), payload, "warm query p50 flask",
             "'find'.wall_s_median must be a finite nonnegative number"),
        )
        for path, value, floor, detail in cases:
            with self.subTest(path=path):
                result = self.run_changed(path, value)
                self.assert_failed(result, floor, detail)
                self.assertNotIn("fixture-marker", result.stdout)
                self.assertNotIn("forged", result.stdout)

    def test_explicit_empty_changes_pass(self):
        result = self.run_changed(CHANGES, [])
        self.assert_passed(result)
        self.assertIn("PASS unrelated edit changes only hubs flask", result.stdout)

    def test_invalid_attempt_counts_fail(self):
        for value in (MISSING, None, False, True, -1, 0.0, "0", []):
            with self.subTest(value=value):
                result = self.run_changed(NETWORK, value)
                self.assert_failed(result, "network attempts", "network[0].inet_attempt_count")

    def test_nonzero_attempt_count_fails(self):
        result = self.run_changed(NETWORK, 1)
        self.assert_failed(result, "network attempts", "value 1 floor == 0")

    def test_missing_or_malformed_network_evidence_fails(self):
        for value, detail in ((MISSING, "network must be a list"), ({}, "network must be a list"),
                              ([], "no traced dctx steps")):
            with self.subTest(value=value):
                self.assert_failed(self.run_changed(("network",), value), "network attempts", detail)

    def test_invalid_change_lists_fail(self):
        for value in (MISSING, None, False, "", {}, [False], [None], [[]]):
            with self.subTest(value=value):
                result = self.run_changed(CHANGES, value)
                self.assert_failed(result, "unrelated edit changes only hubs flask", "changed_after_edit")

    def test_change_to_non_hubs_query_fails(self):
        result = self.run_changed(CHANGES, ["overview", "find"])
        self.assert_failed(result, "unrelated edit changes only hubs flask", "value 1 floor == 0")

    def test_invalid_cold_wall_times_fail(self):
        for value in (MISSING, *INVALID_TIMINGS):
            with self.subTest(value=value):
                result = self.run_changed(COLD_WALL, value)
                self.assert_failed(result, "cold index wall flask", "cold_wall_s_median")

    def test_missing_or_malformed_warm_wall_lists_fail(self):
        for value in (MISSING, None, False, "", "0.1", [], {}, 0.1):
            with self.subTest(value=value):
                result = self.run_changed((*WARM_FIND, "walls"), value)
                self.assert_failed(result, "warm query p50 flask", "'find'.walls must be a nonempty list")

    def test_invalid_warm_wall_sample_fails_even_with_valid_peers(self):
        for value in (*INVALID_TIMINGS, [], {}):
            with self.subTest(value=value):
                result = self.run_changed((*WARM_FIND, "walls", 0), value)
                self.assert_failed(result, "warm query p50 flask", "'find'.walls[0]")

    def test_invalid_warm_kind_medians_fail(self):
        for value in INVALID_TIMINGS:
            with self.subTest(value=value):
                result = self.run_changed((*WARM_FIND, "wall_s_median"), value)
                self.assert_failed(result, "warm query p50 flask", "'find'.wall_s_median")

    def test_query_names_are_escaped_in_timing_output(self):
        kind = "find\nforged\r\x1b\u2028\ud800"
        shown = r"'find\nforged\r\x1b\u2028\ud800'"
        query = {"supported": True, "walls": [0.1], "wall_s_median": 0.1, "identical_across_runs": True}
        for overrides, detail in (({"walls": []}, ".walls must be a nonempty list"),
                                  ({"walls": [-1]}, ".walls[0]"),
                                  ({"wall_s_median": -1}, ".wall_s_median")):
            with self.subTest(overrides=overrides):
                result = self.run_changed(WARM_FIND[:-1], {kind: {**query, **overrides}})
                self.assert_failed(result, "warm query p50 flask", shown + detail)
        result = self.run_changed(WARM_FIND[:-1], {kind: query})
        self.assert_passed(result)
        self.assertIn(f"INFO slowest query kind flask: {shown} median 0.1 s", result.stdout.splitlines())

    def test_unsupported_queries_do_not_supply_timing_samples(self):
        result = self.run_changed(("query", "mixed", "dctx", "callers1", "walls"), [-1])
        self.assert_passed(result)
        self.assertIn("PASS warm query p50 mixed (s, 20 runs)", result.stdout)


if __name__ == "__main__":
    unittest.main()
