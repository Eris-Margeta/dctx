# SPDX-License-Identifier: Apache-2.0
"""Validate real RPC envelopes and cache status refusals against the wire contract."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from jsonschema import Draft7Validator
from jsonschema.exceptions import SchemaError

from parity import ROOT, isolated_env


HOSTILE = 'correlation\n\x1b[31m"'
HELLO = {"id": HOSTILE, "verb": "hello"}


class WireEnvelopes(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        binary = os.environ.get("DCTX_TEST_BIN")
        if not binary:
            if os.environ.get("DCTX_REQUIRE_ENGINE_TESTS") == "1":
                raise AssertionError("DCTX_TEST_BIN is required")
            raise unittest.SkipTest("DCTX_TEST_BIN is unset")
        cls.binary = Path(binary).resolve()
        schema = json.loads((ROOT / "docs/protocol/dctx-rpc-v1.schema.json").read_bytes())
        try:
            Draft7Validator.check_schema(schema)
        except SchemaError as error:
            raise AssertionError(f"invalid wire schema: {error.message!a}") from None
        cls.validator = Draft7Validator(schema)

    def setUp(self):
        temp = self.enterContext(tempfile.TemporaryDirectory(prefix=".dctx-wire-", dir=ROOT))
        self.repo = Path(temp) / "repo"
        self.repo.mkdir()
        self.env = isolated_env(Path(temp) / "env")

    def run_engine(self, args, wire=None):
        try:
            return subprocess.run(
                [self.binary, *args], cwd=self.repo, env=self.env, input=wire,
                capture_output=True, timeout=30, check=False,
            )
        except (OSError, subprocess.SubprocessError) as error:
            self.fail(f"engine command failed for {args!a}: {error!a}")

    def rpc(self, *frames):
        wire = b"".join(
            (frame if isinstance(frame, bytes) else json.dumps(frame).encode()) + b"\n"
            for frame in frames
        )
        result = self.run_engine(["rpc"], wire)
        self.assertEqual(result.returncode, 0, f"RPC stderr: {result.stderr!r}")
        self.assertEqual(result.stderr, b"")
        lines = result.stdout.split(b"\n")
        self.assertEqual(lines[-1], b"")
        self.assertEqual(len(lines) - 1, len(frames), f"RPC stdout: {result.stdout!r}")
        self.assertNotIn(b"\x1b", result.stdout)
        return [json.loads(line) for line in lines[:-1]]

    def assert_wire(self, frame):
        error = next(self.validator.iter_errors(frame), None)
        if error is not None:
            self.fail(f"wire schema rejected {json.dumps(frame)}: {error.message!a}")

    def test_hello_and_raw_refusals_validate(self):
        self.assert_wire(HELLO)
        hello, malformed, missing, null_id, correlated = self.rpc(
            HELLO,
            b'{"id":',
            {"verb": "hello"},
            {"id": None, "verb": "hello"},
            {"id": 42, "verb": HOSTILE},
        )
        self.assert_wire(hello)
        self.assertEqual(ascii(hello["id"]), ascii(HOSTILE))
        self.assertEqual(hello["result"]["protocol_version"].encode(), b"1.0.0")
        for reply, expected_id in zip((malformed, missing, null_id, correlated), (None, None, None, 42)):
            self.assert_wire(reply)
            self.assertEqual(reply["id"], expected_id)
            self.assertEqual(reply["error"]["code"], 2)
            self.assertEqual(reply["error"]["output"].encode(), b"")

    def test_schema_rejects_missing_ids_and_null_request_ids(self):
        hello, refusal = self.rpc(HELLO, b'{"id":')
        for frame in (HELLO, hello, refusal):
            without_id = {key: value for key, value in frame.items() if key != "id"}
            self.assertFalse(self.validator.is_valid(without_id), f"accepted {without_id!a}")
        self.assertFalse(self.validator.is_valid({**HELLO, "id": None}))

    def test_schema_failure_escapes_hostile_values(self):
        with self.assertRaises(AssertionError) as failure:
            self.assert_wire({**HELLO, "id": [HOSTILE]})
        diagnostic = str(failure.exception)
        self.assertIn(json.dumps(HOSTILE), diagnostic)
        self.assertNotIn("\n", diagnostic)
        self.assertNotIn("\x1b", diagnostic)

    def test_cache_status_preserves_cli_refusals(self):
        cases = (
            (["--root", "424242"], 1, "root is not registered"),
            (["--root"], 2, "--root requires a value"),
            (["--bogus"], 2, "unknown status flag"),
        )
        hello, *replies = self.rpc(HELLO, *(
            {"id": i, "verb": "cache_status", "args": args}
            for i, (args, _, _) in enumerate(cases)
        ))
        self.assert_wire(hello)
        for i, ((args, code, message), reply) in enumerate(zip(cases, replies)):
            with self.subTest(args=args):
                cli = self.run_engine(["cache", "status", "--json", *args])
                self.assertEqual(cli.returncode, code, f"CLI stderr: {cli.stderr!r}")
                self.assert_wire(reply)
                self.assertEqual(reply["id"], i)
                error = reply["error"]
                self.assertEqual(error["code"], code)
                self.assertIn(message, error["message"])
                self.assertTrue(error["hint"])
                self.assertEqual(error["output"].encode(), cli.stdout)
                self.assertEqual(f"❌ Error: {error['message']}\nhint: {error['hint']}\n".encode(), cli.stderr)


if __name__ == "__main__":
    unittest.main()
