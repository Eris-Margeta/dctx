# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Exercise the release payload contract, determinism and signer failure handling."""
import hashlib
import importlib.util
import io
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


# The expected release layout is independent of the writer's target and naming helpers.
NATIVE_FORMATS = {
    "x86_64-unknown-linux-musl": ("tar.gz", "dctx"),
    "aarch64-unknown-linux-musl": ("tar.gz", "dctx"),
    "x86_64-apple-darwin": ("tar.gz", "dctx"),
    "aarch64-apple-darwin": ("tar.gz", "dctx"),
    "x86_64-pc-windows-msvc": ("zip", "dctx.exe"),
}


class DisposableRelease(unittest.TestCase):
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


class ReleasePayloads(DisposableRelease):
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


class ReleaseInventories(DisposableRelease):
    def setUp(self):
        super().setUp()
        self.artifacts = self.root / "artifacts"
        self.stage = self.root / "stage"
        self.artifacts.mkdir()
        self.outer_names = {"archive-provenance.jsonl", "airgap-provenance.jsonl"}
        for target, (extension, _) in NATIVE_FORMATS.items():
            supply.archive(target, self.binary, self.artifacts, self.epoch)
            sbom = f"dctx-{target}.cdx.json"
            (self.artifacts / sbom).write_text('{"bomFormat":"CycloneDX"}')
            self.outer_names.update((f"dctx-{target}.{extension}", sbom, f"dctx-{target}-airgap.tar.gz"))
        (self.artifacts / "archive-provenance.jsonl").write_text('{"synthetic":"provenance"}\n')
        self.signed = []

        def fake_sign(path):
            self.signed.append(path)
            Path(str(path) + ".sigstore.json").write_text("synthetic test bundle")

        with patch.object(supply, "sign", side_effect=fake_sign):
            supply.seal(self.artifacts, self.stage, self.epoch)
            (self.artifacts / "airgap-provenance.jsonl").write_text('{"synthetic":"airgap"}\n')
            supply.finish(self.artifacts)

    def assert_manifest(self, manifest, expected, read_payload):
        rows = []
        for line in manifest.decode("utf-8").removesuffix("\n").split("\n"):
            digest, separator, name = line.partition("  ")
            self.assertTrue(separator and name, f"malformed checksum row {ascii(line)}")
            rows.append((name, digest))
        self.assertCountEqual([name for name, _ in rows], expected, "wrong manifest inventory")
        for name, digest in rows:
            actual = hashlib.sha256(read_payload(name)).hexdigest()
            self.assertTrue(actual == digest, f"checksum mismatch for {ascii(name)}")

    def assert_outer_manifest(self):
        self.assert_manifest((self.artifacts / "SHA256SUMS").read_bytes(), self.outer_names,
                             lambda name: (self.artifacts / name).read_bytes())

    def assert_airgap(self, target):
        extension, executable = NATIVE_FORMATS[target]
        native = f"dctx-{target}.{extension}"
        sbom = f"dctx-{target}.cdx.json"
        payloads = {native, sbom, executable, "LICENSE", "NOTICE", "VERIFYING.md", "archive-provenance.jsonl"}
        self.assertEqual(len(payloads), 7)
        sidecars = {name + ".sigstore.json" for name in (native, sbom, "SHA256SUMS")}
        with tarfile.open(self.artifacts / f"dctx-{target}-airgap.tar.gz", "r:gz") as bundle:
            self.assertCountEqual(bundle.getnames(), payloads | sidecars | {"SHA256SUMS"})

            def read_member(name):
                return bundle.extractfile(name).read()

            self.assert_manifest(read_member("SHA256SUMS"), payloads, read_member)
            stream = io.BytesIO(read_member(native))
            if extension == "zip":
                with zipfile.ZipFile(stream) as archive:
                    nested = archive.read(executable)
            else:
                with tarfile.open(fileobj=stream, mode="r:gz") as archive:
                    nested = archive.extractfile(executable).read()
            loose = read_member(executable)
            self.assertEqual(nested, loose, "native and loose executables differ")
            self.assertEqual(loose, self.binary.read_bytes())

    def test_signed_release_inventories(self):
        self.assertEqual(len(self.outer_names), 17)
        self.assert_outer_manifest()
        signed_names = self.outer_names | {"SHA256SUMS"}
        self.assertCountEqual((p.name for p in self.artifacts.iterdir()),
                              signed_names | {name + ".sigstore.json" for name in signed_names})
        for name in signed_names:
            self.assertIn(self.artifacts / name, self.signed)
            self.assertTrue((self.artifacts / (name + ".sigstore.json")).is_file(), ascii(name))
        for target in NATIVE_FORMATS:
            with self.subTest(target=target):
                self.assertIn(self.stage / target / "SHA256SUMS", self.signed)
                self.assert_airgap(target)

    def test_missing_executable_and_duplicate_row(self):
        target = "x86_64-unknown-linux-musl"
        stage = self.stage / target
        manifest = stage / "SHA256SUMS"
        rows = manifest.read_bytes().splitlines(keepends=True)
        executable_row, = (row for row in rows if row.endswith(b"  dctx\n"))
        for label, damaged in (("missing", [row for row in rows if row != executable_row]),
                               ("duplicate", rows + [executable_row])):
            with self.subTest(damage=label):
                manifest.write_bytes(b"".join(damaged))
                supply.pack_airgap(stage, self.artifacts, target, self.epoch)
                with self.assertRaisesRegex(AssertionError, "wrong manifest inventory"):
                    self.assert_airgap(target)

    def test_missing_outer_provenance_row(self):
        manifest = self.artifacts / "SHA256SUMS"
        rows = manifest.read_bytes().splitlines(keepends=True)
        manifest.write_bytes(b"".join(row for row in rows if not row.endswith(b"  airgap-provenance.jsonl\n")))
        with self.assertRaisesRegex(AssertionError, "wrong manifest inventory"):
            self.assert_outer_manifest()

    def test_disagreeing_executable_copies_with_valid_checksums(self):
        for target in ("x86_64-apple-darwin", "x86_64-pc-windows-msvc"):
            with self.subTest(target=target):
                stage = self.stage / target
                manifest = stage / "SHA256SUMS"
                names = [row.split("  ")[1] for row in manifest.read_text().splitlines()]
                (stage / NATIVE_FORMATS[target][1]).write_bytes(b"different executable")
                supply.checksums(stage, names)
                supply.pack_airgap(stage, self.artifacts, target, self.epoch)
                with self.assertRaisesRegex(AssertionError, "native and loose executables differ"):
                    self.assert_airgap(target)

    def test_member_diagnostic_escapes_hostile_name(self):
        target = "x86_64-apple-darwin"
        hostile = 'unexpected\n\x1b[31m"member"'
        bundle = self.artifacts / f"dctx-{target}-airgap.tar.gz"
        supply.tar_gz(bundle, {hostile: (b"", 0o644)}, self.epoch)
        self.maxDiff = None
        with self.assertRaises(AssertionError) as failure:
            self.assert_airgap(target)
        self.assertIn(ascii(hostile), str(failure.exception))
        self.assertNotIn("\x1b", str(failure.exception))

    def test_detect_tampering_and_refuse_reseal(self):
        original = self.artifacts / "dctx-x86_64-unknown-linux-musl.tar.gz"
        original.write_bytes(b"tampered")
        with self.assertRaisesRegex(AssertionError, "checksum mismatch"):
            self.assert_outer_manifest()
        with self.assertRaisesRegex(ValueError, "reseal"):
            with patch.object(supply, "sign"):
                supply.finish(self.artifacts)


if __name__ == "__main__":
    unittest.main()
