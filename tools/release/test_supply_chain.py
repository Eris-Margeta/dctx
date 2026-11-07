# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Exercise the release payload contract, determinism and signer failure handling."""
import importlib.util
import os
import subprocess
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("supply_chain", Path(__file__).with_name("supply-chain.py"))
supply = importlib.util.module_from_spec(spec)
spec.loader.exec_module(supply)


class ReleasePayloads(unittest.TestCase):
    def setUp(self):
        # Keep test writes under the checkout even on a developer machine.
        temp_root = supply.ROOT / "target/release-script-tests"
        temp_root.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=temp_root)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binary = self.root / "cli"
        self.binary.write_bytes(b"synthetic executable\x00\x01")
        self.epoch = 1794070123

    def test_archives_ignore_host_mtime_mode_and_location(self):
        for target in supply.TARGETS:
            with self.subTest(target=target):
                one = supply.archive(target, self.binary, self.root / "one", self.epoch)
                os.utime(self.binary, (1, 1))
                self.binary.chmod(0o600)
                two = supply.archive(target, self.binary, self.root / "two", self.epoch)
                self.assertEqual(one.read_bytes(), two.read_bytes())
                expected = ["LICENSE", "NOTICE", supply.executable(target)]
                if "windows" in target:
                    with zipfile.ZipFile(one) as archive:
                        self.assertEqual(archive.namelist(), sorted(expected))
                        for member in archive.infolist():
                            self.assertEqual(member.date_time, (1980, 1, 1, 0, 0, 0))
                            self.assertEqual(member.create_system, 3)
                else:
                    with tarfile.open(one) as archive:
                        self.assertEqual(archive.getnames(), sorted(expected))
                        for member in archive.getmembers():
                            self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, self.epoch))
                            self.assertEqual((member.uname, member.gname), ("", ""))
                            self.assertEqual(member.mode, 0o755 if member.name == "dctx" else 0o644)

    def test_seal_all_platforms_and_detect_tampering(self):
        artifacts = self.root / "artifacts"
        artifacts.mkdir()
        for target in supply.TARGETS:
            supply.archive(target, self.binary, artifacts, self.epoch)
            (artifacts / f"dctx-{target}.cdx.json").write_text('{"bomFormat":"CycloneDX"}')
        (artifacts / "archive-provenance.jsonl").write_text('{"synthetic":"provenance"}\n')
        signed = []

        def fake_sign(path):
            signed.append(path)
            Path(str(path) + ".sigstore.json").write_text("synthetic test bundle")

        with patch.object(supply, "sign", side_effect=fake_sign):
            supply.seal(artifacts, self.root / "stage", self.epoch)
            (artifacts / "airgap-provenance.jsonl").write_text('{"synthetic":"airgap"}\n')
            supply.finish(artifacts)
        for payload in artifacts.iterdir():
            if not payload.name.endswith(".sigstore.json"):
                self.assertIn(payload, signed)
                self.assertTrue(Path(str(payload) + ".sigstore.json").is_file())
        for target in supply.TARGETS:
            bundle = artifacts / f"dctx-{target}-airgap.tar.gz"
            with tarfile.open(bundle) as archive:
                names = set(archive.getnames())
                required = {supply.executable(target), "LICENSE", "NOTICE", "VERIFYING.md",
                            "SHA256SUMS", "SHA256SUMS.sigstore.json", "archive-provenance.jsonl",
                            supply.archive_name(target), supply.archive_name(target) + ".sigstore.json",
                            f"dctx-{target}.cdx.json", f"dctx-{target}.cdx.json.sigstore.json"}
                self.assertEqual(names, required)
                self.assertEqual(archive.extractfile(supply.executable(target)).read(), self.binary.read_bytes())
                for line in archive.extractfile("SHA256SUMS").read().decode().splitlines():
                    digest, filename = line.split("  ")
                    data = archive.extractfile(filename).read()
                    self.assertEqual(supply.hashlib.sha256(data).hexdigest(), digest)
        sums = {name: digest for digest, name in
                (line.split("  ") for line in (artifacts / "SHA256SUMS").read_text().splitlines())}
        original = artifacts / supply.archive_name(supply.TARGETS[0])
        original.write_bytes(b"tampered")
        self.assertNotEqual(supply.sha256(original), sums[original.name])
        with self.assertRaisesRegex(ValueError, "reseal"):
            with patch.object(supply, "sign"):
                supply.finish(artifacts)

    def test_sign_requires_identity_and_verifies_issuer(self):
        path = self.binary
        identity = "https://github.com/Eris-Margeta/dctx/.github/workflows/release.yml@refs/tags/v1.2.0"
        with patch.dict(os.environ, {"DCTX_CERTIFICATE_IDENTITY": identity}):
            with patch.object(supply.subprocess, "run") as run:
                supply.sign(path)
                self.assertEqual(run.call_count, 2)
                verify = run.call_args_list[1].args[0]
                self.assertEqual(verify[0:2], ["cosign", "verify-blob"])
                self.assertIn(identity, verify)
                self.assertIn("https://token.actions.githubusercontent.com", verify)
                self.assertFalse(any(flag.startswith("--insecure") for flag in verify))
            with patch.object(supply.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "cosign")):
                with self.assertRaises(subprocess.CalledProcessError):
                    supply.sign(path)


if __name__ == "__main__":
    unittest.main()
