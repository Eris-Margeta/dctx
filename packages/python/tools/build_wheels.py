#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Build wrapper and platform wheels locally, without downloads or publication."""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
import tomllib
from pathlib import Path

PACKAGE = Path(__file__).resolve().parents[1]
ROOT = PACKAGE.parents[1]
sys.path.insert(0, str(PACKAGE))
import build_backend  # noqa: E402

# Static Linux release binaries run on glibc and musl. GNU development wheels
# deliberately use generic linux tags, without claiming a portable libc floor.
TARGETS = {
    "x86_64-unknown-linux-musl": ("linux_x86_64", ("manylinux_2_17_x86_64", "musllinux_1_2_x86_64")),
    "aarch64-unknown-linux-musl": ("linux_aarch64", ("manylinux_2_17_aarch64", "musllinux_1_2_aarch64")),
    "x86_64-unknown-linux-gnu": ("linux_x86_64", ("linux_x86_64",)),
    "aarch64-unknown-linux-gnu": ("linux_aarch64", ("linux_aarch64",)),
    "x86_64-apple-darwin": ("darwin_x86_64", ("macosx_10_13_x86_64",)),
    "aarch64-apple-darwin": ("darwin_aarch64", ("macosx_11_0_arm64",)),
    "x86_64-pc-windows-msvc": ("windows_x86_64", ("win_amd64",)),
}


def validate_binary(data: bytes, target: str) -> None:
    """Refuse a mislabeled architecture or dynamic Linux release payload."""
    if "linux" in target:
        if data[:6] != b"\x7fELF\x02\x01":
            raise ValueError("Expected a little-endian ELF64 binary")
        machine = struct.unpack_from("<H", data, 18)[0]
        if machine != (183 if target.startswith("aarch64") else 62):
            raise ValueError("Binary architecture does not match target")
        if target.endswith("musl"):
            offset = struct.unpack_from("<Q", data, 32)[0]
            size, count = struct.unpack_from("<HH", data, 54)
            for index in range(count):
                header = offset + index * size
                kind = struct.unpack_from("<I", data, header)[0]
                if kind == 3:
                    raise ValueError("Portable Linux wheels require a static binary (PT_INTERP found)")
                if kind == 2:
                    start = struct.unpack_from("<Q", data, header + 8)[0]
                    length = struct.unpack_from("<Q", data, header + 32)[0]
                    for entry in range(start, start + length, 16):
                        if struct.unpack_from("<Q", data, entry)[0] == 1:
                            raise ValueError("Portable Linux wheels cannot contain DT_NEEDED dependencies")
    elif "apple" in target:
        if data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("Expected a little-endian Mach-O 64-bit binary")
        cpu = struct.unpack_from("<I", data, 4)[0]
        if cpu != (0x0100000C if target.startswith("aarch64") else 0x01000007):
            raise ValueError("Binary architecture does not match target")
    else:
        if data[:2] != b"MZ":
            raise ValueError("Expected a PE executable")
        offset = struct.unpack_from("<I", data, 60)[0]
        if data[offset:offset + 4] != b"PE\0\0" or struct.unpack_from("<H", data, offset + 4)[0] != 0x8664:
            raise ValueError("Expected an x86_64 PE executable")


def build(output: Path, binary: Path | None = None, target: str | None = None) -> list[Path]:
    subprocess.run([sys.executable, str(PACKAGE / "tools/generate_api.py"), "--check"], check=True)
    config = build_backend.project()
    version = config["version"]
    engine_version = tomllib.loads((ROOT / "crates/dctx-cli/Cargo.toml").read_text())["package"]["version"]
    if version != (ROOT / "VERSION").read_text().strip() or version != engine_version:
        raise ValueError("Python package, VERSION and engine manifest must match exactly")
    if (binary is None) != (target is None):
        raise ValueError("--binary and --target must be supplied together")
    payload = None
    if binary is not None:
        binary = binary.resolve(strict=True)
        payload = binary.read_bytes()
        assert target is not None
        validate_binary(payload, target)
        hello = subprocess.run([str(binary), "rpc"], input=b'{"id":0,"verb":"hello"}\n',
                               stdout=subprocess.PIPE, check=True, timeout=30)
        reply = json.loads(hello.stdout)
        if (reply.get("engine_version") != version or reply.get("id") != 0
                or reply.get("result", {}).get("protocol_version", "").split(".")[0] != "1"):
            raise ValueError(f"Binary RPC handshake does not match package {version}")
    wheels = [output / build_backend.build_wheel(str(output))]
    if payload is not None:
        key, tags = TARGETS[target]
        module = f"dctx_binary_{key}"
        distribution = module.replace("_", "-")
        executable = "dctx.exe" if key.startswith("windows") else "dctx"
        files = {f"{module}/__init__.py": (b"# SPDX-License-Identifier: Apache-2.0\n", 0o644),
                 f"{module}/{executable}": (payload, 0o755)}
        wheels.append(output / build_backend.write_wheel(
            output, distribution, version, tags, files,
            build_backend.metadata(distribution, version, f"dctx {version} engine for {key}"), pure=False))
    return wheels


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=PACKAGE / "dist")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--target", choices=TARGETS)
    args = parser.parse_args()
    try:
        wheels = build(args.output, args.binary, args.target)
    except (OSError, ValueError, struct.error, subprocess.SubprocessError) as exc:
        parser.exit(1, f"Wheel build refused: {exc}\n")
    for wheel in wheels:
        print(wheel)


if __name__ == "__main__":
    main()
