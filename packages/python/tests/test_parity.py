# SPDX-License-Identifier: Apache-2.0
"""The same fixed corpus and cases as crates/dctx-cli/tests/rpc.rs."""
from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parents[1]
sys.path.insert(0, str(PACKAGE / "src"))
from dctx import Client, EngineError, __version__

BINARY = os.environ.get("DCTX_TEST_BIN")
CASES = [
    ("find", ("ping",)),
    ("find", ("not_present_zz",)),
    ("find", ("ping", "--limit", "1")),
    ("map", ("m:src/lib.rs",)),
    ("read", ("s:src/lib.rs#ping",)),
    ("read", ("m:src/lib.rs", "--lines", "1-2")),
    ("skeleton", ("m:src/lib.rs",)),
    ("explain", ("s:src/lib.rs#ping",)),
    ("neighbors", ("s:src/lib.rs#ping",)),
    ("grep", ("pub",)),
    ("hubs", ()),
    ("path", ("m:src/lib.rs", "s:src/lib.rs#ping")),
    ("pack", ()),
    ("context", ("--for", "`ping`")),
    ("languages", ()),
    ("cache_status", ()),
]


def cli_words(verb):
    if verb in ("pack", "context"):
        return ["pack"]
    if verb == "languages":
        return ["languages"]
    if verb == "cache_status":
        return ["cache", "status"]
    return ["map", verb]


def isolated_environment(root: Path) -> dict[str, str]:
    env = os.environ.copy()
    env.pop("DCTX_BIN", None)
    for key, directory in (
        ("HOME", "home"), ("USERPROFILE", "home"), ("XDG_CONFIG_HOME", "config"),
        ("APPDATA", "config"), ("XDG_CACHE_HOME", "cache"), ("XDG_DATA_HOME", "data"),
        ("LOCALAPPDATA", "local"), ("TMPDIR", "tmp"), ("TMP", "tmp"), ("TEMP", "tmp"),
        ("DCTX_CACHE_DIR", "engine-cache"), ("DCTX_MAP_CACHE", "engine-map-cache"),
    ):
        folder = root / directory
        folder.mkdir(parents=True, exist_ok=True)
        env[key] = str(folder)
    env.update(DCTX_AUTO_CLEAN="0", NO_COLOR="1")
    return env


class EngineRequiredTests(unittest.TestCase):
    def test_ci_requires_an_explicit_binary(self):
        if os.environ.get("DCTX_REQUIRE_ENGINE_TESTS") == "1":
            self.assertTrue(BINARY, "CI must set DCTX_TEST_BIN; parity/offline tests cannot be skipped")


@unittest.skipUnless(BINARY, "Set DCTX_TEST_BIN to run real-engine parity and offline installation")
class ParityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.env = mock.patch.dict(os.environ, isolated_environment(Path(self.temp.name)), clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.binary = str(Path(BINARY).resolve(strict=True))
        self.repo = ROOT / "crates/dctx/tests/fixtures/repository"

    def cli(self, verb, args):
        return subprocess.run([self.binary, *cli_words(verb), *args, "--json"],
                              cwd=self.repo, capture_output=True, timeout=60)

    def test_every_verb_matches_cli_output_bytes(self):
        with Client(self.repo, binary=self.binary) as client:
            hello = client.hello()
            self.assertEqual(set(hello["verbs"]) - {"hello"}, {verb for verb, _ in CASES})
            self.assertEqual(hello["protocol_version"].split(".")[0], "1")
            version = subprocess.run([self.binary, "--version"], capture_output=True, check=True)
            self.assertIn(__version__.encode(), version.stdout)
            for verb, args in CASES:
                with self.subTest(verb=verb, args=args):
                    cli = self.cli(verb, args)
                    self.assertEqual(cli.returncode, 0, cli.stderr)
                    self.assertEqual(getattr(client, verb)(*args), cli.stdout)

    def test_cli_errors_preserve_codes_and_emitted_output(self):
        with Client(self.repo, binary=self.binary) as client:
            cases = [
                ("read", ("m:absent.rs",)),
                ("find", ("",)),
                ("find", ("ping", "--place", "missing-directory")),
                ("languages", ("--bad",)),
                ("cache_status", ("--bad",)),
                ("pack", ("--ext", "rs")),
                ("context", ("--for", "`absent`")),
                ("pack", ("--for", "--json")),
            ]
            codes = set()
            for verb, args in cases:
                with self.subTest(verb=verb, args=args):
                    cli = self.cli(verb, args)
                    with self.assertRaises(EngineError) as raised:
                        getattr(client, verb)(*args)
                    error = raised.exception
                    codes.add(error.code)
                    self.assertEqual(error.code, cli.returncode)
                    self.assertEqual(error.output, cli.stdout)
                    diagnostic = f"❌ Error: {error.message}\n"
                    if error.hint:
                        diagnostic += f"hint: {error.hint}\n"
                    self.assertEqual(diagnostic.encode(), cli.stderr)
            self.assertEqual(codes, {1, 2, 3})
            self.assertEqual(client.languages("--json"), self.cli("languages", ()).stdout)


if __name__ == "__main__":
    unittest.main()
