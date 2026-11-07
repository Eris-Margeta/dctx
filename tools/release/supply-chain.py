#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Deterministic CLI archives and signed air-gap payloads (standard library only)."""
import argparse
import gzip
import hashlib
import io
import os
import shutil
import subprocess
import tarfile
import zipfile
from pathlib import Path

TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
)
ROOT = Path(__file__).resolve().parents[2]
ISSUER = "https://token.actions.githubusercontent.com"


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def checksums(directory, names, filename="SHA256SUMS"):
    """Hash an explicit list, never accidentally include an older checksum/signature."""
    (directory / filename).write_text(
        "".join(f"{sha256(directory / name)}  {name}\n" for name in sorted(names)),
        encoding="utf-8",
    )


def archive_name(target):
    return f"dctx-{target}.{'zip' if 'windows' in target else 'tar.gz'}"


def executable(target):
    return "dctx.exe" if "windows" in target else "dctx"


def tar_gz(destination, files, epoch):
    """No host paths, timestamps, ownership, traversal order or gzip filename."""
    with destination.open("wb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0, compresslevel=9) as gz:
            with tarfile.open(fileobj=gz, mode="w", format=tarfile.USTAR_FORMAT) as tar:
                for name, (data, mode) in sorted(files.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(data), mode, epoch
                    info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    tar.addfile(info, io.BytesIO(data))


def archive(target, binary, output, epoch):
    files = {name: ((ROOT / name).read_bytes(), 0o644) for name in ("LICENSE", "NOTICE")}
    files[executable(target)] = (binary.read_bytes(), 0o755)
    output.mkdir(parents=True, exist_ok=True)
    destination = output / archive_name(target)
    if "windows" in target:
        # Stored ZIP avoids host zlib and local-time differences; metadata is explicit.
        with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_STORED) as package:
            for name, (data, mode) in sorted(files.items()):
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = (0o100000 | mode) << 16
                package.writestr(info, data)
    else:
        tar_gz(destination, files, epoch)
    return destination


def sign(path):
    identity = os.environ["DCTX_CERTIFICATE_IDENTITY"]
    subprocess.run(
        ["cosign", "sign-blob", "--yes",
         "--bundle", str(path) + ".sigstore.json", str(path)], check=True,
    )
    # Fail publication if the signing workflow identity or the blob is unexpected.
    subprocess.run(
        ["cosign", "verify-blob", "--bundle",
         str(path) + ".sigstore.json", "--certificate-identity", identity,
         "--certificate-oidc-issuer", ISSUER, str(path)], check=True,
    )


def stage_airgap(artifacts, stage, target):
    directory = stage / target
    directory.mkdir(parents=True, exist_ok=False)
    name = archive_name(target)
    sbom = f"dctx-{target}.cdx.json"
    # Carry the original signed archive as well as the directly installable binary.
    for filename in (name, name + ".sigstore.json", sbom, sbom + ".sigstore.json",
                     "archive-provenance.jsonl"):
        shutil.copyfile(artifacts / filename, directory / filename)
    if "windows" in target:
        with zipfile.ZipFile(artifacts / name) as package:
            contents = {n: package.read(n) for n in (executable(target), "LICENSE", "NOTICE")}
    else:
        with tarfile.open(artifacts / name, "r:gz") as package:
            contents = {}
            for n in (executable(target), "LICENSE", "NOTICE"):
                member = package.getmember(n)
                if not member.isfile():
                    raise ValueError(f"not a regular release member: {n}")
                contents[n] = package.extractfile(member).read()
    for filename, data in contents.items():
        (directory / filename).write_bytes(data)
    shutil.copyfile(ROOT / "docs/VERIFYING.md", directory / "VERIFYING.md")
    # Covers the raw binary, licences, SBOM, archive and embedded provenance.
    checksums(directory, [p.name for p in directory.iterdir() if not p.name.endswith(".sigstore.json")])
    return directory


def pack_airgap(directory, artifacts, target, epoch):
    files = {p.name: (p.read_bytes(), 0o755 if p.name == executable(target) else 0o644)
             for p in directory.iterdir() if p.is_file()}
    destination = artifacts / f"dctx-{target}-airgap.tar.gz"
    tar_gz(destination, files, epoch)
    return destination


def seal(artifacts, stage, epoch):
    # Attestations are generated by the workflow; cosign signs every published payload.
    for target in TARGETS:
        sign(artifacts / archive_name(target))
        sign(artifacts / f"dctx-{target}.cdx.json")
    for target in TARGETS:
        directory = stage_airgap(artifacts, stage, target)
        sign(directory / "SHA256SUMS")
        sign(pack_airgap(directory, artifacts, target, epoch))


def finish(artifacts):
    names = [p.name for p in artifacts.iterdir() if p.is_file() and not p.name.endswith(".sigstore.json")]
    if "SHA256SUMS" in names:
        raise ValueError("refusing to reseal an existing release")
    # Every published payload is signed, including provenance and future added payloads.
    for name in sorted(names):
        path = artifacts / name
        if not Path(str(path) + ".sigstore.json").is_file():
            sign(path)
    checksums(artifacts, names)
    sign(artifacts / "SHA256SUMS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("archive")
    create.add_argument("--target", choices=TARGETS, required=True)
    create.add_argument("--binary", type=Path, required=True)
    create.add_argument("--out", type=Path, required=True)
    create.add_argument("--epoch", type=int, required=True)
    sealing = commands.add_parser("seal")
    sealing.add_argument("--artifacts", type=Path, required=True)
    sealing.add_argument("--stage", type=Path, required=True)
    sealing.add_argument("--epoch", type=int, required=True)
    final = commands.add_parser("finish")
    final.add_argument("--artifacts", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "archive":
        archive(args.target, args.binary, args.out, args.epoch)
    elif args.command == "seal":
        seal(args.artifacts, args.stage, args.epoch)
    else:
        finish(args.artifacts)
