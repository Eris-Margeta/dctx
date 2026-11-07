#!/usr/bin/env python3
"""Fill npm packages from checksummed CLI release archives. No network access."""
import argparse
import hashlib
import json
import re
import shutil
import tarfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = {
    "linux-x64": "x86_64-unknown-linux-musl",
    "linux-arm64": "aarch64-unknown-linux-musl",
    "darwin-x64": "x86_64-apple-darwin",
    "darwin-arm64": "aarch64-apple-darwin",
    "win32-x64": "x86_64-pc-windows-msvc",
}


def prepare(tag, artifacts, output):
    if not re.fullmatch(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", tag):
        raise ValueError("tag must be vX.Y.Z or vX.Y.Z-prerelease")
    version = tag[1:]
    checksums = {}
    for line in (artifacts / "SHA256SUMS").read_text().splitlines():
        digest, filename = line.split(maxsplit=1)
        checksums[filename.lstrip(" *")] = digest
    # Validate all archives and read just the executable, never extract arbitrary paths.
    binaries = {}
    for platform, target in PLATFORMS.items():
        windows = platform.startswith("win32")
        executable = "dctx.exe" if windows else "dctx"
        filename = f"dctx-{target}.{'zip' if windows else 'tar.gz'}"
        archive = artifacts / filename
        if hashlib.sha256(archive.read_bytes()).hexdigest() != checksums.get(filename):
            raise ValueError(f"checksum mismatch or missing SHA256SUMS entry: {filename}")
        if windows:
            with zipfile.ZipFile(archive) as package:
                binary = package.read(executable)
        else:
            with tarfile.open(archive, "r:gz") as package:
                member = package.getmember(executable)
                if not member.isfile():
                    raise ValueError(f"{filename}: executable is not a regular file")
                binary = package.extractfile(member).read()
        if not binary:
            raise ValueError(f"{filename}: empty executable")
        binaries[platform] = (executable, binary)

    for directory in ["dctx", *(f"cli-{p}" for p in PLATFORMS)]:
        source = ROOT / "packaging/npm" / directory
        destination = output / directory
        if source.resolve() != destination.resolve():
            shutil.copytree(source, destination, dirs_exist_ok=True)
        manifest_path = destination / "package.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["version"] = version
        if directory == "dctx":
            manifest["optionalDependencies"] = {f"@dctx/cli-{p}": version for p in PLATFORMS}
        else:
            executable, binary = binaries[directory.removeprefix("cli-")]
            path = destination / "bin" / executable
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(binary)
            path.chmod(0o755)
        for filename in ("LICENSE", "NOTICE"):
            shutil.copyfile(ROOT / filename, destination / filename)
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"Prepared {manifest['name']}@{version}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag")
    parser.add_argument("artifacts", type=Path, help="directory with five archives and SHA256SUMS")
    parser.add_argument("--out-dir", type=Path, default=ROOT / "packaging/npm")
    args = parser.parse_args()
    prepare(args.tag, args.artifacts, args.out_dir)
