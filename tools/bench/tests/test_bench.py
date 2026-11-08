# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Offline regressions: evidence equivalence, accounting, reproducibility, bounds."""
import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import bench


class PairTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        (self.repo / "docs with spaces").mkdir(parents=True)
        (self.repo / "docs with spaces/café.md").write_bytes(
            "first\r\nGold café: 17. {{{file.id}}} {{attr_id}}\r\nlast\r\n".encode())
        (self.repo / "a.md").write_bytes(b"distractor\n")
        self.questions = {"schema_version": 1, "questions": [{
            "id": "question-1", "scenario": "windows", "question": "What value?",
            "evidence": [{"path": "docs with spaces/café.md", "start_line": 2,
                          "end_line": 2}, {"path": "a.md"}],
            "gold_facts": [{"id": "f1", "fact": "WITHHELD RUBRIC: seventeen"}]}]}

    def build(self, name="pairs", **kwargs):
        return bench.build_pairs(self.repo, self.questions, self.root / name,
                                 ["model-a", "model-b"], **kwargs)

    def test_evidence_bytes_equal_lengths_gold_withheld_and_template(self):
        result = self.build()
        pair = result["pairs"][0]
        a = (self.root / "pairs" / pair["dctx"]["file"]).read_bytes()
        b = (self.root / "pairs" / pair["plain"]["file"]).read_bytes()
        source = "Gold café: 17. {{{file.id}}} {{attr_id}}\r\n".encode()
        for prompt in (a, b):
            self.assertEqual(prompt.count(source), 1)
            self.assertNotIn(b"WITHHELD RUBRIC", prompt)
            self.assertNotIn(b"first\r\n", prompt)
        self.assertEqual(len(a), len(b))
        for arm in ("dctx", "plain"):
            self.assertEqual(pair[arm]["unpadded"]["utf8_bytes"]
                             + pair[arm]["padding_bytes"], len(a))
        self.assertEqual(pair["dctx"]["estimated_tokens"], pair["plain"]["estimated_tokens"])
        self.assertIn(b'<FILE_CONTENT_START ID="F003"', a)
        self.assertIn(b"[DIRECTORY:benchmark]\n<INSTRUCTIONS>\n", a)
        # Header instructions quote the id cleanly (df622da); no backslash escapes.
        self.assertIn(b'<FILE_CONTENT_START ID="UNIQUE_ID">', a)
        self.assertNotIn(b'ID=\\"UNIQUE_ID', a)
        self.assertNotIn(b"<INSTRUCTIONS>", b)
        self.assertEqual(pair["evidence"][1]["start_line"], 2)
        self.assertTrue(result["offline_only"])
        self.assertIn("NOT established", result["length_match"])
        self.assertEqual(json.loads((self.root / "pairs/gold.json").read_text()),
                         {"question-1": self.questions["questions"][0]["gold_facts"]})

    def test_repeatable_balanced_schedule_and_artifacts(self):
        one = self.build("one", seed=17)
        two = self.build("two", seed=17)
        self.assertEqual(one, two)
        self.assertEqual(len(one["schedule"]), 10)
        for model in one["models"]:
            trials = [t for t in one["schedule"] if t["model"] == model]
            self.assertEqual(sorted(t["run"] for t in trials), [1, 2, 3, 4, 5])
            self.assertLessEqual(abs(sum(t["arm_order"][0] == "dctx" for t in trials) - 2.5), 0.5)
        for path in (self.root / "one").iterdir():
            self.assertEqual(path.read_bytes(), (self.root / "two" / path.name).read_bytes())

    def test_preorder_same_for_both_arms(self):
        (self.repo / "a").mkdir()
        (self.repo / "a/z.md").write_text("inside directory", encoding="utf-8")
        self.questions["questions"][0]["evidence"] = [
            {"path": "a.md"}, {"path": "a/z.md"}]
        result = self.build()
        self.assertEqual([e["path"] for e in result["pairs"][0]["evidence"]],
                         ["a/z.md", "a.md"])
        for arm in ("plain", "dctx"):
            data = (self.root / "pairs" / result["pairs"][0][arm]["file"]).read_text()
            self.assertLess(data.index("inside directory"), data.index("distractor"))

    def test_no_overwrite_and_no_partial_output_on_bad_input(self):
        self.build()
        with self.assertRaises(FileExistsError):
            self.build()
        self.questions["questions"][0]["evidence"][0]["end_line"] = 999
        with self.assertRaises(ValueError):
            self.build("invalid")
        self.assertFalse((self.root / "invalid").exists())

    def test_path_escape_windows_paths_and_duplicate_files(self):
        for name in ("../outside.md", "/outside.md", "C:/outside.md", "docs\\bad.md"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                bench.source_slice(self.repo.resolve(), {"path": name}, 1000)
        self.questions["questions"][0]["evidence"] *= 2
        with self.assertRaises(ValueError):
            self.build()

    def test_symlink_escape(self):
        outside = self.root / "outside.md"
        outside.write_text("outside", encoding="utf-8")
        try:
            (self.repo / "link.md").symlink_to(outside)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable on this host")
        with self.assertRaises(ValueError):
            bench.source_slice(self.repo.resolve(), {"path": "link.md"}, 100)

    def test_binary_oversize_and_empty_sources(self):
        for data in (b"\xff", b"has\0nul"):
            (self.repo / "bad.md").write_bytes(data)
            with self.assertRaises(ValueError):
                bench.source_slice(self.repo.resolve(), {"path": "bad.md"}, 100)
        with self.assertRaises(ValueError):
            self.build(max_source_bytes=2)
        (self.repo / "empty.md").write_bytes(b"")
        value = bench.source_slice(self.repo.resolve(), {"path": "empty.md"}, 100)
        self.assertEqual((value["text"], value["start_line"], value["end_line"]), ("", 1, 0))

    def test_portable_ids_models_and_run_minimum(self):
        for identity in ("../bad", "a/b", "bad.", "CON", "lpt9"):
            with self.subTest(identity=identity):
                self.questions["questions"][0]["id"] = identity
                with self.assertRaises(ValueError):
                    self.build()
        self.questions["questions"][0]["id"] = "good"
        for models, runs in ((["same", "same"], 3), (["only"], 5), (["a", "b"], 2)):
            with self.assertRaises(ValueError):
                bench.build_pairs(self.repo, self.questions, self.root / "bad", models, runs)
        duplicate = copy.deepcopy(self.questions["questions"][0])
        duplicate["id"] = "GOOD"
        self.questions["questions"].append(duplicate)
        with self.assertRaises(ValueError):
            self.build()

    def test_offline_build_does_not_dispatch_models_or_network(self):
        with patch("socket.socket", side_effect=AssertionError("network used")), \
             patch("subprocess.Popen", side_effect=AssertionError("process used")):
            self.build()


class CostTests(unittest.TestCase):
    def projection(self, **kwargs):
        return {"schema_version": 1, "kind": "projection", "calls": 3,
                "startup_tokens": 50000, "context_tokens": 2000,
                "history_growth_tokens": 500, "output_tokens_per_call": 200,
                "cached_prefix_tokens": 50000, "cache_hit_share": 0.8, **kwargs}

    def test_projection_counts_startup_each_call_history_and_output(self):
        result = bench.cost_model(self.projection(), 0.9, 1, 4)
        self.assertEqual(result["input_tokens_including_cached"], 157500)
        self.assertEqual(result["cached_input_tokens"], 80000)
        self.assertEqual(result["total_billed_tokens"], 158100)
        self.assertEqual(result["effective_input_tokens_at_uncached_rate"], 85500)
        self.assertAlmostEqual(result["modeled_cost"], 0.0879)
        self.assertEqual(result["calls"][0]["cached_input_tokens"], 0)

    def test_raw_token_total_does_not_shrink_with_discount(self):
        no_cache = bench.cost_model(self.projection(), 0)
        full_discount = bench.cost_model(self.projection(), 1)
        self.assertEqual(no_cache["total_billed_tokens"], full_discount["total_billed_tokens"])
        self.assertIsNone(no_cache["modeled_cost"])
        self.assertEqual(bench.cost_model(self.projection(calls=1))["cached_input_tokens"], 0)

    def test_50k_startup_repaid_7_vs_23_model_calls(self):
        for calls in (7, 23):
            spec = self.projection(calls=calls, context_tokens=0,
                                   history_growth_tokens=0, output_tokens_per_call=0)
            self.assertEqual(bench.cost_model(spec)["total_billed_tokens"], calls * 50000)

    def test_usage_cache_share_is_weighted_and_input_includes_cached(self):
        spec = {"schema_version": 1, "kind": "usage", "calls": [
            {"input_tokens": 100, "cached_input_tokens": 50, "output_tokens": 10},
            {"input_tokens": 900, "cached_input_tokens": 0, "output_tokens": 20}]}
        result = bench.cost_model(spec, 0.5, 2, 3)
        self.assertEqual(result["prompt_cache_hit_share"], 0.05)
        self.assertEqual(result["total_billed_tokens"], 1030)
        self.assertEqual(result["effective_input_tokens_at_uncached_rate"], 975)
        self.assertAlmostEqual(result["modeled_cost"], 0.00204)

    def test_invalid_values_and_unknown_cache_fail(self):
        for override in ({"calls": -1}, {"startup_tokens": True},
                         {"calls": 2.0}, {"cache_hit_share": 1.1},
                         {"cached_prefix_tokens": 53000}, {"context_tokens": -1}):
            with self.subTest(override=override), self.assertRaises(ValueError):
                bench.cost_model(self.projection(**override))
        for discount in (-1, 2, float("nan"), float("inf")):
            with self.assertRaises(ValueError):
                bench.cost_model(self.projection(), discount)
        for cached in (None, 101, -1):
            with self.assertRaises(ValueError):
                bench.cost_model({"schema_version": 1, "kind": "usage", "calls": [
                    {"input_tokens": 100, "cached_input_tokens": cached, "output_tokens": 0}]})
        with self.assertRaises(ValueError):
            bench.cost_model(self.projection(), input_price=1)

    def test_zero_input_share_is_unknown_not_zero(self):
        result = bench.cost_model(self.projection(calls=1, startup_tokens=0,
                                                 context_tokens=0, cached_prefix_tokens=0))
        self.assertIsNone(result["prompt_cache_hit_share"])


class CliTests(unittest.TestCase):
    def test_build_cli_with_synthetic_repo(self):
        examples = Path(__file__).resolve().parents[1] / "examples"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "pairs"
            command = [sys.executable, str(Path(bench.__file__)), "build", "--repo",
                       str(examples / "corpus"), "--questions", str(examples / "questions.json"),
                       "--out", str(output), "--models", "model-a", "model-b", "--runs", "3"]
            result = subprocess.run(command, capture_output=True, text=True, close_fds=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(result.stdout)["paired_trials"], 12)
            self.assertEqual(len(list(output.glob("*.txt"))), 4)
            result = subprocess.run(command, capture_output=True, text=True, close_fds=True)
            self.assertEqual(result.returncode, 2)
            self.assertNotIn("Traceback", result.stderr)

    def test_cost_cli_and_invalid_input(self):
        examples = Path(__file__).resolve().parents[1] / "examples"
        command = [sys.executable, str(Path(bench.__file__)), "cost", "--spec",
                   str(examples / "usage.json")]
        result = subprocess.run(command, capture_output=True, text=True, close_fds=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["total_billed_tokens"], 104900)
        result = subprocess.run(command + ["--cached-discount", "nan"],
                                capture_output=True, text=True, close_fds=True)
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
