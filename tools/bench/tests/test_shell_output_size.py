# SPDX-License-Identifier: Apache-2.0
"""Output-size accounting, command coverage and report failures; no timing gates."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

BENCH = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("shell_output_size", BENCH / "shell_output_size.py")
sizes = importlib.util.module_from_spec(SPEC)
with patch.object(sys, "path", [str(BENCH), *sys.path]):
    SPEC.loader.exec_module(sizes)


class ShellOutputSizeTests(unittest.TestCase):
    def test_fixed_terms_are_real_fixture_symbols(self):
        expected = (
            "latency_probe", "helper", "scale_item_000001", "scale_item_000004",
            "scale_item_000005", "scale_item_000100", "scale_item_000401",
            "scale_item_001000", "scale_item_001548", "scale_item_001549",
        )
        self.assertEqual(sizes.TERMS, expected)
        with tempfile.TemporaryDirectory() as tmp:
            place = Path(tmp) / "fixture"
            self.assertEqual(sizes.generate_fixture(place, 1551),
                             {"files": 1551, "dirs": 106, "depth": 4})
            first = sizes.targets(place)
            self.assertEqual(first, sizes.targets(place))
            for term, (file, node) in first.items():
                self.assertIn(term, (place / file).read_text())
                self.assertTrue(file.endswith((".rs", ".py")))
                self.assertEqual(node, f"s:{file}#{term}")

    def test_cli_defaults_run_all_commands_and_terms(self):
        completed = subprocess.CompletedProcess([], 0, stdout=b"answer\n", stderr=b"")
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            sizes.subprocess, "run", return_value=completed
        ) as run, patch.dict(sizes.os.environ, {"DCTX_MAX_RECORDS": "1"}):
            report = sizes.measure(Path("dctx"), Path(tmp))
            self.assertEqual(run.call_count, 130)
            self.assertEqual(len(report["samples"]), 130)
            self.assertEqual({row["command"] for row in report["samples"]}, {
                "map", "find", "neighbors", "explain", "read", "hubs", "grep",
                "skeleton", "path", "trace", "impact", "export", "pack",
            })
            for row, call in zip(report["samples"], run.call_args_list):
                self.assertEqual(call.args[0], ["dctx", *row["arguments"]])
                self.assertTrue(call.kwargs["check"])
                self.assertTrue(call.kwargs["capture_output"])
                self.assertNotIn("DCTX_MAX_RECORDS", call.kwargs["env"])
                self.assertEqual(call.kwargs["cwd"], Path(tmp) / "fixture")
                self.assertEqual(call.kwargs["env"]["DCTX_CACHE_DIR"], str(Path(tmp) / "cache"))
    def test_json_shape_and_byte_threshold_include_unicode_and_final_line(self):
        # 30,000 bytes is not over the threshold; 30,001 bytes is. Multibyte
        # text must be counted as bytes, and an unterminated last line counts.
        payloads = [b"x" * 30_000, b"x" * 30_001, "é\nlast".encode()]
        counter = iter(range(130))

        def query(*args, **kwargs):
            return subprocess.CompletedProcess([], 0, stdout=payloads[next(counter) % 3], stderr=b"")

        with tempfile.TemporaryDirectory() as tmp, patch.object(
            sizes.subprocess, "run", side_effect=query
        ):
            report = json.loads(json.dumps(sizes.measure(Path("dctx"), Path(tmp))))
        self.assertEqual(set(report), {
            "schema", "fixture_shape", "terms", "commands", "modes", "threshold_bytes",
            "calls", "encoding", "median_method", "percentile_method", "samples", "summary",
        })
        self.assertEqual(report["schema"], 1)
        self.assertEqual(report["threshold_bytes"], 30_000)
        self.assertEqual(report["calls"], 130)
        self.assertEqual(report["modes"], ["cli_defaults"])
        self.assertEqual(len(report["terms"]), 10)
        self.assertEqual(len(report["summary"]), 13)
        self.assertEqual([r["stdout_bytes"] for r in report["samples"][:3]], [30000, 30001, 7])
        self.assertEqual([r["stdout_lines"] for r in report["samples"][:3]], [1, 1, 2])
        self.assertEqual([r["over_30000"] for r in report["samples"][:3]], [False, True, False])
        for row in report["samples"]:
            self.assertEqual(set(row), {"command", "mode", "term", "arguments", "stdout_bytes",
                                       "stdout_lines", "over_30000"})

    def test_summary_median_p95_max_and_counts(self):
        samples = [{"command": command, "mode": mode, "stdout_bytes": n * 5000,
                    "over_30000": n > 6}
                   for command in sizes.COMMANDS for mode in sizes.MODES for n in range(1, 11)]
        for row in sizes.summarize(samples):
            self.assertEqual(row["calls"], 10)
            self.assertEqual(row["median_bytes"], 27500)
            self.assertEqual(row["p95_bytes"], 50000)
            self.assertEqual(row["max_bytes"], 50000)
            self.assertEqual(row["over_30000"], 4)
        summary = sizes.summarize(samples)
        summary[0]["median_bytes"] = 27500.5
        self.assertIn("| 27,500.5 | 50,000 | 50,000 | 4 |", sizes.table({"summary": summary}))

    def test_main_writes_json_and_prints_its_table(self):
        completed = subprocess.CompletedProcess([], 0, stdout=b"ok\n", stderr=b"")
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            sizes.subprocess, "run", return_value=completed
        ), contextlib.redirect_stdout(io.StringIO()) as stdout:
            root = Path(tmp)
            binary = root / "dctx"
            binary.touch()
            output = root / "nested" / "sizes.json"
            with patch.object(sys, "argv", ["shell_output_size.py", "--binary", str(binary),
                                           "--output", str(output)]):
                sizes.main()
            report = json.loads(output.read_text())
            self.assertEqual(report["calls"], 130)
            self.assertEqual(stdout.getvalue(), sizes.table(report) + "\n")
            self.assertEqual(len(stdout.getvalue().splitlines()), 15)
            self.assertEqual(list(output.parent.iterdir()), [output])
            self.assertNotIn(str(root), output.read_text())

    def test_failed_process_removes_stale_json(self):
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            sizes.subprocess, "run", side_effect=subprocess.CalledProcessError(3, ["dctx"])
        ):
            root = Path(tmp)
            binary = root / "dctx"
            binary.touch()
            output = root / "sizes.json"
            output.write_text('{"stale": true}')
            with patch.object(sys, "argv", ["shell_output_size.py", "--binary", str(binary),
                                           "--output", str(output)]):
                with self.assertRaises(subprocess.CalledProcessError):
                    sizes.main()
            self.assertFalse(output.exists())

    def test_missing_binary_removes_stale_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            output = root / "sizes.json"
            output.write_text('{"stale": true}')
            with patch.object(sys, "argv", ["shell_output_size.py", "--binary", str(root / "missing"),
                                           "--output", str(output)]):
                with self.assertRaises(FileNotFoundError):
                    sizes.main()
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
