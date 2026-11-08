# SPDX-License-Identifier: Apache-2.0
from __future__ import annotations

import ast
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parents[1]
sys.path.insert(0, str(PACKAGE / "src"))

from dctx import Client, EngineError, TransportError, VersionMismatchError, __version__
from dctx import _binary
from dctx._generated import Operations


class SchemaTests(unittest.TestCase):
    def test_generated_api_and_versions_are_current(self):
        subprocess.run([sys.executable, str(PACKAGE / "tools/generate_api.py"), "--check"], check=True)
        self.assertEqual(__version__, (ROOT / "VERSION").read_text().strip())
        schema = json.loads((ROOT / "docs/protocol/dctx-rpc-v1.schema.json").read_text())
        verbs = {case["enum"][0] for case in schema["definitions"]["Verb"]["oneOf"]}
        self.assertEqual(verbs - {"hello"}, {name for name in vars(Operations) if not name.startswith("_")})
        self.assertTrue(hasattr(Client, "hello"))

    def test_wrapper_boundary(self):
        allowed = {"__future__", "importlib", "json", "os", "pathlib", "platform", "subprocess",
                   "sys", "threading", "typing", "re"}
        for source in (PACKAGE / "src/dctx").glob("*.py"):
            tree = ast.parse(source.read_text(), filename=str(source))
            for node in ast.walk(tree):
                if isinstance(node, ast.Import):
                    for alias in node.names:
                        self.assertIn(alias.name.split(".")[0], allowed, str(source))
                elif isinstance(node, ast.ImportFrom) and not node.level:
                    self.assertIn(node.module.split(".")[0], allowed, str(source))
                if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
                    self.assertNotIn(node.func.attr, {"walk", "rglob", "glob", "read_text", "read_bytes",
                                                     "write_text", "write_bytes", "sort", "sorted"}, str(source))


class ResolutionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.executable = self.root / "engine with spaces"
        self.executable.write_bytes(b"engine")
        self.executable.chmod(0o755)
        # Cleared, but with a home directory: Windows Path.home() raises
        # RuntimeError without USERPROFILE, and the error message formats the cache path.
        home = {"HOME": str(self.root), "USERPROFILE": str(self.root)}
        self.env = mock.patch.dict(os.environ, home, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_explicit_then_environment_are_authoritative(self):
        with mock.patch.dict(os.environ, {"DCTX_BIN": str(self.root / "missing")}):
            self.assertEqual(_binary.resolve_binary(self.executable), self.executable)
            with self.assertRaises(_binary.BinaryNotFoundError):
                _binary.resolve_binary()
        with mock.patch.dict(os.environ, {"DCTX_BIN": str(self.executable)}):
            self.assertEqual(_binary.resolve_binary(), self.executable)
            with self.assertRaises(_binary.BinaryNotFoundError):
                _binary.resolve_binary(self.root / "missing")

    def test_wheel_before_cache_and_broken_wheel_is_not_hidden(self):
        module = self.root / "vendored" / "__init__.py"
        module.parent.mkdir()
        module.touch()
        vendored = module.parent / _binary.executable_name()
        vendored.write_bytes(b"engine")
        vendored.chmod(0o755)
        spec = importlib.util.spec_from_file_location("vendor", module)
        with mock.patch.object(_binary.importlib.util, "find_spec", return_value=spec), \
             mock.patch.object(_binary, "cache_binary", return_value=self.executable):
            self.assertEqual(_binary.resolve_binary(), vendored)
            vendored.unlink()
            with self.assertRaises(_binary.BinaryNotFoundError):
                _binary.resolve_binary()

    def test_cache_then_exact_install_instructions(self):
        with mock.patch.object(_binary.importlib.util, "find_spec", return_value=None), \
             mock.patch.object(_binary, "cache_binary", return_value=self.executable):
            self.assertEqual(_binary.resolve_binary(), self.executable)
            self.executable.unlink()
            with self.assertRaises(_binary.BinaryNotFoundError) as error:
                _binary.resolve_binary()
            self.assertIn(f'python -m pip install "dctx[binary]=={__version__}"', str(error.exception))
            self.assertIn("--no-index --find-links ./wheelhouse", str(error.exception))
            self.assertIn(str(self.executable), str(error.exception))

    def test_platform_and_user_cache_locations(self):
        for system, machine, key, cache in (
            ("linux", "x86_64", "linux_x86_64", self.root / "xdg"),
            ("darwin", "arm64", "darwin_aarch64", self.root / "Library/Caches"),
            ("win32", "AMD64", "windows_x86_64", self.root / "local"),
        ):
            with self.subTest(system=system), mock.patch.object(_binary.sys, "platform", system), \
                 mock.patch.object(_binary.platform, "machine", return_value=machine), \
                 mock.patch.object(_binary.Path, "home", return_value=self.root), \
                 mock.patch.dict(os.environ, {"XDG_CACHE_HOME": str(self.root / "xdg"),
                                              "LOCALAPPDATA": str(self.root / "local")}):
                self.assertEqual(_binary.platform_key(), key)
                filename = "dctx.exe" if system == "win32" else "dctx"
                self.assertEqual(_binary.cache_binary(), cache / "dctx/bin" / __version__ / key / filename)
        # Empty directory variables must still use the per-user cache, never cwd.
        for system, folder in (("linux", ".cache"), ("win32", "AppData/Local")):
            with self.subTest(empty_cache_environment=system), \
                 mock.patch.object(_binary.sys, "platform", system), \
                 mock.patch.object(_binary.Path, "home", return_value=self.root), \
                 mock.patch.dict(os.environ, {"XDG_CACHE_HOME": "", "LOCALAPPDATA": ""}):
                expected = self.root / folder / "dctx/bin" / __version__
                self.assertTrue(_binary.cache_binary().is_relative_to(expected))


class SessionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.server = self.root / "fake engine.py"
        self.popen = subprocess.Popen

    def client(self, mode="normal"):
        self.server.write_text('''import json, sys, time
for line in sys.stdin.buffer:
    request = json.loads(line)
    verb = request["verb"]
    reply = {"id": request["id"], "engine_version": VERSION, "future": True}
    if verb == "hello":
        reply["result"] = {"protocol_version": PROTOCOL, "verbs": ["hello", "languages", "future_verb"], "future": 1}
        if MODE == "bad_hello": reply["result"]["verbs"] = [False]
    elif MODE == "blocked":
        print("STARTED", file=sys.stderr, flush=True)
        time.sleep(30)
    elif MODE == "bad_json":
        print("{invalid", flush=True)
        continue
    elif MODE == "truncated":
        sys.stdout.write("{}")
        sys.stdout.flush()
        break
    elif MODE == "wrong_id":
        reply["id"] = False
        reply["result"] = {"output": "wrong"}
    elif MODE == "wrong_version_later":
        reply["engine_version"] = "0.0.0"
        reply["result"] = {"output": "wrong"}
    elif MODE == "bad_error":
        reply["error"] = {"code": True, "message": "m", "hint": "h", "output": ""}
    elif MODE == "surrogate":
        reply["result"] = {"output": chr(0xd800)}
    elif MODE == "both":
        reply["result"] = {"output": ""}
        reply["error"] = {}
    elif MODE == "no_output":
        reply["result"] = {}
    elif MODE == "error":
        reply["error"] = {"code": 1, "message": "gone", "hint": "refresh", "output": "before\\r\\n\\n", "future": 1}
    else:
        reply["result"] = {"output": json.dumps(request["args"], ensure_ascii=False) + "\\r\\n\\n", "future": 1}
    print(json.dumps(reply), flush=True)
if MODE == "hang_on_eof":
    time.sleep(30)
'''.replace("VERSION", repr("0.0.0" if mode == "version" else __version__))
   .replace("PROTOCOL", repr("2.0.0" if mode == "protocol" else "1.42.0"))
   .replace("MODE", repr(mode)), encoding="utf-8")
        self.server.chmod(0o755)
        def launch(args, **kwargs):
            self.assertEqual(args, [str(self.server), "rpc"])
            self.assertIsNone(kwargs["stderr"])
            if mode == "blocked":
                kwargs["stderr"] = subprocess.PIPE
            return self.popen([sys.executable, str(self.server), "rpc"], **kwargs)
        patch = mock.patch("dctx._client.subprocess.Popen", side_effect=launch)
        with patch:
            client = Client(self.root, binary=self.server)
        self.addCleanup(client.close)
        return client

    def test_opaque_bytes_arguments_and_forward_compatible_envelope(self):
        with self.client() as client:
            args = ("; touch unwanted", "--limit", "1", "space \' quote", "λ雪", "--cursor", "opaque")
            expected = (json.dumps(args, ensure_ascii=False) + "\r\n\n").encode("utf-8")
            self.assertEqual(client.find(*args), expected)
            self.assertEqual(client.hello()["protocol_version"], "1.42.0")
            self.assertFalse((self.root / "unwanted").exists())
        with self.assertRaises(TransportError):
            client.languages()
        self.assertEqual(client._process.returncode, 0)
        self.assertTrue(client._process.stdin.closed)
        self.assertTrue(client._process.stdout.closed)

    def test_errors_keep_output_and_session_remains_usable(self):
        with self.client("error") as client:
            for _ in range(2):
                with self.assertRaises(EngineError) as error:
                    client.read("m:absent.rs")
                self.assertEqual((error.exception.code, error.exception.message, error.exception.hint,
                                  error.exception.output), (1, "gone", "refresh", b"before\r\n\n"))
            self.assertIn("hello", client.hello()["verbs"])

    def test_mismatched_handshakes_and_malformed_hello_are_refused(self):
        for mode, exception in (("version", VersionMismatchError), ("protocol", VersionMismatchError),
                                ("bad_hello", TransportError)):
            with self.subTest(mode=mode), self.assertRaises(exception):
                self.client(mode)

    def test_transport_failure_is_terminal_without_retries(self):
        for mode in ("bad_json", "truncated", "wrong_id", "wrong_version_later", "bad_error", "surrogate", "both", "no_output"):
            with self.subTest(mode=mode):
                with self.client(mode) as client:
                    with self.assertRaises(TransportError):
                        client.languages()
                    self.assertIsNotNone(client._process.poll())
                    with self.assertRaises(TransportError):
                        client.languages()

    def test_graceful_shutdown_timeout_reaps_the_engine(self):
        client = self.client("hang_on_eof")
        client.close(timeout=0.1)
        self.assertIsNotNone(client._process.poll())
        self.assertTrue(client._process.stdin.closed)
        self.assertTrue(client._process.stdout.closed)
        client.close()

    def test_request_validation_preserves_session(self):
        with self.client() as client:
            with self.assertRaises(TypeError):
                client.find(1)
            with self.assertRaises(ValueError):
                client.find("x" * 1_048_576)
            self.assertEqual(client.languages(), b"[]\r\n\n")

    def test_concurrent_calls_stay_correlated(self):
        with self.client() as client:
            results, failures = {}, []
            def run(index):
                try:
                    results[index] = client.find(str(index))
                except BaseException as exc:
                    failures.append(exc)
            threads = [threading.Thread(target=run, args=(index,)) for index in range(12)]
            for thread in threads:
                thread.start()
            for thread in threads:
                thread.join(timeout=10)
                self.assertFalse(thread.is_alive())
            self.assertEqual(failures, [])
            self.assertEqual(results, {i: f'["{i}"]\r\n\n'.encode() for i in range(12)})

    def test_terminate_aborts_running_call(self):
        with self.client("blocked") as client:
            failures = []
            def run():
                try:
                    client.languages()
                except TransportError as exc:
                    failures.append(exc)
            thread = threading.Thread(target=run)
            thread.start()
            # The child signals after reading the operation, while the call owns
            # the session lock. terminate must kill before acquiring that lock.
            self.assertEqual(client._process.stderr.readline().rstrip(b"\r\n"), b"STARTED")
            client.terminate()
            client._process.stderr.close()
            thread.join(timeout=10)
            self.assertFalse(thread.is_alive())
            self.assertEqual(len(failures), 1)
            self.assertIsNotNone(client._process.poll())


if __name__ == "__main__":
    unittest.main()
