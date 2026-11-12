# SPDX-License-Identifier: Apache-2.0
"""Latency accounting and errors without clock-based assertions."""
import importlib.util
import argparse
from collections import Counter
import json
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


class FixtureTests(unittest.TestCase):
    def test_default_keeps_one_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            place = Path(tmp) / "fixture"
            shape = latency.generate_fixture(place)
            self.assertEqual(shape, {"files": 1, "dirs": 1, "depth": 0})
            self.assertEqual([p.name for p in place.iterdir()], ["lib.rs"])
            self.assertEqual((place / "lib.rs").read_text(), latency.FIXTURE)

    def test_deterministic_paths_and_contents(self):
        with tempfile.TemporaryDirectory() as tmp:
            a, b = Path(tmp) / "a", Path(tmp) / "b"
            self.assertEqual(latency.generate_fixture(a, 41), latency.generate_fixture(b, 41))
            expected = sorted(
                f"level_0_0/level_1_0/level_2_0/group_{index // 20:06d}/"
                f"source_{index:06d}.{('rs', 'py', 'md', 'json')[index % 4]}"
                for index in range(41)
            )
            paths = sorted(p.relative_to(a).as_posix() for p in a.rglob("*") if p.is_file())
            self.assertEqual(paths, expected)
            self.assertEqual(paths, sorted(p.relative_to(b).as_posix()
                                           for p in b.rglob("*") if p.is_file()))
            for path in paths:
                self.assertEqual((a / path).read_bytes(), (b / path).read_bytes())
            self.assertEqual((a / paths[0]).read_text(), latency.FIXTURE)

    def test_repo_scale_shape_mix_and_settled_mtimes(self):
        now_ns = 1_800_000_000_000_000_000
        for count in (0, 1, 1551):
            with self.subTest(count=count), tempfile.TemporaryDirectory() as tmp, patch.object(
                latency.time, "time_ns", return_value=now_ns
            ):
                place = Path(tmp) / "fixture"
                shape = latency.generate_fixture(place, count)
                files = [p for p in place.rglob("*") if p.is_file()]
                dirs = [place, *(p for p in place.rglob("*") if p.is_dir())]
                self.assertEqual(shape, {"files": len(files), "dirs": len(dirs),
                                         "depth": max(len(p.relative_to(place).parent.parts)
                                                      for p in files)})
                self.assertEqual(len(files), count or 1)
                self.assertTrue(all(p.stat().st_mtime_ns <= now_ns - 5_000_000_000
                                    for p in files))
                if count == 1551:
                    self.assertEqual(shape, {"files": 1551, "dirs": 106, "depth": 4})
                    self.assertEqual(Counter(p.suffix for p in files),
                                     {".rs": 388, ".py": 388, ".md": 388, ".json": 387})
                    self.assertEqual(Counter(Counter(p.parent for p in files).values()),
                                     {20: 77, 11: 1})
                    self.assertTrue(all(len(p.relative_to(place).parent.parts) == 4
                                        for p in files))

    def test_negative_scale_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            place = Path(tmp) / "fixture"
            with self.assertRaises(ValueError):
                latency.generate_fixture(place, -1)
            self.assertFalse(place.exists())


class LatencyTests(unittest.TestCase):
    def test_failed_run_removes_previous_report(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / "report.json"
            report.write_text('{"calls": 50}')
            args = argparse.Namespace(binary=Path(tmp) / "missing-binary", output=report,
                                      scale_files=0)
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
            self.assertEqual(report["fixture_shape"], {"files": 1, "dirs": 1, "depth": 0})
            self.assertEqual(run.call_args.args[0][1:4], ["map", "find", "latency_probe"])
            self.assertEqual(run.call_args.kwargs["env"]["DCTX_AUTO_CLEAN"], "0")

    def test_cli_scale_shape_and_phases_are_recorded(self):
        phases = {"config_and_root": 100, "generation_open": 200, "decode": 30,
                  "query": 4, "render": 5, "write": 6}
        result = subprocess.CompletedProcess(
            [], 0, stdout=b"latency_probe",
            stderr=json.dumps({"dctx_timings": phases}).encode(),
        )
        with tempfile.TemporaryDirectory() as tmp, patch.dict(
            latency.os.environ, {"DCTX_TIMINGS": "1"}
        ), patch.object(latency.subprocess, "run", return_value=result) as run:
            root = Path(tmp)
            binary = root / "dctx"
            binary.touch()
            output = root / "scale.json"
            with patch("sys.argv", ["map_find_latency.py", "--binary", str(binary),
                                    "--scale-files", "41", "--output", str(output)]):
                latency.main()
            report = json.loads(output.read_text())
            self.assertEqual(report["fixture_shape"], {"files": 41, "dirs": 7, "depth": 4})
            self.assertEqual(report["phase_medians_us"], phases)
            self.assertEqual(report["warmup_calls"], 1)
            self.assertEqual(run.call_count, 51)

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
