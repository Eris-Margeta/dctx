# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Regression for synthetic -> fresh-tree output lifetime; no large fixture needed."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

PROJECT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("catalog_runner", PROJECT / "run.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class RunnerTests(unittest.TestCase):
    def test_fresh_tree_output_is_new_and_removed(self):
        for mode, expected_modes in (("--synthetic-only", ["synthetic", "build", "build"]),
                                     ("--fresh-only", ["build", "build"])):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(dir=PROJECT / "scratch") as temporary:
                root = Path(temporary)
                project = root / "tools/catalog-bench"
                project.mkdir(parents=True)
                report = root / "report.json"
                calls = []

                def fake_run(_binary, args):
                    calls.append(args[0])
                    output = Path(args[1] if args[0] == "synthetic" else args[2])
                    # Match the Rust writer's create_new contract.
                    with output.open("xb") as stream:
                        stream.write(b"catalog")
                    return {"status": 0, "values": {}}

                with patch.object(RUNNER, "WORKTREE", root), \
                     patch.object(RUNNER, "PROJECT", project), \
                     patch.object(RUNNER, "machine", return_value={}), \
                     patch.object(RUNNER, "run", side_effect=fake_run), \
                     patch.object(RUNNER, "cold_catalog", return_value=None), \
                     patch.object(RUNNER.resource, "setrlimit"), \
                     patch("sys.argv", ["run.py", mode, "--fresh-tree-files", "1", "--report", str(report)]):
                    self.assertEqual(RUNNER.main(), 0)
                self.assertEqual(calls, expected_modes)
                data = json.loads(report.read_text())
                self.assertTrue(data["fresh_tree_deleted"])
                self.assertEqual(data["fresh_tree"]["files"], 1)
                self.assertFalse(list(project.rglob("*.cat")))
                self.assertFalse(Path(data["fresh_tree"]["path"]).exists())


if __name__ == "__main__":
    unittest.main()
