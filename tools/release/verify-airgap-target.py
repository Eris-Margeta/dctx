#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Verify a selected air-gap tarball in the current directory before extraction."""
import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path


# Keep this offline verifier standalone for transfer. These are the CLI release targets.
TARGETS = (
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
)
WORKFLOW = "https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/"
ISSUER = "https://token.actions.githubusercontent.com"


class Parser(argparse.ArgumentParser):
    def error(self, message):
        self.exit(2, f"argument error: {json.dumps(message)}\n")


def verify_signature(blob, root, identity):
    result = subprocess.run(
        ["cosign", "verify-blob", "--trusted-root", str(root),
         "--bundle", blob.name + ".sigstore.json", "--certificate-identity", identity,
         "--certificate-oidc-issuer", ISSUER, blob.name],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    )
    if result.returncode:
        detail = result.stderr.decode("utf-8", errors="replace")
        raise ValueError(f"cosign rejected {blob.name} (exit {result.returncode}): {detail}")


def selected_digest(manifest, basename):
    selected = None
    with manifest.open("rb") as rows:
        for row in rows:
            match = re.fullmatch(rb"([0-9a-f]{64})  (.+)", row.removesuffix(b"\n"))
            if not match:
                raise ValueError("malformed SHA256SUMS row")
            if match[2] == basename.encode("ascii"):
                if selected is not None:
                    raise ValueError(f"duplicate SHA256SUMS row for {basename}")
                selected = match[1].decode("ascii")
    if selected is None:
        raise ValueError(f"missing SHA256SUMS row for {basename}")
    return selected


def main():
    parser = Parser(prog="verify-airgap-target.py", description=__doc__, allow_abbrev=False)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--tag", required=True, help="independently selected CLI release tag")
    parser.add_argument("--trusted-root", type=Path, required=True, help="authenticated Sigstore root file")
    args = parser.parse_args()
    if not re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?", args.tag):
        parser.error("expected a CLI release tag such as v1.3.2")

    manifest = Path("SHA256SUMS")
    outer = Path(f"dctx-{args.target}-airgap.tar.gz")
    identity = WORKFLOW + args.tag
    try:
        root = args.trusted_root.absolute()
        verify_signature(manifest, root, identity)
        verify_signature(outer, root, identity)
        expected = selected_digest(manifest, outer.name)
        with outer.open("rb") as stream:
            if hashlib.file_digest(stream, "sha256").hexdigest() != expected:
                raise ValueError(f"SHA256SUMS digest mismatch for {outer.name}")
    except (OSError, ValueError) as error:
        print(f"verification failed: {json.dumps(str(error))}", file=sys.stderr)
        return 1
    print(f"Verified {json.dumps(outer.name)} for {json.dumps(args.tag)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
