# SPDX-License-Identifier: Apache-2.0
"""Exercise archive, pip and installed engine identity using disposable local artifacts."""

import base64
import hashlib
import io
import json
import os
import platform
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_wheel_install as check


LINUX = "x86_64-unknown-linux-musl"
WINDOWS = "x86_64-pc-windows-msvc"
FORMATS = (
    (LINUX, "tar.gz", "dctx_binary_linux_x86_64/dctx", "manylinux_2_17_x86_64.musllinux_1_2_x86_64"),
    (WINDOWS, "zip", "dctx_binary_windows_x86_64/dctx.exe", "win_amd64"),
)
VERSION = (check.ROOT / "VERSION").read_text().strip()
ENGINE = b"same version\x00payload"
HOSTILE = 'outside\n\x1b"fixture'


def write_members(path, entries):
    if path.suffix == ".gz":
        with tarfile.open(path, "w:gz", compresslevel=1) as archive:
            for name, data, kind in entries:
                member = tarfile.TarInfo(name)
                member.type = {stat.S_IFREG: tarfile.REGTYPE, stat.S_IFLNK: tarfile.SYMTYPE}[kind]
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
    else:
        with zipfile.ZipFile(path, "w") as archive:
            for name, data, kind in entries:
                member = zipfile.ZipInfo(name)
                member.create_system = 3
                member.external_attr = (kind | 0o755) << 16
                with warnings.catch_warnings():
                    warnings.filterwarnings("ignore", message="Duplicate name:", category=UserWarning)
                    archive.writestr(member, data)


class WheelInstallTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=check.ROOT)
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.house = self.root / "wheelhouse with spaces"
        self.house.mkdir()

    def candidates(self, member, tag):
        module = member.split("/")[0]
        return (self.house / f"dctx-{VERSION}-py3-none-any.whl",
                self.house / f"{module}-{VERSION}-py3-none-{tag}.whl")

    def checker(self, target):
        return subprocess.run(
            [sys.executable, "-B", str(check.SCRIPT), "--target", target,
             "--archives", str(self.root), "--wheelhouse", str(self.house)],
            capture_output=True, timeout=180,
            env=dict(os.environ, DCTX_BIN=str(self.root / "missing override")),
        )

    def test_matching_and_changed_engine_bytes_in_both_formats(self):
        for target, suffix, member, tag in FORMATS:
            with self.subTest(target=target):
                archive = self.root / f"dctx-{target}.{suffix}"
                _, native = self.candidates(member, tag)
                write_members(archive, [(Path(member).name, ENGINE, stat.S_IFREG)])
                for data in (ENGINE, ENGINE[:-1] + b"X", ENGINE + b"X", ENGINE[:-1]):
                    write_members(native, [(member, data, stat.S_IFREG)])
                    if data == ENGINE:
                        digest = check.compare_engines(archive, native, member)
                        self.assertEqual(digest, hashlib.sha256(data).hexdigest())
                    else:
                        with self.assertRaisesRegex(ValueError, "bytes differ"):
                            check.compare_engines(archive, native, member)

    def test_member_must_be_unique_regular_and_at_expected_path(self):
        for target, suffix, member, tag in FORMATS:
            archive = self.root / f"dctx-{target}.{suffix}"
            _, native = self.candidates(member, tag)
            for path, expected in ((archive, Path(member).name), (native, member)):
                regular = (expected, ENGINE, stat.S_IFREG)
                variants = ([], [regular, regular], [regular, ("other/" + expected, ENGINE, stat.S_IFREG)],
                            [("other/" + expected, ENGINE, stat.S_IFREG)], [(expected, b"", stat.S_IFLNK)])
                for entries in variants:
                    with self.subTest(target=target, file=path.name, entries=entries):
                        write_members(path, entries)
                        with self.assertRaisesRegex(ValueError, "one regular|Non-regular"):
                            with check.engine_member(path, expected):
                                self.fail("Unexpectedly accepted invalid member")

    def test_only_the_two_named_upload_candidates_are_accepted(self):
        _, _, member, tag = FORMATS[0]
        module = member.split("/")[0]
        wrapper, native = self.candidates(member, tag)
        args = (self.house, module, tag.split("."), VERSION)
        wrapper.touch()
        with self.assertRaisesRegex(ValueError, "upload candidates"):
            check.upload_pair(*args)
        native.touch()
        self.assertEqual(check.upload_pair(*args), (wrapper, native))
        (self.house / "extra.whl").touch()
        with self.assertRaisesRegex(ValueError, "upload candidates"):
            check.upload_pair(*args)

    def test_pip_must_select_both_exact_local_files_and_hashes(self):
        for target, _, member, tag in FORMATS:
            with self.subTest(target=target):
                wrapper, native = self.candidates(member, tag)
                wheels = {wrapper: "a" * 64, native: "b" * 64}

                def row(path, digest):
                    return {"download_info": {"url": path.as_uri(), "archive_info": {"hashes": {"sha256": digest}}}}

                for chosen, other in ((wrapper, native), (native, wrapper)):
                    good = row(chosen, wheels[chosen])
                    another = row(other, wheels[other])
                    check.check_selection({"install": [another, good]}, wheels)
                    for wrong in ([], [another], [good, good], [row(chosen, "0" * 64)],
                                  [row(self.root / chosen.name, wheels[chosen])],
                                  [row(self.house / "wrong.whl", wheels[chosen])]):
                        with self.assertRaisesRegex(ValueError, "pip selec"):
                            check.check_selection({"install": [another, *wrong]}, wheels)

    def test_hostile_archive_member_is_escaped_on_one_line(self):
        target, suffix, member, tag = FORMATS[1]
        archive = self.root / f"dctx-{target}.{suffix}"
        wrapper, native = self.candidates(member, tag)
        wrapper.touch()
        write_members(native, [(member, ENGINE, stat.S_IFREG)])
        write_members(archive, [(HOSTILE + "/dctx.exe", ENGINE, stat.S_IFREG)])
        result = self.checker(target)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(len(result.stderr.splitlines()), 1)
        self.assertIn(b"outside", result.stderr)
        self.assertIn(b"fixture", result.stderr)
        self.assertNotIn(b"\x1b", result.stderr)
        self.assertNotIn(HOSTILE.encode(), result.stderr)

    def test_installed_record_and_archive_must_both_match_engine(self):
        _, _, member, _ = FORMATS[0]
        module = member.split("/")[0]
        engine = self.root / member
        engine.parent.mkdir()
        engine.write_bytes(ENGINE)
        info = self.root / f"{module}-{VERSION}.dist-info"
        info.mkdir()
        (info / "METADATA").write_text(f"Name: {module.replace('_', '-')}\nVersion: {VERSION}\n")
        record = info / "RECORD"
        digest = hashlib.sha256(ENGINE)
        encoded = base64.urlsafe_b64encode(digest.digest()).rstrip(b"=").decode("ascii")
        row = f"{member},sha256={encoded},{len(ENGINE)}\n"
        with patch.object(sys, "path", [str(self.root), *sys.path]):
            record.write_text(row)
            self.assertEqual(check.recorded_engine(member, digest.hexdigest(), self.root), engine)
            for bad in ("", row * 2, row.replace(encoded, "wrong"), row.replace(f",{len(ENGINE)}\n", ",0\n")):
                record.write_text(bad)
                with self.assertRaisesRegex(ValueError, "native RECORD"):
                    check.recorded_engine(member, digest.hexdigest(), self.root)
            record.write_text(row)
            with self.assertRaisesRegex(ValueError, "release archive"):
                check.recorded_engine(member, "0" * 64, self.root)

    @unittest.skipUnless(os.environ.get("DCTX_TEST_BIN"), "Set DCTX_TEST_BIN for the native offline check")
    def test_native_offline_install_and_real_languages_rpc(self):
        binary = Path(os.environ["DCTX_TEST_BIN"]).resolve(strict=True)
        machine = platform.machine().lower()
        arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
        # The local Cargo CLI can be GNU-linked. Use the development Linux tag for this fixture.
        target = {"linux": f"{arch}-unknown-linux-gnu",
                  "darwin": f"{arch}-apple-darwin", "win32": WINDOWS}[sys.platform]
        build = subprocess.run(
            [sys.executable, "-B", str(check.ROOT / "packages/python/tools/build_wheels.py"),
             "--output", str(self.house), "--binary", str(binary), "--target", target],
            capture_output=True, timeout=120,
        )
        self.assertEqual(build.returncode, 0, ascii(build.stderr))
        exe = "dctx.exe" if target == WINDOWS else "dctx"
        archive = self.root / f"dctx-{target}.{'zip' if target == WINDOWS else 'tar.gz'}"
        data = binary.read_bytes()
        write_members(archive, [(exe, data, stat.S_IFREG)])
        result = self.checker(target)
        self.assertEqual(result.returncode, 0, ascii(result.stderr))
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt["rpc"], "languages")
        self.assertEqual(receipt["engine_sha256"], hashlib.sha256(data).hexdigest())
        self.assertEqual(receipt["wheels"], {p.name: check.sha256(p).hexdigest() for p in self.house.glob("*.whl")})

        # Keep the real engine and version, changing just trailing bytes in its release archive.
        write_members(archive, [(exe, data + b"\x00", stat.S_IFREG)])
        result = self.checker(target)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"engine bytes differ", result.stderr)


if __name__ == "__main__":
    unittest.main()
