# SPDX-License-Identifier: Apache-2.0
"""Port-free consumer policy and ordering tests. Fixture claims have no signatures."""
import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

COMMAND = Path(__file__).resolve().with_name("verify-offline-provenance.sh")
TAG = "v1.3.2"
LINUX = "x86_64-unknown-linux-musl"
WINDOWS = "x86_64-pc-windows-msvc"
REPO = "Eris-Margeta/dctx"
WORKFLOW = REPO + "/.github/workflows/release.yml"
REF = "refs/tags/" + TAG
IDENTITY = "https://github.com/" + WORKFLOW + "@" + REF
ISSUER = "https://token.actions.githubusercontent.com"
PREDICATE = "https://slsa.dev/provenance/v1"
HOSTILE = 'outside\n\x1b"quote'

# Two local executable stand-ins. Read the files named by the *actual* CLI
# arguments. Record each call before answering so a later call cannot hide a refusal.
RECORDER = r'''#!/usr/bin/env python3
import hashlib
import json
import os
import sys
from pathlib import Path

tool = Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ["CALL_LOG"], "a", encoding="ascii") as log:
    print(json.dumps([tool, *args], ensure_ascii=True), file=log)
if "--help" in args:
    flags = ("--trusted-root --bundle --certificate-identity --certificate-oidc-issuer"
             if tool == "cosign" else
             "--custom-trusted-root --bundle --repo --source-ref --signer-workflow "
             "--deny-self-hosted-runners --predicate-type").split()
    if tool == os.environ.get("LIMIT_TOOL"):
        flag = os.environ["OMIT"]
        flags[flags.index(flag)] = flag + "-unsupported"
    print("\n".join("  " + flag + " string" for flag in flags))
    sys.exit(0)

def value(flag):
    return args[args.index(flag) + 1]

blob = Path(args[-1] if tool == "cosign" else args[2])
claims = json.loads(Path(value("--bundle")).read_bytes())
if tool == "cosign":
    root = Path(value("--trusted-root"))
    policy = {"identity": value("--certificate-identity"), "issuer": value("--certificate-oidc-issuer")}
else:
    root = Path(value("--custom-trusted-root"))
    policy = {"repo": value("--repo"), "ref": value("--source-ref"), "workflow": value("--signer-workflow"),
              "predicate": value("--predicate-type"), "hosted": "--deny-self-hosted-runners" in args}
policy.update(root=root.read_text(), digest=hashlib.sha256(blob.read_bytes()).hexdigest())
if policy != claims or [tool, blob.name] == json.loads(os.environ.get("FAIL_CALL", "null")):
    # Deliberately hostile subprocess output to exercise the consumer's diagnostics.
    print('outside\n\x1b"quote')
    print('outside\n\x1b"quote', file=sys.stderr)
    sys.exit(23)
'''


def tar_bytes(path, name, data):
    with tarfile.open(path, "w:gz") as archive:
        member = tarfile.TarInfo(name)
        member.size = len(data)
        archive.addfile(member, io.BytesIO(data))


class OfflinePolicy(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=COMMAND.parent)
        self.addCleanup(temp.cleanup)
        self.base = Path(temp.name)
        self.release = self.base / "assets"
        self.release.mkdir()
        self.log = self.base / "calls.jsonl"
        self.cosign_root = self.base / ("cosign " + HOSTILE)
        self.gh_root = self.base / "gh root"
        for tool, root in (("cosign", self.cosign_root), ("gh", self.gh_root)):
            root.write_text(tool + "-fixture")
            exe = self.base / tool
            exe.write_text(RECORDER)
            exe.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.base) + os.pathsep + os.environ["PATH"],
                        CALL_LOG=str(self.log))
        self.material(LINUX)

    def material(self, target):
        self.target = target
        self.outer = self.release / f"dctx-{target}-airgap.tar.gz"
        if target == WINDOWS:
            self.native = self.release / f"dctx-{target}.zip"
            with zipfile.ZipFile(self.native, "w") as archive:
                archive.writestr("dctx.exe", b"fixture")
        else:
            self.native = self.release / f"dctx-{target}.tar.gz"
            tar_bytes(self.native, "dctx", b"fixture")
        tar_bytes(self.outer, self.native.name, self.native.read_bytes())
        self.calls = [["cosign", "verify-blob", "--help"], ["gh", "attestation", "verify", "--help"]]
        for blob, bundle in ((self.outer, "airgap-provenance.jsonl"),
                             (self.native, "archive-provenance.jsonl")):
            digest = hashlib.sha256(blob.read_bytes()).hexdigest()
            sidecar = blob.name + ".sigstore.json"
            (self.release / sidecar).write_text(json.dumps(dict(
                digest=digest, identity=IDENTITY, issuer=ISSUER, root="cosign-fixture")))
            (self.release / bundle).write_text(json.dumps(dict(
                digest=digest, repo=REPO, ref=REF, workflow=WORKFLOW, predicate=PREDICATE,
                hosted=True, root="gh-fixture")))
            self.calls.extend([
                ["cosign", "verify-blob", "--trusted-root", str(self.cosign_root), "--bundle", sidecar,
                 "--certificate-identity", IDENTITY, "--certificate-oidc-issuer", ISSUER, blob.name],
                ["gh", "attestation", "verify", blob.name, "--bundle", bundle, "--custom-trusted-root",
                 str(self.gh_root), "--repo", REPO, "--signer-workflow", WORKFLOW, "--source-ref",
                 REF, "--deny-self-hosted-runners", "--predicate-type", PREDICATE],
            ])

    def check_run(self, count, *, success=False, tag=TAG, **env):
        self.log.write_text("")
        before = {p.name: p.read_bytes() for p in self.release.iterdir()}
        result = subprocess.run([COMMAND, tag, self.target, self.cosign_root, self.gh_root],
                                cwd=self.release, env=self.env | env, capture_output=True, text=True)
        self.assertEqual(result.returncode == 0, success, ascii((result.stdout, result.stderr)))
        self.assertEqual([json.loads(line) for line in self.log.read_text().splitlines()],
                         self.calls[:count])
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.release.iterdir()})
        if not success:
            self.assertEqual(result.stdout, "")
            self.assertEqual(len(result.stderr.splitlines()), 1)
            self.assertNotIn("\x1b", result.stderr)
        return result

    def test_native_tar(self):
        self.check_run(6, success=True)

    def test_native_zip(self):
        self.material(WINDOWS)
        self.check_run(6, success=True)

    def test_missing_inputs(self):
        for path in [self.cosign_root, self.gh_root, *self.release.iterdir()]:
            with self.subTest(file=path.name):
                data = path.read_bytes()
                path.unlink()
                result = self.check_run(0)
                if path == self.cosign_root:
                    self.assertIn(r"outside\n", result.stderr)
                    self.assertIn("quote", result.stderr)
                path.write_bytes(data)

    def test_unsupported_offline_options(self):
        for tool, flag, count in (("cosign", "--trusted-root", 1), ("gh", "--bundle", 2),
                                  ("gh", "--custom-trusted-root", 2), ("gh", "--source-ref", 2)):
            with self.subTest(tool=tool, flag=flag):
                self.check_run(count, LIMIT_TOOL=tool, OMIT=flag)

    def test_wrong_root(self):
        for path, count in ((self.cosign_root, 3), (self.gh_root, 4)):
            with self.subTest(root=path.name):
                original = path.read_bytes()
                path.write_text("untrusted-fixture")
                self.check_run(count)
                path.write_bytes(original)

    def test_swapped_provenance(self):
        outer = self.release / "airgap-provenance.jsonl"
        native = self.release / "archive-provenance.jsonl"
        outer_bytes = outer.read_bytes()
        outer.write_bytes(native.read_bytes())
        native.write_bytes(outer_bytes)
        self.check_run(4)
        outer.write_bytes(outer_bytes)
        self.check_run(6)

    def test_changed_payload(self):
        self.material(WINDOWS)
        for path, count in ((self.outer, 3), (self.native, 5)):
            with self.subTest(blob=path.name):
                original = path.read_bytes()
                path.write_bytes(original + b"changed")
                self.check_run(count)
                path.write_bytes(original)

    def test_denied_claims(self):
        for bundle, field, count in (
            (self.outer.name + ".sigstore.json", "issuer", 3),
            (self.native.name + ".sigstore.json", "identity", 5),
            ("airgap-provenance.jsonl", "ref", 4),
            ("archive-provenance.jsonl", "repo", 6),
            ("archive-provenance.jsonl", "workflow", 6),
            ("archive-provenance.jsonl", "digest", 6),
        ):
            with self.subTest(bundle=bundle, field=field):
                path = self.release / bundle
                original = path.read_bytes()
                claims = json.loads(original)
                claims[field] += "-denied"
                path.write_text(json.dumps(claims))
                self.check_run(count)
                path.write_bytes(original)

    def test_different_expected_tag(self):
        tag = "v1.3.3-rc.2"
        args = self.calls[2]
        args[args.index(IDENTITY)] = IDENTITY.replace(TAG, tag)
        self.check_run(3, tag=tag)

    def test_verifier_nonzero_stops_at_that_call(self):
        for count, (tool, blob) in enumerate(
            (("cosign", self.outer), ("gh", self.outer), ("cosign", self.native), ("gh", self.native)),
            start=3,
        ):
            with self.subTest(tool=tool, blob=blob.name):
                self.check_run(count, FAIL_CALL=json.dumps([tool, blob.name]))


if __name__ == "__main__":
    print("Offline provenance policy and ordering tests (recording verifiers)", file=sys.stderr)
    unittest.main()
