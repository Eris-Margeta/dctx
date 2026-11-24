#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check the release archive, upload candidates, pip receipt and installed RPC engine."""

import argparse
import base64
import csv
import hashlib
import io
import json
import os
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from contextlib import contextmanager
from importlib import metadata
from pathlib import Path, PurePosixPath
from urllib.parse import urlsplit
from urllib.request import url2pathname

SCRIPT = Path(__file__).resolve()
ROOT = SCRIPT.parents[2]


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256")


def unique_member(entries, expected):
    basename = PurePosixPath(expected).name
    names = [name for name in entries if PurePosixPath(name).name == basename]
    if names != [expected]:
        raise ValueError(f"Expected one regular {expected!a} member, found {names!a}")


@contextmanager
def engine_member(path, expected):
    if path.suffix == ".gz":
        with tarfile.open(path, "r:gz") as archive:
            unique_member(archive.getnames(), expected)
            entry = archive.getmember(expected)
            if not entry.isfile():
                raise ValueError(f"Non-regular tar member {expected!a}")
            with archive.extractfile(entry) as stream:
                yield stream
    else:
        with zipfile.ZipFile(path) as archive:
            unique_member(archive.namelist(), expected)
            entry = archive.getinfo(expected)
            if entry.is_dir() or not stat.S_ISREG(entry.external_attr >> 16):
                raise ValueError(f"Non-regular ZIP member {expected!a}")
            with archive.open(entry) as stream:
                yield stream


def compare_engines(archive, native, member):
    digest = hashlib.sha256()
    with engine_member(archive, PurePosixPath(member).name) as release, engine_member(native, member) as wheel:
        while True:
            chunk = release.read(1024 * 1024)
            if chunk != wheel.read(1024 * 1024):
                raise ValueError("Release archive and wheel engine bytes differ")
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def upload_pair(wheelhouse, module, tags, version):
    wrapper = wheelhouse / f"dctx-{version}-py3-none-any.whl"
    native = wheelhouse / f"{module}-{version}-py3-none-{'.'.join(tags)}.whl"
    candidates = set(wheelhouse.glob("*.whl"))
    if candidates != {wrapper, native} or any(p.is_symlink() or not p.is_file() for p in candidates):
        raise ValueError(f"Unexpected upload candidates {sorted(p.name for p in candidates)!a}")
    return wrapper, native


def check_selection(report, wheels):
    remaining = wheels.copy()
    for install in report["install"]:
        download = install["download_info"]
        url = urlsplit(download["url"])
        if url.scheme != "file" or url.netloc or url.query or url.fragment:
            raise ValueError("pip selected a non-local wheel or an unexpected file URL")
        path = Path(url2pathname(url.path))
        if path not in remaining:
            raise ValueError(f"Unexpected pip selection {str(path)!a}")
        if download["archive_info"]["hashes"]["sha256"] != remaining.pop(path):
            raise ValueError(f"pip selected different wheel bytes for {path.name!a}")
    if remaining:
        raise ValueError(f"Missing pip selections {sorted(p.name for p in remaining)!a}")


def recorded_engine(member, archive_hash, prefix):
    distribution = metadata.distribution(member.split("/")[0].replace("_", "-"))
    record = distribution.read_text("RECORD")
    if record is None:
        raise ValueError("Missing installed native RECORD")
    rows = [row for row in csv.reader(io.StringIO(record)) if row and row[0] == member]
    engine = Path(distribution.locate_file(member)).resolve(strict=True)
    if not engine.is_relative_to(prefix):
        raise ValueError("Recorded native engine is outside the venv")
    digest = sha256(engine)
    encoded = base64.urlsafe_b64encode(digest.digest()).rstrip(b"=").decode("ascii")
    if rows != [[member, "sha256=" + encoded, str(engine.stat().st_size)]]:
        raise ValueError("Installed engine disagrees with native RECORD")
    if digest.hexdigest() != archive_hash:
        raise ValueError("Installed engine bytes differ from release archive")
    return engine


def check_installed(member, archive_hash):
    # This function runs under the venv's isolated interpreter, outside the checkout's import path.
    import dctx

    prefix = Path(sys.prefix).resolve()
    if not Path(dctx.__file__).resolve().is_relative_to(prefix):
        raise ValueError("Wrapper imported from outside the venv")
    engine = recorded_engine(member, archive_hash, prefix)
    with dctx.Client(Path.cwd()) as client:
        if client.binary != engine:
            raise ValueError("Client selected an engine other than the native RECORD member")
        languages = json.loads(client.languages())["languages"]
        if not isinstance(languages, list) or not languages:
            raise ValueError("Installed engine returned an empty or invalid languages page")


def run_checked(args, cwd, env):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=120)
    if result.returncode:
        raise ValueError(f"Command {args!a} exited {result.returncode}, "
                         f"stdout {result.stdout!a}, stderr {result.stderr!a}")


def check_install(root, wheels, version, member, archive_hash):
    env = {k: v for k, v in os.environ.items() if not k.startswith("PIP_") and k != "DCTX_BIN"}
    for key in ("HOME", "USERPROFILE", "LOCALAPPDATA", "XDG_CACHE_HOME", "DCTX_CACHE_DIR",
                "DCTX_MAP_CACHE", "TMPDIR", "TEMP", "TMP"):
        env[key] = str(root)
    env["PIP_CONFIG_FILE"] = os.devnull
    decoys = root / "path"
    decoys.mkdir()
    decoy = decoys / PurePosixPath(member).name
    if os.name == "nt":
        shutil.copyfile(sys.executable, decoy)  # A real PE that cannot answer a dctx RPC.
    else:
        decoy.write_bytes(b"#!/bin/sh\nexit 1\n")
        decoy.chmod(0o755)
    env["PATH"] = str(decoys) + os.pathsep + env.get("PATH", "")

    venv = root / "venv"
    run_checked([sys.executable, "-I", "-B", "-m", "venv", str(venv)], root, env)
    python = venv / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    isolated = [str(python), "-I", "-B", str(SCRIPT)]
    report = root / "install.json"
    wheelhouse = next(iter(wheels)).parent
    run_checked([*isolated, "_pip", "install", "--no-index", "--no-cache-dir", "--only-binary=:all:",
                 "--disable-pip-version-check", "--find-links", str(wheelhouse), "--report", str(report),
                 f"dctx[binary]=={version}"], root, env)
    check_selection(json.loads(report.read_text(encoding="utf-8")), wheels)
    run_checked([*isolated, "_installed", member, archive_hash], root, env)


def deny_network(event, args):
    if event in ("socket.connect", "socket.getaddrinfo", "socket.bind"):
        raise RuntimeError("Offline wheel check attempted network access")


def main():
    # The two child modes only import from the standard library and installed wheels.
    if sys.argv[1:2] == ["_installed"]:
        sys.addaudithook(deny_network)
        check_installed(*sys.argv[2:])
        return
    if sys.argv[1:2] == ["_pip"]:
        import runpy

        sys.addaudithook(deny_network)
        sys.argv = ["pip", *sys.argv[2:]]
        runpy.run_module("pip", run_name="__main__")
        return

    sys.path.insert(0, str(ROOT / "packages/python/tools"))
    from build_wheels import TARGETS

    class Parser(argparse.ArgumentParser):
        def error(self, message):
            raise ValueError(message)

    parser = Parser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--archives", type=Path, required=True)
    parser.add_argument("--wheelhouse", type=Path, required=True)
    args = parser.parse_args()
    key, tags = TARGETS[args.target]
    module = "dctx_binary_" + key
    windows = key.startswith("windows_")
    member = module + ("/dctx.exe" if windows else "/dctx")
    archive = args.archives / f"dctx-{args.target}.{'zip' if windows else 'tar.gz'}"
    version = (ROOT / "VERSION").read_text(encoding="ascii").strip()
    wrapper, native = upload_pair(args.wheelhouse.resolve(), module, tags, version)
    archive_hash = compare_engines(archive, native, member)
    wheels = {path: sha256(path).hexdigest() for path in (wrapper, native)}
    with tempfile.TemporaryDirectory(dir=ROOT) as directory:
        check_install(Path(directory), wheels, version, member, archive_hash)
    print(json.dumps({"target": args.target, "archive": archive.name, "member": member,
                      "engine_sha256": archive_hash, "wheels": {p.name: h for p, h in wheels.items()},
                      "rpc": "languages"}))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        sys.exit(f"Wheel install check failed: {str(error)!a}")
