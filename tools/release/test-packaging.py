#!/usr/bin/env python3
"""Exercise archive checks, npm tarballs and the shim without publishing anything."""
import hashlib
import importlib.util
import io
import json
import os
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("prepare_npm", ROOT / "tools/release/prepare-npm.py")
prepare_npm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare_npm)


class Packaging(unittest.TestCase):
    def test_artifacts_tarballs_and_shim(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            artifacts, packages = directory / "artifacts", directory / "packages"
            artifacts.mkdir()
            handshake = (
                b'#!/bin/sh\nif [ "$1" = rpc ]; then\n'
                b'IFS= read -r request\n'
                b"printf '%s\\n' '{\"id\":\"0\",\"engine_version\":\"1.2.3\",\"result\":{\"protocol_version\":\"1.0.0\",\"verbs\":[\"hello\"]}}'\n"
                b'cat >/dev/null\nexit 0\nfi\n'
            )
            binary = handshake + b'printf "arg=<%s>\\n" "$@"\nexit 23\n'
            checksums = []
            for platform, target in prepare_npm.PLATFORMS.items():
                windows = platform.startswith("win32")
                archive = artifacts / f"dctx-{target}.{'zip' if windows else 'tar.gz'}"
                if windows:
                    with zipfile.ZipFile(archive, "w") as package:
                        package.writestr("dctx.exe", binary)
                else:
                    with tarfile.open(archive, "w:gz") as package:
                        entry = tarfile.TarInfo("dctx")
                        entry.size, entry.mode = len(binary), 0o755
                        package.addfile(entry, io.BytesIO(binary))
                checksums.append(f"{hashlib.sha256(archive.read_bytes()).hexdigest()}  {archive.name}")
            (artifacts / "SHA256SUMS").write_text("\n".join(checksums) + "\n")
            prepare_npm.prepare("v1.2.3", artifacts, packages)
            for package in sorted(packages.iterdir()):
                manifest = json.loads((package / "package.json").read_text())
                self.assertEqual(manifest["version"], "1.2.3")
                result = subprocess.run(["npm", "pack", "--dry-run", "--json", "--ignore-scripts"],
                                        cwd=package, capture_output=True, text=True, check=True)
                contents = {f["path"] for f in json.loads(result.stdout)[0]["files"]}
                self.assertTrue({"LICENSE", "NOTICE", "README.md", "package.json"} <= contents)
                executable = "dctx.js" if package.name == "dctx" else "dctx.exe" if "win32" in package.name else "dctx"
                self.assertIn(f"bin/{executable}", contents)
                self.assertNotIn("scripts", manifest)
                print(f"npm pack --dry-run: {manifest['name']} PASS")

            main = packages / "dctx"
            native = packages / "cli-linux-x64"
            modules = main / "node_modules/@tejl"
            modules.mkdir(parents=True)
            (modules / "dctx-linux-x64").symlink_to(native, target_is_directory=True)
            command = ["node", str(main / "bin/dctx.js"), "with spaces", "--flag", "λ"]
            env = {**os.environ, "XDG_CACHE_HOME": str(directory / "cache"),
                   "HOME": str(directory / "home")}
            env.pop("DCTX_BIN", None)
            result = subprocess.run(command, capture_output=True, text=True, env=env)
            self.assertEqual(result.returncode, 23)
            self.assertEqual(result.stdout, "arg=<with spaces>\narg=<--flag>\narg=<λ>\n")
            self.assertEqual(result.stderr, "")
            (native / "bin/dctx").write_bytes(handshake + b"kill -TERM $$\n")
            result = subprocess.run(command, capture_output=True, text=True, env=env)
            self.assertEqual(result.returncode, -15)
            (modules / "dctx-linux-x64").unlink()
            result = subprocess.run(command, capture_output=True, text=True, env=env)
            self.assertEqual(result.returncode, 1)
            self.assertIn("npm install @tejl/dctx-linux-x64@1.2.3", result.stderr)
            self.assertNotIn("dctx@", result.stderr)
            # A damaged archive must be rejected before any output package is written.
            archive.write_bytes(b"corrupted")
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                prepare_npm.prepare("v1.2.3", artifacts, directory / "bad-packages")
            self.assertFalse((directory / "bad-packages").exists())


if __name__ == "__main__":
    unittest.main()
