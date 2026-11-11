# SPDX-License-Identifier: Apache-2.0
"""Latency accounting and errors without clock-based assertions."""
import importlib.util
import argparse
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "map_find_latency", Path(__file__).resolve().parents[1] / "map_find_latency.py"
)
latency = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(latency)


class LatencyTests(unittest.TestCase):
    def test_failed_run_removes_previous_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / "report.json"
            report.write_text('{"calls": 50}')
            args = argparse.Namespace(binary=Path(tmp) / "missing-binary", output=report)
            with patch.object(latency.argparse.ArgumentParser, "parse_args", return_value=args):
                with self.assertRaises(FileNotFoundError):
                    latency.main()
            self.assertFalse(report.exists())

    def test_nearest_rank_percentiles(self):
        samples = list(reversed(range(1, 51)))
        self.assertEqual(latency.percentile(samples, 0.50), 25)
        self.assertEqual(latency.percentile(samples, 0.95), 48)

    def test_fifty_processes_after_one_unmeasured_warmup(self):
        # The fake clock advances exactly 1..50 ms for the measured calls.
        ticks = [tick for n in range(1, 51) for tick in (0, n * 1_000_000)]
        result = subprocess.CompletedProcess([], 0, stdout=b"latency_probe", stderr=b"")
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            latency.subprocess, "run", return_value=result
        ) as run, patch.object(latency.time, "perf_counter_ns", side_effect=ticks):
            report = latency.measure(Path("dctx.exe"), Path(tmp))
            self.assertEqual(run.call_count, 51)
            self.assertEqual(report["samples_ms"], list(range(1, 51)))
            self.assertEqual(report["p50_ms"], 25)
            self.assertEqual(report["p95_ms"], 48)
            self.assertEqual(report["calls"], 50)
            self.assertEqual(run.call_args.args[0][1:4], ["map", "find", "latency_probe"])
            self.assertEqual(run.call_args.kwargs["env"]["DCTX_AUTO_CLEAN"], "0")

    def test_missing_symbol_and_failed_process_are_errors(self):
        for result in [
            subprocess.CompletedProcess([], 0, stdout=b"no match", stderr=b""),
            subprocess.CalledProcessError(3, ["dctx"]),
        ]:
            with self.subTest(result=result), tempfile.TemporaryDirectory() as tmp:
                kwargs = {"side_effect": result} if isinstance(result, Exception) else {"return_value": result}
                with patch.object(latency.subprocess, "run", **kwargs):
                    with self.assertRaises((RuntimeError, subprocess.CalledProcessError)):
                        latency.measure(Path("dctx.exe"), Path(tmp))


if __name__ == "__main__":
    unittest.main()
