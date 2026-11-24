# SPDX-License-Identifier: Apache-2.0
"""Compare wrapper replies with the real CLI and independent fixture expectations."""
from __future__ import annotations

import json
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
from dctx import Client, EngineError, TransportError, __version__

BINARY = os.environ.get("DCTX_TEST_BIN")
WRAPPER_CASES = ROOT / "tools/engine-contract/wrapper-cases/pages.json"
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
    ("trace", ("s:src/lib.rs#ping", "s:src/lib.rs#ping")),
    # The isolated ping has no directed path to its module or incoming callers.
    ("trace", ("s:src/lib.rs#ping", "m:src/lib.rs", "--max-depth", "1",
               "--max-paths", "1", "--max-bytes", "256")),
    ("impact", ("s:src/lib.rs#ping",)),
    ("impact", ("s:src/lib.rs#ping", "--max-depth", "1",
                "--max-sites", "1", "--max-bytes", "256")),
    ("pack", ()),
    ("context", ("--for", "`ping`")),
    ("languages", ()),
    ("cache_status", ()),
]


def cli_words(verb, args):
    if verb == "context" and "--for" not in args:
        return ["context"]
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
        self.binary = str(Path(BINARY).resolve())
        self.assertTrue(Path(self.binary).is_file() and os.access(self.binary, os.X_OK),
                        f"DCTX_TEST_BIN is not executable: {self.binary!r}")
        self.repo = ROOT / "crates/dctx/tests/fixtures/repository"

    def cli(self, verb, args):
        words = [self.binary, *cli_words(verb, args), *args, "--json"]
        try:
            return subprocess.run(words, cwd=self.repo, capture_output=True, timeout=60)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise self.failureException(f"CLI did not complete: {str(error)!r}") from None

    def connect(self):
        try:
            return Client(self.repo, binary=self.binary)
        except (OSError, TransportError) as error:
            raise self.failureException(f"Could not connect to engine: {str(error)!r}") from None

    def call(self, client, verb, args):
        try:
            return getattr(client, verb)(*args)
        except (EngineError, TransportError) as error:
            raise self.failureException(f"{verb!r} failed: {str(error)!r}") from None

    def failure(self, client, verb, args, cli):
        with self.assertRaises(EngineError) as raised:
            try:
                getattr(client, verb)(*args)
            except TransportError as error:
                raise self.failureException(f"{verb!r} did not return an engine reply: {str(error)!r}") from None
        error = raised.exception
        self.assertEqual(error.code, cli.returncode)
        self.assertEqual(error.output, cli.stdout)
        diagnostic = f"❌ Error: {error.message}\n"
        if error.hint:
            diagnostic += f"hint: {error.hint}\n"
        self.assertEqual(diagnostic.encode(), cli.stderr)
        return error

    def wrapper_fixture(self):
        cases = json.loads(WRAPPER_CASES.read_bytes())
        self.repo = Path(self.temp.name) / "wrapper-pages"
        self.repo.mkdir()
        for name, content in cases["files"].items():
            (self.repo / name).write_bytes(content.encode())
        return cases

    def page(self, client, verb, args):
        cli = self.cli(verb, args)
        self.assertEqual(cli.returncode, 0, repr(cli.stderr))
        output = self.call(client, verb, args)
        self.assertEqual(output, cli.stdout)
        return self.decode_page(output)

    def decode_page(self, output):
        try:
            return json.loads(output)
        except (UnicodeError, ValueError):
            raise self.failureException(f"Invalid JSON page: {output!r}") from None

    def assert_fields(self, actual, expected, field="page"):
        if isinstance(expected, dict):
            self.assertIsInstance(actual, dict, repr(field))
            for key, value in expected.items():
                self.assertIn(key, actual, repr(field))
                self.assert_fields(actual[key], value, f"{field}.{key}")
        elif isinstance(expected, list):
            self.assertIsInstance(actual, list, repr(field))
            self.assertEqual(len(actual), len(expected), repr(field))
            for index, (item, value) in enumerate(zip(actual, expected)):
                self.assert_fields(item, value, f"{field}[{index}]")
        else:
            self.assertIs(type(actual), type(expected), repr(field))
            self.assertEqual(repr(actual), repr(expected), repr(field))

    def test_every_verb_matches_cli_output_bytes(self):
        with self.connect() as client:
            hello = client.hello()
            self.assertEqual(set(hello["verbs"]) - {"hello"}, {verb for verb, _ in CASES})
            self.assertEqual(repr(hello["protocol_version"].split(".")[0]), repr("1"))
            version = subprocess.run([self.binary, "--version"], capture_output=True, check=True)
            self.assertIn(__version__.encode(), version.stdout)
            for verb, args in CASES:
                with self.subTest(verb=verb, args=args):
                    cli = self.cli(verb, args)
                    self.assertEqual(cli.returncode, 0, repr(cli.stderr))
                    self.assertEqual(self.call(client, verb, args), cli.stdout)

    def test_cli_errors_preserve_codes_and_emitted_output(self):
        with self.connect() as client:
            hostile = json.loads(WRAPPER_CASES.read_bytes())["hostile_flag"]
            cases = [
                ("read", ("m:absent.rs",)),
                ("find", ("",)),
                ("find", ("ping", "--place", "missing-directory")),
                ("languages", ("--bad",)),
                ("cache_status", ("--bad",)),
                ("pack", ("--ext", "rs")),
                ("context", ("--for", "`absent`")),
                ("pack", ("--for", "--json")),
                ("languages", (hostile,)),
            ]
            codes = set()
            for verb, args in cases:
                with self.subTest(verb=verb, args=args):
                    cli = self.cli(verb, args)
                    error = self.failure(client, verb, args, cli)
                    codes.add(error.code)
                    if args == (hostile,):
                        self.assertEqual(error.code, 2)
                        self.assertIn(hostile, error.message)
                        self.assertTrue(error.hint)
            self.assertEqual(codes, {1, 2, 3})
            self.assertEqual(self.call(client, "languages", ("--json",)), self.cli("languages", ()).stdout)

    def test_mixed_named_context_delivers_history_on_the_same_connection(self):
        case = self.wrapper_fixture()["context"]
        good, gone = case["good"], case["gone"]
        flags = ("--session", case["session"], "--dedupe")
        mixed = (good, gone, *flags)
        with self.connect() as client:
            error = self.failure(client, "context", mixed, self.cli("context", mixed))
            self.assertEqual(error.code, 1)
            pages = error.output.splitlines(keepends=True)
            self.assertEqual(len(pages), 2)
            full, missing = map(self.decode_page, pages)
            self.assert_fields(missing, {"verb": "context", "target": gone, "chunks": []})
            self.assert_fields(full, {"verb": "context", "target": case["target"], "chunks": [
                {"target": case["target"], "role": "source", "ref": False, "content": case["content"]},
            ]})
            chunk_id = full["chunks"][0]["id"]
            self.assertTrue(chunk_id)

            repeat = self.call(client, "context", (good, *flags))
            cli_repeat = self.cli("context", (good, good, *flags))
            self.assertEqual(cli_repeat.returncode, 0, repr(cli_repeat.stderr))
            self.assertEqual(cli_repeat.stdout, pages[0] + repeat)
            reference = self.decode_page(repeat)
            self.assert_fields(reference, {"target": case["target"], "chunks": [
                {"id": chunk_id, "ref": True},
            ]})
            self.assertNotIn("content", reference["chunks"][0])

        with self.connect() as fresh:
            cli_full = self.cli("context", (good, *flags))
            self.assertEqual(cli_full.returncode, 0, repr(cli_full.stderr))
            self.assertEqual(self.call(fresh, "context", (good, *flags)), cli_full.stdout)
            self.assertEqual(cli_full.stdout, pages[0])

    def test_read_spans_match_crlf_and_unterminated_source_bytes(self):
        cases = self.wrapper_fixture()
        with self.connect() as client:
            for case in cases["reads"]:
                with self.subTest(target=case["target"]):
                    page = self.page(client, "read", (case["target"],))
                    self.assert_fields(page, {"records": [{
                        "id": case["target"], "path": case["file"], "lines": case["lines"],
                    }]})
                    record = page["records"][0]
                    disk = (self.repo / case["file"]).read_bytes()
                    lines = disk.split(b"\n")
                    first, last = case["lines"]
                    start = sum(len(line) + 1 for line in lines[:first - 1])
                    selected = b"\n".join(lines[first - 1:last]).removesuffix(b"\r")
                    end = start + len(selected)
                    self.assert_fields(record, {"span": [start, end]})
                    self.assertEqual(record["source_utf8"].encode(), disk[start:end])

    def test_truncated_graphs_and_two_cursor_pages(self):
        cases = self.wrapper_fixture()
        with self.connect() as client:
            for case in cases["graph"]:
                self.assert_fields(self.page(client, case["verb"], case["args"]), case["expected"])
            outline = cases["outline"]
            args = (outline["target"], "--limit", "1")
            first = self.page(client, "map", args)
            self.assert_fields(first, {"records": [{"id": outline["ids"][0]}], "more": 1})
            cursor = first["next"]
            self.assertTrue(isinstance(cursor, str) and cursor)
            second = self.page(client, "map", (*args, "--cursor", cursor))
            self.assert_fields(second, {"records": [{"id": outline["ids"][1]}], "more": 0, "next": None})

    def test_extraction_markers_in_successful_and_refused_pages(self):
        case = self.wrapper_fixture()["extraction"]
        # The index skips an oversized input before reading its contents.
        with (self.repo / case["oversized_file"]).open("wb") as oversized:
            oversized.truncate(case["size"])
        with self.connect() as client:
            for name in ("partial", "opaque"):
                row = case[name]
                self.assert_fields(self.page(client, row["verb"], row["args"]), row["expected"])
            refused = case["refused_read"]
            cli = self.cli(refused["verb"], refused["args"])
            error = self.failure(client, refused["verb"], refused["args"], cli)
            self.assertEqual(error.code, 1)
            self.assert_fields(self.decode_page(error.output), refused["expected"])

    def test_pack_to_file_returns_empty_stdout_and_writes_the_cli_page(self):
        cases = self.wrapper_fixture()
        output = Path(self.temp.name) / "pack.json"
        with self.connect() as client:
            self.assertEqual(self.call(client, "pack", ("--output", str(output))), b"")
        cli = self.cli("pack", ())
        self.assertEqual(cli.returncode, 0, repr(cli.stderr))
        saved = output.read_bytes()
        self.assertEqual(saved, cli.stdout)
        self.assert_fields(self.decode_page(saved), {"files": len(cases["files"])})


if __name__ == "__main__":
    unittest.main()
