# SPDX-License-Identifier: Apache-2.0
from contextlib import redirect_stdout
from io import StringIO
import json
from pathlib import Path
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
        cold = next(iters[Path(binary).name])
        return {"cold": {"p95_ms": cold, "p50_ms": cold / 2}, "warm": {"p95_ms": 20.0}}

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


class MainTests(unittest.TestCase):
    def test_main_writes_report_summary_and_exit_code(self):
        for cand, status in (([450] * 5, 0), ([700] * 5, 1)):
            with self.subTest(cand=cand), tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
                output = Path(tmp) / "ab.json"
                summary = Path(tmp) / "summary.md"
                measure, _ = fake_measure({"base": [450] * 5, "cand": cand})
                out = StringIO()
                with patch.dict(ab.os.environ, {"GITHUB_STEP_SUMMARY": str(summary), "GITHUB_ACTIONS": "true"}), \
                        patch.object(ab.shutil, "which", side_effect=lambda p: p), \
                        patch.object(ab.latency, "measure", side_effect=measure), redirect_stdout(out):
                    code = ab.main(["--baseline", "base", "--candidate", "cand", "--rounds", "5",
                                    "--files", "1", "--output", str(output)])
                self.assertEqual(code, status)
                report = json.loads(output.read_text())
                self.assertEqual(report["gate"]["verdict"], "PASS" if status == 0 else "FAIL")
                self.assertIn("median", summary.read_text())
                if status:
                    self.assertIn("::warning", out.getvalue())


if __name__ == "__main__":
    unittest.main()
