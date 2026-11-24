# SPDX-License-Identifier: Apache-2.0
"""Exercise the local MSRV gate without installing or building a toolchain."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
STUB = r'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

tool = Path(sys.argv[0]).name
if tool == "bun":
    sys.exit(0)
with Path("calls.jsonl").open("a") as calls:
    print(json.dumps(sys.argv[1:]), file=calls)
if tool == "rustup" and sys.argv[1] == "run":
    sys.exit(int(os.environ["MISSING_TOOLCHAIN"]))
if tool == "cargo" and os.environ["CARGO_ERROR"]:
    sys.stderr.write(os.environ["CARGO_ERROR"])
    sys.exit(101)
'''


class MsrvCheckTests(unittest.TestCase):
    def run_gate(self, *, missing=False, cargo_error=""):
        with tempfile.TemporaryDirectory(dir=ROOT) as temp:
            root = Path(temp)
            (root / "Cargo.toml").write_text('[workspace.package]\nrust-version = "1.92.0"\n')
            (root / "apps/dctx-desktop").mkdir(parents=True)
            for tool in ("rustup", "cargo", "bun"):
                stub = root / tool
                stub.write_text(STUB)
                stub.chmod(0o755)
            result = subprocess.run(
                ["just", "--justfile", str(ROOT / "justfile"), "--working-directory", ".", "msrv-check"],
                cwd=root,
                env={**os.environ, "PATH": os.pathsep.join((str(root), os.environ["PATH"])),
                     "MISSING_TOOLCHAIN": str(int(missing)), "CARGO_ERROR": cargo_error},
                capture_output=True,
                text=True,
            )
            calls = [json.loads(line) for line in (root / "calls.jsonl").read_text().splitlines()]
            return result, calls

    def test_uses_declared_version_for_all_workspace_targets(self):
        result, calls = self.run_gate()
        self.assertEqual(result.returncode, 0, ascii(result.stderr))
        self.assertEqual(calls, [
            ["run", "1.92.0", "cargo", "--version"],
            ["+1.92.0", "check", "--workspace", "--all-targets", "--locked", "--color", "never"],
        ])

    def test_installs_missing_toolchain(self):
        result, calls = self.run_gate(missing=True)
        self.assertEqual(result.returncode, 0, ascii(result.stderr))
        self.assertEqual(calls[1], ["toolchain", "install", "1.92.0", "--profile", "minimal"])

    def test_escapes_cargo_failure_output(self):
        hostile = "broken path\n\x1b[31m' quoted\".rs"
        result, _ = self.run_gate(cargo_error=hostile)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f"MSRV check failed (101): {ascii(hostile)}", result.stderr.splitlines())
        self.assertNotIn("\x1b", result.stderr)


if __name__ == "__main__":
    unittest.main()
