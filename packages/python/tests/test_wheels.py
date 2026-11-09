# SPDX-License-Identifier: Apache-2.0
from __future__ import annotations

import base64
import csv
import hashlib
import importlib.util
import io
import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import unittest
import venv
import zipfile
from pathlib import Path
from unittest import mock

from test_parity import BINARY, PACKAGE, ROOT, isolated_environment

sys.path.insert(0, str(PACKAGE))
import build_backend

spec = importlib.util.spec_from_file_location("build_wheels", PACKAGE / "tools/build_wheels.py")
build_wheels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_wheels)


class WheelTests(unittest.TestCase):
    def test_wrapper_wheel_and_sdist_are_reproducible_and_have_verified_records(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            first = root / build_backend.build_wheel(str(root))
            expected = first.read_bytes()
            self.assertEqual(root / build_backend.build_wheel(str(root)), first)
            self.assertEqual(first.read_bytes(), expected)
            with zipfile.ZipFile(first) as archive:
                self.assertIn("dctx/py.typed", archive.namelist())
                record = next(path for path in archive.namelist() if path.endswith("/RECORD"))
                for path, digest, size in csv.reader(io.StringIO(archive.read(record).decode())):
                    if path == record:
                        self.assertEqual((digest, size), ("", ""))
                    else:
                        data = archive.read(path)
                        expected_hash = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
                        self.assertEqual(digest, "sha256=" + expected_hash)
                        self.assertEqual(int(size), len(data))
                metadata = archive.read(next(path for path in archive.namelist() if path.endswith("/METADATA"))).decode()
                self.assertEqual(metadata.count("Requires-Dist:"), 5)
                self.assertIn('extra == "binary"', metadata)
                self.assertIn("License-Expression: Apache-2.0", metadata)
            sdist = root / build_backend.build_sdist(str(root))
            original = sdist.read_bytes()
            build_backend.build_sdist(str(root))
            self.assertEqual(sdist.read_bytes(), original)
            with tarfile.open(sdist) as archive:
                archive.extractall(root / "unpacked", filter="data")
            # Building from an unpacked sdist must not need a repository or downloads.
            unpacked = next((root / "unpacked").iterdir())
            subprocess.run([sys.executable, "-c", "import build_backend; build_backend.build_wheel('wheelhouse')"],
                           cwd=unpacked, check=True)
            self.assertEqual(next((unpacked / "wheelhouse").glob("*.whl")).read_bytes(), expected)

    @unittest.skipUnless(BINARY, "Set DCTX_TEST_BIN for native binary validation")
    def test_builder_rejects_wrong_architecture_and_false_portability(self):
        binary = Path(BINARY).resolve()
        data = binary.read_bytes()
        if sys.platform == "linux":
            arch = "aarch64" if platform.machine() == "aarch64" else "x86_64"
            wrong = "x86_64" if arch == "aarch64" else "aarch64"
            with self.assertRaisesRegex(ValueError, "architecture"):
                build_wheels.validate_binary(data, f"{wrong}-unknown-linux-gnu")
            # Alter the program header to simulate a dynamic interpreter even for a static input.
            import struct
            dynamic = bytearray(data)
            header = struct.unpack_from("<Q", dynamic, 32)[0]
            struct.pack_into("<I", dynamic, header, 3)
            with self.assertRaisesRegex(ValueError, "static"):
                build_wheels.validate_binary(dynamic, f"{arch}-unknown-linux-musl")
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(build_wheels.build_backend, "project", return_value={"version": "0.0.0"}):
            with self.assertRaisesRegex(ValueError, "match exactly"):
                build_wheels.build(Path(directory))
            self.assertEqual(list(Path(directory).iterdir()), [])

    @unittest.skipUnless(BINARY, "Set DCTX_TEST_BIN for clean-venv offline install")
    def test_offline_binary_extra_installs_and_runs_without_source_or_network(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = Path(BINARY).resolve(strict=True)
            arch = platform.machine().lower()
            arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(arch, arch)
            target = os.environ.get("DCTX_WHEEL_TARGET") or {
                "linux": f"{arch}-unknown-linux-gnu",
                "darwin": f"{arch}-apple-darwin",
                "win32": "x86_64-pc-windows-msvc",
            }[sys.platform]
            wheelhouse = root / "wheelhouse"
            wheels = build_wheels.build(wheelhouse, binary, target)
            native = next(path for path in wheels if "binary" in path.name)
            with zipfile.ZipFile(native) as archive:
                executable = next(path for path in archive.namelist() if Path(path).name in ("dctx", "dctx.exe"))
                self.assertEqual(archive.read(executable), binary.read_bytes())
                self.assertEqual((archive.getinfo(executable).external_attr >> 16) & 0o777, 0o755)
            env = isolated_environment(root)
            # -I prevents PYTHONPATH/the source checkout from masking the installed wheel.
            env.pop("PYTHONPATH", None)
            env.update(PIP_CONFIG_FILE=os.devnull, PIP_NO_INDEX="1", PIP_DISABLE_PIP_VERSION_CHECK="1",
                       PIP_NO_CACHE_DIR="1", HTTP_PROXY="http://127.0.0.1:9", HTTPS_PROXY="http://127.0.0.1:9")
            environment = root / "isolated-venv"
            venv.EnvBuilder(with_pip=True).create(environment)
            python = environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
            # An audit hook rejects socket use even if pip were to ignore --no-index.
            offline_pip = '''import runpy, sys
 def audit(event, args):
     if event in ("socket.connect", "socket.getaddrinfo", "socket.bind"):
         raise RuntimeError("Offline install attempted network access: " + event)
 sys.addaudithook(audit)
 sys.argv = ["pip", "install", "--no-index", "--no-cache-dir", "--disable-pip-version-check", "--find-links", sys.argv[1], sys.argv[2]]
 runpy.run_module("pip", run_name="__main__")
'''.replace("\n ", "\n")
            version = build_backend.project()["version"]
            subprocess.run([str(python), "-I", "-c", offline_pip, str(wheelhouse), f"dctx[binary]=={version}"],
                           cwd=root, env=env, check=True, capture_output=True, timeout=120)
            cli = subprocess.run([str(binary), "languages", "--json"], cwd=root, env=env,
                                 check=True, capture_output=True, timeout=60)
            run = '''import pathlib, socket, sys
import dctx
def deny(*args, **kwargs):
    raise RuntimeError("Runtime attempted network access")
socket.socket = deny
socket.create_connection = deny
prefix = pathlib.Path(sys.prefix).resolve()
assert pathlib.Path(dctx.__file__).resolve().is_relative_to(prefix)
with dctx.Client(sys.argv[1]) as client:
    assert client.binary.is_relative_to(prefix)
    sys.stdout.buffer.write(client.languages())
'''
            result = subprocess.run([str(python), "-I", "-c", run, str(root)], cwd=root, env=env,
                                    check=True, capture_output=True, timeout=60)
            self.assertEqual(result.stdout, cli.stdout)
            self.assertEqual(result.stderr, b"")
            # Wrapper-only installation has a hard, actionable failure, not a download.
            key = build_wheels.TARGETS[target][0]
            subprocess.run([str(python), "-I", "-m", "pip", "uninstall", "--yes", f"dctx-binary-{key.replace('_', '-')}"],
                           cwd=root, env=env, check=True, capture_output=True, timeout=60)
            missing = subprocess.run([str(python), "-I", "-c",
                                     "import dctx, sys; dctx.Client(sys.argv[1])", str(root)],
                                    cwd=root, env=env, capture_output=True, timeout=60)
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn(f'--find-links ./wheelhouse "dctx[binary]=={version}"'.encode(), missing.stderr)


if __name__ == "__main__":
    unittest.main()
