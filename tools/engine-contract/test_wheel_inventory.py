# SPDX-License-Identifier: Apache-2.0
"""Check wheel claims on the build host using synthetic executable headers.

The real input checks inspect format and CPU, and look for PT_INTERP and
DT_NEEDED in portable ELF inputs. ZIP inspection binds the tagged wheel to the
supplied bytes, member path, mode, and metadata. The synthetic RPC reply gives
no evidence of execution on a foreign target or at an advertised OS/libc floor.
"""

import base64
import csv
import hashlib
import io
import json
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile
from email.parser import BytesHeaderParser
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
VERSION = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
original_path = sys.path.copy()
try:
    sys.path.insert(0, str(ROOT / "packages/python/tools"))
    import build_wheels
finally:
    sys.path[:] = original_path


def elf(machine, *, needed=False):
    # ELF64 Ehdr, one Phdr, then (for PT_DYNAMIC) DT_NEEDED and DT_NULL.
    data = bytearray(64 + 56 + 32)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQQIHHH", data, 16, 2, machine, 1, 0, 64, 0, 0, 64, 56, 1)
    struct.pack_into("<IIQQQQQQ", data, 64, 2 if needed else 1, 4, 120, 0, 0, 32, 32, 8)
    if needed:
        struct.pack_into("<QQ", data, 120, 1, 1)
    return bytes(data)


def macho(cpu):
    return struct.pack("<8I", 0xFEEDFACF, cpu, 0, 2, 0, 0, 0, 0)


def pe(machine):
    data = bytearray(64 + 4 + 20)
    data[:2] = b"MZ"
    struct.pack_into("<I4sH", data, 60, 64, b"PE\0\0", machine)
    return bytes(data)


# These are packaging expectations, independent of the builder and manifest.
# Each tuple specifies the expanded platform tags, sole executable and input.
TARGETS = {
    "x86_64-unknown-linux-musl": (
        ("manylinux_2_17_x86_64", "musllinux_1_2_x86_64"), "dctx_binary_linux_x86_64/dctx", elf(62)),
    "aarch64-unknown-linux-musl": (
        ("manylinux_2_17_aarch64", "musllinux_1_2_aarch64"), "dctx_binary_linux_aarch64/dctx", elf(183)),
    "x86_64-apple-darwin": (("macosx_10_13_x86_64",), "dctx_binary_darwin_x86_64/dctx", macho(0x01000007)),
    "aarch64-apple-darwin": (("macosx_11_0_arm64",), "dctx_binary_darwin_aarch64/dctx", macho(0x0100000C)),
    "x86_64-pc-windows-msvc": (("win_amd64",), "dctx_binary_windows_x86_64/dctx.exe", pe(0x8664)),
    "x86_64-unknown-linux-gnu": (("linux_x86_64",), "dctx_binary_linux_x86_64/dctx", elf(62, needed=True)),
    "aarch64-unknown-linux-gnu": (("linux_aarch64",), "dctx_binary_linux_aarch64/dctx", elf(183, needed=True)),
}

BINARY_REQUIREMENTS = sorted(
    f'{name}=={VERSION}; (sys_platform == \'{system}\' and platform_machine == \'{machine}\') and extra == "binary"'
    for name, system, machine in (
        ("dctx-binary-linux-x86-64", "linux", "x86_64"),
        ("dctx-binary-linux-aarch64", "linux", "aarch64"),
        ("dctx-binary-darwin-x86-64", "darwin", "x86_64"),
        ("dctx-binary-darwin-aarch64", "darwin", "arm64"),
        ("dctx-binary-windows-x86-64", "win32", "AMD64"),
    )
)


def build_pair(directory, target, payload):
    binary = directory / "engine"
    binary.write_bytes(payload)
    real_run = subprocess.run

    def run(command, **kwargs):
        if command == [str(binary), "rpc"]:
            hello = {"id": 0, "engine_version": VERSION, "result": {"protocol_version": "1.0"}}
            return subprocess.CompletedProcess(command, 0, stdout=json.dumps(hello).encode())
        return real_run(command, **kwargs)

    with patch.object(build_wheels.subprocess, "run", side_effect=run):
        return build_wheels.build(directory, binary, target)


def check_record(archive, record):
    remaining = set()
    for name in archive.namelist():
        if name in remaining:
            raise AssertionError(f"Duplicate ZIP member {json.dumps(name)}")
        remaining.add(name)
    if record not in remaining:
        raise AssertionError(f"Missing RECORD member {json.dumps(record)}")

    with archive.open(record) as stream, io.TextIOWrapper(stream, encoding="utf-8", newline="") as csv_file:
        for row in csv.reader(csv_file, strict=True):
            if len(row) != 3:
                raise AssertionError(f"Malformed RECORD row {json.dumps(row)}")
            name, digest, size = row
            if name not in remaining:
                raise AssertionError(f"Duplicate or unknown RECORD path {json.dumps(name)}")
            remaining.remove(name)
            if name == record:
                if digest or size:
                    raise AssertionError(f"Nonblank RECORD self-row {json.dumps(name)}")
                continue
            data = archive.read(name)
            encoded = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode("ascii")
            if digest != f"sha256={encoded}" or size != str(len(data)):
                raise AssertionError(f"RECORD digest or size mismatch for {json.dumps(name)}")
    if remaining:
        raise AssertionError(f"Missing RECORD rows for {json.dumps(sorted(remaining))}")


def rewrite_member(wheel, name, data):
    """Repack one changed or additional member with a valid ZIP CRC."""
    output = io.BytesIO()
    with zipfile.ZipFile(wheel) as source, zipfile.ZipFile(output, "w") as dest:
        for info in source.infolist():
            dest.writestr(info, data if info.filename == name else source.read(info))
        if name not in source.namelist():
            dest.writestr(name, data)
    output.seek(0)
    return output


class WheelInventoryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        tmp = tempfile.TemporaryDirectory(dir=ROOT)
        cls.addClassCleanup(tmp.cleanup)
        cls.root = Path(tmp.name)
        cls.wheels = {}
        for target, (_, _, payload) in TARGETS.items():
            directory = cls.root / target
            directory.mkdir()
            cls.wheels[target] = build_pair(directory, target, payload)

    def check_filename(self, filename, module, platforms):
        # Expand the dot-compressed platform field. Both tag lists are unordered.
        prefix = f"{module}-{VERSION}-py3-none-"
        self.assertTrue(filename.startswith(prefix) and filename.endswith(".whl"),
                        f"Unexpected wheel filename {json.dumps(filename)}")
        self.assertCountEqual(filename[len(prefix):-4].split("."), platforms)

    def check_wheel_headers(self, archive, dist_info, platforms, pure):
        headers = BytesHeaderParser().parsebytes(archive.read(f"{dist_info}/WHEEL"))
        self.assertEqual(headers.get_all("Root-Is-Purelib"), [pure])
        self.assertCountEqual(headers.get_all("Tag", []), [f"py3-none-{p}" for p in platforms])

    def check_wheel(self, wheel, module, platforms, executable=None, payload=None):
        self.check_filename(wheel.name, module, platforms)
        dist_info = f"{module}-{VERSION}.dist-info"
        with zipfile.ZipFile(wheel) as archive:
            check_record(archive, f"{dist_info}/RECORD")
            self.check_wheel_headers(archive, dist_info, platforms, "true" if executable is None else "false")
            metadata = BytesHeaderParser().parsebytes(archive.read(f"{dist_info}/METADATA"))
            self.assertEqual(metadata.get_all("Name"), [module.replace("_", "-")])
            self.assertEqual(metadata.get_all("Version"), [VERSION])
            self.assertEqual(metadata.get_all("Provides-Extra", []), ["binary"] if executable is None else [])
            self.assertEqual(sorted(metadata.get_all("Requires-Dist", [])),
                             BINARY_REQUIREMENTS if executable is None else [])
            candidates = [i.filename for i in archive.infolist()
                          if (i.external_attr >> 16) & 0o111 or i.filename.rsplit("/", 1)[-1] in ("dctx", "dctx.exe")]
            self.assertEqual(candidates, [executable] if executable else [])
            if executable:
                self.assertEqual(archive.getinfo(executable).external_attr >> 16, 0o100755)
                self.assertEqual(archive.read(executable), payload)

    def test_built_wheel_pairs(self):
        for target, (platforms, executable, payload) in TARGETS.items():
            with self.subTest(target=target):
                wrapper, native = self.wheels[target]
                self.check_wheel(wrapper, "dctx", ("any",))
                self.check_wheel(native, executable.split("/")[0], platforms, executable, payload)

    def test_missing_duplicate_and_nonblank_record_rows(self):
        for wheel in self.wheels["x86_64-pc-windows-msvc"]:
            with zipfile.ZipFile(wheel) as archive:
                record, = (n for n in archive.namelist() if n.endswith(".dist-info/RECORD"))
                rows = list(csv.reader(io.StringIO(archive.read(record).decode(), newline="")))
            ordinary = next(row for row in rows if row[0] != record)
            self_row = [record, "", ""]
            for label, changed, error in (
                ("missing", [r for r in rows if r != ordinary], "Missing RECORD rows"),
                ("duplicate", rows + [ordinary], "Duplicate or unknown RECORD path"),
                ("missing self", [r for r in rows if r != self_row], "Missing RECORD rows"),
                ("duplicate self", rows + [self_row], "Duplicate or unknown RECORD path"),
                ("nonblank self", [[record, "", "0"] if r == self_row else r for r in rows], "Nonblank RECORD"),
                ("wrong size", [[r[0], r[1], "-1"] if r == ordinary else r for r in rows], "digest or size"),
            ):
                with self.subTest(wheel=wheel.name, mutation=label):
                    output = io.StringIO(newline="")
                    csv.writer(output).writerows(changed)
                    with zipfile.ZipFile(rewrite_member(wheel, record, output.getvalue().encode())) as archive:
                        with self.assertRaisesRegex(AssertionError, error):
                            check_record(archive, record)

    def test_damaged_native_bytes_fail_record_with_size_preserved(self):
        target = "aarch64-apple-darwin"
        _, wheel = self.wheels[target]
        _, executable, payload = TARGETS[target]
        damaged = payload[:-1] + bytes([payload[-1] ^ 1])
        record = f"dctx_binary_darwin_aarch64-{VERSION}.dist-info/RECORD"
        with zipfile.ZipFile(rewrite_member(wheel, executable, damaged)) as archive:
            with self.assertRaisesRegex(AssertionError, "RECORD digest or size mismatch"):
                check_record(archive, record)

    def test_wrong_additional_platform_tag(self):
        target = "x86_64-unknown-linux-gnu"
        _, wheel = self.wheels[target]
        platforms, executable, _ = TARGETS[target]
        module = executable.split("/")[0]
        with self.assertRaises(AssertionError):
            self.check_filename(wheel.stem + ".manylinux_2_17_x86_64.whl", module, platforms)
        dist_info = f"{module}-{VERSION}.dist-info"
        name = f"{dist_info}/WHEEL"
        with zipfile.ZipFile(wheel) as archive:
            changed = archive.read(name) + b"Tag: py3-none-manylinux_2_17_x86_64\n"
        with zipfile.ZipFile(rewrite_member(wheel, name, changed)) as archive:
            with self.assertRaises(AssertionError):
                self.check_wheel_headers(archive, dist_info, platforms, "false")

    def test_hostile_member_in_missing_row_diagnostic(self):
        wheel, _ = self.wheels["x86_64-pc-windows-msvc"]
        name = 'extra/line\n\x1b"member'
        with zipfile.ZipFile(rewrite_member(wheel, name, b"x")) as archive:
            with self.assertRaises(AssertionError) as caught:
                check_record(archive, f"dctx-{VERSION}.dist-info/RECORD")
        self.assertEqual(str(caught.exception), f"Missing RECORD rows for [{json.dumps(name)}]")

    def test_builder_rejects_wrong_cpu_and_needed_without_interp(self):
        for target, payload, reason in (
            ("aarch64-apple-darwin", macho(0x01000007), "architecture"),
            ("x86_64-apple-darwin", macho(0x0100000C), "architecture"),
            ("x86_64-pc-windows-msvc", pe(0xAA64), "x86_64 PE"),
            ("x86_64-unknown-linux-musl", elf(62, needed=True), "DT_NEEDED"),
        ):
            with self.subTest(target=target), tempfile.TemporaryDirectory(dir=self.root) as directory:
                with self.assertRaisesRegex(ValueError, reason):
                    build_pair(Path(directory), target, payload)


if __name__ == "__main__":
    unittest.main()
