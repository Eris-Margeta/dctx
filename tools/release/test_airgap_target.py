# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Run the documented pre-extraction gate with checksum fixtures and a cosign recorder."""
import hashlib
import io
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
LINUX = "x86_64-unknown-linux-musl"
ARM = "aarch64-unknown-linux-musl"
WINDOWS = "x86_64-pc-windows-msvc"
TAG = "v3.2.1-rc.2"
HOSTILE = 'outside\n\x1b"end'

# Model only the blob/sidecar pairing. No fixture contains signature evidence.
RECORDER = '''#!/usr/bin/env python3
import hashlib
import json
import sys
from pathlib import Path

args = sys.argv[1:]
with Path("calls.jsonl").open("a") as calls:
    print(json.dumps(args), file=calls)
blob = Path(args[-1])
bundle = Path(args[args.index("--bundle") + 1])
if bundle.read_text() != hashlib.sha256(blob.read_bytes()).hexdigest():
    sys.stderr.write('outside\\n\\x1b"end')
    sys.exit(1)
'''


def outer(target):
    return f"dctx-{target}-airgap.tar.gz"


def digest(blob):
    return hashlib.sha256(blob.read_bytes()).hexdigest()


def make_sidecar(blob):
    blob.with_name(blob.name + ".sigstore.json").write_text(digest(blob))


class AirgapSelection(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        start = (REPO / "docs/START.md").read_text(encoding="utf-8")
        blocks = re.findall(r"^```sh\n(.*?)^```$", start, re.DOTALL | re.MULTILINE)
        cls.recipe, = [block for block in blocks if 'python3 "$VERIFIER"' in block]

    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=REPO)
        self.addCleanup(temp.cleanup)
        self.directory = Path(temp.name)
        recorder = self.directory / "cosign"
        recorder.write_text(RECORDER)
        recorder.chmod(0o755)
        self.root = self.directory / "trusted root.json"
        self.root.write_text("fixture root")
        self.manifest = self.directory / "SHA256SUMS"
        self.destination = self.directory / "verified-dctx"
        rows = []
        for target in (LINUX, WINDOWS, ARM):
            blob = self.directory / outer(target)
            data = target.encode("ascii")
            member = tarfile.TarInfo("dctx.exe" if target == WINDOWS else "dctx")
            member.size = len(data)
            with tarfile.open(blob, "w:gz") as archive:
                archive.addfile(member, io.BytesIO(data))
            make_sidecar(blob)
            rows.append(f"{digest(blob)}  {blob.name}\n")
        self.rows = rows
        self.set_manifest(rows)

    def set_manifest(self, rows):
        self.manifest.write_bytes("".join(rows).encode("ascii"))
        make_sidecar(self.manifest)

    def run_recipe(self, target):
        env = dict(os.environ, PATH=str(self.directory) + os.pathsep + os.environ["PATH"],
                   TAG=TAG, TARGET=target, TRUSTED_ROOT=str(self.root),
                   VERIFIER=str(HERE / "verify-airgap-target.py"))
        return subprocess.run(["sh", "-c", self.recipe], cwd=self.directory, env=env,
                              capture_output=True, text=True)

    def calls(self):
        log = self.directory / "calls.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def expected_call(self, name):
        return ["verify-blob", "--trusted-root", str(self.root),
                "--bundle", name + ".sigstore.json", "--certificate-identity",
                "https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/" + TAG,
                "--certificate-oidc-issuer", "https://token.actions.githubusercontent.com", name]

    def assert_accepted(self, target, member):
        result = self.run_recipe(target)
        self.assertEqual(result.returncode, 0, repr(result.stderr))
        self.assertEqual(self.calls(), [self.expected_call("SHA256SUMS"), self.expected_call(outer(target))])
        self.assertEqual((self.destination / member).read_bytes(), target.encode("ascii"))

    def assert_refused(self, reason, target=LINUX):
        result = self.run_recipe(target)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(reason, result.stderr)
        self.assertFalse(self.destination.exists())
        return result

    def test_linux_x64(self):
        self.assert_accepted(LINUX, "dctx")

    def test_windows_x64(self):
        self.assert_accepted(WINDOWS, "dctx.exe")

    def test_same_tag_arm64_pair_renamed_x64(self):
        for suffix in ("", ".sigstore.json"):
            shutil.copyfile(self.directory / (outer(ARM) + suffix), self.directory / (outer(LINUX) + suffix))
        self.assert_refused("digest mismatch")
        self.assertEqual(self.calls(), [self.expected_call("SHA256SUMS"), self.expected_call(outer(LINUX))])

    def test_only_full_basename_counts(self):
        selected = self.rows[0]
        self.set_manifest([*self.rows[1:], selected.replace("  ", "  nested/"),
                           selected.rstrip("\n") + ".sigstore.json\n"])
        self.assert_refused("missing SHA256SUMS row")

    def test_duplicate_even_with_identical_digest(self):
        self.set_manifest([*self.rows, self.rows[0]])
        self.assert_refused("duplicate SHA256SUMS row")

    def test_malformed_digest(self):
        self.set_manifest(["g" + self.rows[0][1:], *self.rows[1:]])
        self.assert_refused("malformed SHA256SUMS row")

    def test_wrong_manifest_sidecar(self):
        shutil.copyfile(self.directory / (outer(LINUX) + ".sigstore.json"),
                        self.directory / "SHA256SUMS.sigstore.json")
        self.assert_refused("cosign rejected SHA256SUMS")
        self.assertEqual(self.calls(), [self.expected_call("SHA256SUMS")])

    def test_wrong_outer_sidecar_and_hostile_subprocess_error(self):
        shutil.copyfile(self.directory / (outer(ARM) + ".sigstore.json"),
                        self.directory / (outer(LINUX) + ".sigstore.json"))
        result = self.assert_refused("cosign rejected dctx-")
        self.assertEqual(self.calls(), [self.expected_call("SHA256SUMS"), self.expected_call(outer(LINUX))])
        self.assertEqual(len(result.stderr.splitlines()), 1)
        self.assertIn(json.dumps(HOSTILE)[1:-1], result.stderr)

    def test_unsupported_target_and_hostile_argument(self):
        result = self.assert_refused("invalid choice", HOSTILE)
        self.assertEqual(self.calls(), [])
        self.assertEqual(len(result.stderr.splitlines()), 1)
        self.assertNotIn("\x1b", result.stderr)


if __name__ == "__main__":
    unittest.main()
