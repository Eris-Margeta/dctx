# SPDX-License-Identifier: Apache-2.0
"""Exercise the benchmark floors CLI with committed inputs and mutated copies."""

import json
import math
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
RSS = ("index", "flask", "dctx", "peak_rss_mb_largest_process")
WARM_FIND = ("query", "flask", "dctx", "find")
GOLD = ("accuracy", "ALL_GOLD_CORPORA", "dctx")
RATIOS = (
    ("precision_micro", "callers precision page 1", 0.99),
    ("precision_micro_allpages", "callers precision 5 pages", 0.99),
    ("recall_page1_micro", "callers recall page 1", 0.85),
    ("recall_allpages_micro", "callers recall 5 pages", 0.95),
)
OVERALL = ("overall", "dctx")
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
        self.assertEqual(result.stderr, "")
        return result

    def run_changed(self, path, value, *, tasks=False):
        document = json.loads(self.tasks if tasks else self.aggregate)
        row = document
        for key in path[:-1]:
            row = row[key]
        if value is MISSING:
            del row[path[-1]]
        else:
            row[path[-1]] = value
        with tempfile.TemporaryDirectory(dir=checker.ROOT) as directory:
            aggregate_path = Path(directory) / "aggregate.json"
            tasks_path = Path(directory) / "tasks_aggregate.json"
            aggregate_path.write_text(self.aggregate if tasks else json.dumps(document), encoding="utf-8")
            tasks_path.write_text(json.dumps(document) if tasks else self.tasks, encoding="utf-8")
            return self.cli(aggregate_path, tasks_path)

    def assert_passed(self, result):
        self.assertEqual(result.returncode, 0, ascii(result.stdout))
        self.assertEqual(result.stdout.splitlines()[-1], "BENCH-FLOORS PASS")

    def assert_failed(self, result, floor, detail):
        self.assert_failures(result, [(floor, detail)])

    def assert_failures(self, result, expected):
        self.assertEqual(result.returncode, 1, ascii(result.stdout))
        lines = result.stdout.splitlines()
        failures = [line for line in lines if line.startswith("FAIL ")]
        self.assertEqual(len(failures), len(expected), ascii(result.stdout))
        for line, (floor, detail) in zip(failures, expected):
            self.assertTrue(line.startswith(f"FAIL {floor}"), ascii(line))
            self.assertIn(detail, line)
        self.assertEqual(lines[-1], f"BENCH-FLOORS FAIL ({len(expected)} failed)")

    def test_real_default_inputs_pass(self):
        result = self.cli()
        self.assert_passed(result)
        self.assertIn("PASS network attempts", result.stdout)

    def test_accuracy_domains_and_floors(self):
        for field, floor, limit in RATIOS:
            path = (*GOLD, field)
            for value in (MISSING, True, False, "1", [], {}, -0.1, 1.1, math.nan, math.inf, -math.inf):
                with self.subTest(field=field, invalid=value):
                    self.assert_failed(self.run_changed(path, value), floor, f"{field} must be a finite number in [0, 1]")
            for value in (0, math.nextafter(limit, 0)):
                with self.subTest(field=field, below_floor=value):
                    self.assert_failed(self.run_changed(path, value), floor, f"value {value} floor >= {limit}")
            for value in (limit, 1):
                with self.subTest(field=field, passing=value):
                    self.assert_passed(self.run_changed(path, value))

    def test_rss_domain_and_limit(self):
        floor = "cold index RSS flask"
        for value in (MISSING, *INVALID_TIMINGS, [], {}):
            with self.subTest(invalid=value):
                self.assert_failed(self.run_changed(RSS, value), floor, "peak_rss_mb_largest_process must be a finite nonnegative number")
        for value in (0, 100):
            with self.subTest(passing=value):
                self.assert_passed(self.run_changed(RSS, value))
        above = math.nextafter(100.0, math.inf)
        self.assert_failed(self.run_changed(RSS, above), floor, f"value {above} floor <= 100.0")

    def test_score_domain_and_floor(self):
        path = (*OVERALL, "score")
        floor = "October protocol score"
        for value in (MISSING, True, False, "140", 138.5, 140.0, [], {}, -1, 141, math.nan, math.inf, -math.inf):
            with self.subTest(invalid=value):
                self.assert_failed(self.run_changed(path, value, tasks=True), floor, "score must be an integer between 0 and 140")
        for value in (0, 137):
            with self.subTest(below_floor=value):
                self.assert_failed(self.run_changed(path, value, tasks=True), floor, f"value {value} floor >= 138")
        for value in (138, 140):
            with self.subTest(passing=value):
                self.assert_passed(self.run_changed(path, value, tasks=True))

    def test_score_requires_valid_protocol_maximum(self):
        path = (*OVERALL, "max_score")
        floor = "October protocol max score"
        for value in (MISSING, True, False, 140.0, "140", [], {}, math.nan, math.inf, -math.inf):
            with self.subTest(invalid=value):
                self.assert_failed(self.run_changed(path, value, tasks=True), floor, "max_score must be an integer")
        for value in (139, 141):
            with self.subTest(wrong_max=value):
                self.assert_failed(self.run_changed(path, value, tasks=True), floor, f"value {value} floor == 140")

    def test_malformed_metric_containers_fail(self):
        payload = "fixture-marker'\n\x1b"
        for value in ([payload], payload, True):
            with self.subTest(value=value):
                result = self.run_changed(GOLD, value)
                self.assert_failures(result, [(floor, field) for field, floor, _ in RATIOS])
                self.assertNotIn("fixture-marker", result.stdout)
                result = self.run_changed(OVERALL, value, tasks=True)
                self.assert_failed(result, "October protocol max score", "max_score must be an integer")
                self.assertNotIn("fixture-marker", result.stdout)
                self.assert_failed(self.run_changed(WARM_FIND, value), "warm query p50 flask", "'find' query metrics must be an object")

    def test_warm_median_uses_unrounded_limit(self):
        # A majority of flask samples come from this single mutated walls field.
        path = (*WARM_FIND, "walls")
        at_limit = self.run_changed(path, [0.300] * 31)
        self.assert_passed(at_limit)
        above_limit = self.run_changed(path, [math.nextafter(0.300, math.inf)] * 31)
        self.assert_failed(above_limit, "warm query p50 flask", "value 0.3 floor <= 0.3")

    def test_filenames_are_escaped_in_header_and_read_errors(self):
        suffix = "'\nforged\r\x1b\u2028.json"
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
        payload = "fixture-marker'\nPASS forged\r\x1b\u2028\ud800"
        cases = (
            (("index", "flask", "dctx", "ok"), payload, "cold index ok flask", "expected a boolean"),
            (RSS, payload, "cold index RSS flask", "peak_rss_mb_largest_process must be a finite nonnegative number"),
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
        kind = "find'\nforged\r\x1b\u2028\ud800"
        shown = ascii(kind)
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
