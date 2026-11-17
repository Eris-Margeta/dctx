#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Build the CLI twice offline in fresh RAM/cache targets and emit a provenance manifest."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile

TOOLCHAIN = "1.100.0"
TARGET = "x86_64-unknown-linux-gnu"
DEFAULT_EPOCH = "1794873600"


def run(args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def source_tree(root):
    """Hash build inputs, including dirty and untracked files, without host paths or mtimes."""
    names = subprocess.check_output(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=root
    ).split(b"\0")
    digest = hashlib.sha256(b"dctx-build-tree/1\0")
    count = 0
    for raw in sorted(set(names) - {b""}):
        name = os.fsdecode(raw)
        if not (
            name in {"Cargo.toml", "Cargo.lock", "VERSION", "scripts/reproducible_build.py",
                     "apps/dctx-desktop/src-tauri/Cargo.toml"}
            or name.startswith(("crates/", "plugins/", "schemas/", ".cargo/", "examples/library/"))
        ):
            continue
        path = root / name
        if not path.exists():
            continue
        if path.is_symlink():
            raise SystemExit(f"build input symlink is unsupported: {name}")
        data = path.read_bytes()
        digest.update(len(raw).to_bytes(8, "big"))
        digest.update(raw)
        digest.update((path.stat().st_mode & 0o777).to_bytes(4, "big"))
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
        count += 1
    return {"algorithm": "dctx-build-tree/1", "sha256": digest.hexdigest(), "files": count}


def main():
    os.umask(0o022)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path,
                        help="result directory under the existing CARGO_TARGET_DIR")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    base = Path(os.environ["CARGO_TARGET_DIR"]).resolve()
    output = args.output.resolve()
    if not output.is_relative_to(base) or output == base:
        parser.error("--output must be a subdirectory of CARGO_TARGET_DIR")
    # Cargo --frozen forbids dependency fetches. The namespace additionally removes
    # external network access for rustup, compiler processes and dependency build scripts.
    if os.environ.get("DCTX_REPRO_NETNS") != "1":
        if sys.platform != "linux" or not shutil.which("bwrap"):
            parser.error("this proof recipe requires Linux and bubblewrap")
        env = os.environ.copy()
        env["DCTX_REPRO_NETNS"] = "1"
        subprocess.run(["bwrap", "--die-with-parent", "--unshare-net", "--bind", "/", "/",
                        "--dev", "/dev", "--proc", "/proc", sys.executable, str(Path(__file__).resolve()),
                        "--output", str(output)], env=env, check=True)
        return
    if {name for _, name in socket.if_nameindex()} - {"lo"}:
        raise SystemExit("offline proof requires an isolated network namespace")
    output.mkdir(parents=True, exist_ok=True)
    epoch = os.environ.get("SOURCE_DATE_EPOCH", DEFAULT_EPOCH)
    if not epoch.isdecimal():
        parser.error("SOURCE_DATE_EPOCH must be a nonnegative integer")
    before = source_tree(root)
    lock = sha256(root / "Cargo.lock")
    commit = run(["git", "rev-parse", "HEAD"], cwd=root)
    env = os.environ.copy()
    for key in list(env):
        if key.startswith(("CARGO_PROFILE_", "CARGO_TARGET_")) or key in {
            "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"
        }:
            env.pop(key)
    env.update(SOURCE_DATE_EPOCH=epoch, CARGO_INCREMENTAL="0", CARGO_NET_OFFLINE="true",
               CARGO_BUILD_JOBS="2", DCTX_GIT_SHA=commit, RUSTUP_TOOLCHAIN=TOOLCHAIN,
               CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="cc")
    toolchain = run(["rustc", f"+{TOOLCHAIN}", "-Vv"], env=env)
    flags = [f"--remap-path-prefix={root}=/dctx/source",
             f"--remap-path-prefix={Path(os.environ['CARGO_HOME']).resolve()}=/dctx/cargo",
             "-C", "strip=symbols", "-C", "link-arg=-Wl,--build-id=none"]
    hashes = []
    artifact = output / "dctx-cli"
    for number in range(2):
        with tempfile.TemporaryDirectory(prefix="pack-repro-", dir=base) as fresh:
            build_env = env.copy()
            build_env["CARGO_TARGET_DIR"] = fresh
            build_env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(
                flags + [f"--remap-path-prefix={fresh}=/dctx/target"]
            )
            subprocess.run(["cargo", f"+{TOOLCHAIN}", "build", "--frozen", "--offline",
                            "--release", "--target", TARGET, "-p", "dctx-cli",
                            "--bin", "dctx-cli"], cwd=root, env=build_env, check=True)
            binary = Path(fresh) / TARGET / "release/dctx-cli"
            hashes.append(sha256(binary))
            if number == 0:
                # Move the deliverable; temporary compiler intermediates are regenerable.
                os.replace(binary, artifact)
            elif hashes[0] != hashes[1]:
                os.replace(binary, output / "dctx-cli-second")
    if source_tree(root) != before or sha256(root / "Cargo.lock") != lock:
        raise SystemExit("build inputs changed during the proof")
    manifest = {
        "schema": "dctx.build/1", "toolchain": toolchain, "target": TARGET,
        "profile": "release", "features": "default", "source_date_epoch": int(epoch),
        "source_commit": commit, "source_tree": before, "lockfile_sha256": lock,
        "linker": run(["cc", "--version"]).splitlines()[0],
        "libc": run(["ldd", "--version"]).splitlines()[0],
        "linker_backend": run(["ld", "--version"]).splitlines()[0],
        "rustflags": ["--remap-path-prefix=SOURCE=/dctx/source",
                      "--remap-path-prefix=CARGO_HOME=/dctx/cargo",
                      "--remap-path-prefix=FRESH_TARGET=/dctx/target",
                      "-C strip=symbols", "-C link-arg=-Wl,--build-id=none"],
        "network": "isolated namespace; loopback only; cargo --frozen --offline",
        "clean_builds": 2, "artifact_sha256": hashes, "reproducible": hashes[0] == hashes[1],
        "artifact": {"file": "dctx-cli", "sha256": hashes[0], "bytes": artifact.stat().st_size},
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"reproducible": manifest["reproducible"], "artifact_sha256": hashes}))
    if not manifest["reproducible"]:
        raise SystemExit("artifact hashes differ; inspect the two hashes in manifest.json")


if __name__ == "__main__":
    main()
