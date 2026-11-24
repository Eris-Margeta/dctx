# SPDX-License-Identifier: Apache-2.0
"""Exercise the proof recipe with disposable inputs and inert compiler outputs."""

from contextlib import ExitStack, redirect_stderr, redirect_stdout
import hashlib
from io import StringIO
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts import reproducible_build as proof


HOSTILE = 'refused\n\x1b"candidate'


def digest(data):
    return hashlib.sha256(data).hexdigest()


class ProofOutputTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=Path(proof.__file__).resolve().parents[1])
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.base = self.root / "cache"
        self.base.mkdir()
        self.output = self.base / "proof"
        self.lock = self.root / "Cargo.lock"
        self.lock.write_bytes(b"initial lock")
        self.names = [b"Cargo.lock"]
        self.targets = []
        self.stdout = StringIO()
        self.stderr = StringIO()
        stack = ExitStack()
        self.addCleanup(stack.close)
        for cm in (
            patch.object(proof, "__file__", str(self.root / "scripts/reproducible_build.py")),
            patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.base), "CARGO_HOME": str(self.root),
                                    "DCTX_REPRO_NETNS": "1", "SOURCE_DATE_EPOCH": "123"}),
            patch.object(proof.socket, "if_nameindex", return_value=[(1, "lo")]),
            redirect_stdout(self.stdout),
            redirect_stderr(self.stderr),
        ):
            stack.enter_context(cm)

    def invoke(self, first=b"first", second=b"first", during_second=None):
        candidates = enumerate((first, second), start=1)

        def command(args, **kwargs):
            self.assertEqual(kwargs["stdout"], subprocess.PIPE)
            self.assertEqual(kwargs["stderr"], subprocess.PIPE)
            if args[0] == "cargo":
                self.assertIn("--offline", args)
                self.assertIn("--frozen", args)
                env = kwargs["env"]
                self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
                target = Path(env["CARGO_TARGET_DIR"])
                self.assertEqual(target.parent, self.base)
                self.assertNotIn(target, self.targets)
                self.targets.append(target)
                number, candidate = next(candidates)
                if number == 2 and during_second is not None:
                    during_second()
                if candidate is None:
                    return subprocess.CompletedProcess(args, 19, b"", HOSTILE.encode())
                binary = target / proof.TARGET / "release/dctx-cli"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(candidate)
                data = b""
            elif args[:2] == ["git", "ls-files"]:
                data = b"\0".join(self.names) + b"\0"
            elif args == ["git", "rev-parse", "HEAD"]:
                data = b"a" * 40
            else:
                data = {"rustc": HOSTILE.encode(), "cc": b"compiler", "ldd": b"libc",
                        "ld": b"linker"}[args[0]]
            return subprocess.CompletedProcess(args, 0, data, b"")

        with patch.object(proof.subprocess, "run", side_effect=command):
            proof.main(["--output", str(self.output)])

    def snapshot(self):
        return {p.name: p.read_bytes() for p in self.output.iterdir()}

    def assert_pair(self, first, second):
        files = self.snapshot()
        manifest = json.loads(files["manifest.json"])
        hashes = [digest(first), digest(second)]
        self.assertEqual(manifest["artifact_sha256"], hashes)
        self.assertEqual(manifest["reproducible"], first == second)
        self.assertEqual(manifest["artifact"],
                         {"file": "dctx-cli", "bytes": len(first), "sha256": hashes[0]})
        self.assertEqual(files["dctx-cli"], first)
        if first != second:
            self.assertEqual(files["dctx-cli-second"], second)
        else:
            self.assertNotIn("dctx-cli-second", files)
        return manifest

    def test_two_build_success_publishes_current_manifest(self):
        self.output.mkdir(mode=0o700)
        self.invoke(during_second=lambda: self.assertEqual(self.snapshot(), {}))
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o700)
        manifest = self.assert_pair(b"first", b"first")
        self.assertEqual(manifest["schema"], "dctx.build/1")
        self.assertEqual(manifest["clean_builds"], 2)
        self.assertEqual(manifest["lockfile_sha256"], digest(self.lock.read_bytes()))
        self.assertEqual(manifest["source_tree"]["files"], 1)
        self.assertEqual(manifest["source_commit"], "a" * 40)
        self.assertEqual(manifest["source_date_epoch"], 123)
        self.assertEqual(manifest["toolchain"], HOSTILE)
        self.assertIn(json.dumps(HOSTILE), (self.output / "manifest.json").read_text())
        self.assertEqual(json.loads(self.stdout.getvalue()),
                         {"reproducible": True, "artifact_sha256": manifest["artifact_sha256"]})
        self.assertEqual(len(self.targets), 2)
        self.assertEqual(list(self.base.iterdir()), [self.output])

    def test_mismatch_publishes_both_candidates_and_refuses_overwrite(self):
        with self.assertRaisesRegex(SystemExit, "artifact hashes differ"):
            self.invoke(second=b"second")
        manifest = self.assert_pair(b"first", b"second")
        self.assertEqual(json.loads(self.stdout.getvalue()),
                         {"reproducible": False, "artifact_sha256": manifest["artifact_sha256"]})
        saved = self.snapshot()
        with self.assertRaises(SystemExit):
            self.invoke()
        self.assertEqual(self.snapshot(), saved)
        self.assertEqual(len(self.targets), 2)

    def test_completed_proof_refused_before_work(self):
        self.invoke(b"original", b"original")
        (self.output / "notes").write_bytes(b"keep this file")
        saved = self.snapshot()
        with patch.dict(os.environ, DCTX_REPRO_NETNS="0"), \
                patch.object(proof.subprocess, "run") as command, self.assertRaises(SystemExit):
            proof.main(["--output", str(self.output)])
        command.assert_not_called()
        self.assertIn("new or empty", self.stderr.getvalue())
        self.assertEqual(self.snapshot(), saved)
        self.assert_pair(b"original", b"original")

    def test_second_build_refusal_then_retry(self):
        with self.assertRaises(SystemExit) as stopped:
            self.invoke(second=None)
        self.assertEqual(str(stopped.exception), f'"cargo" exited 19: {json.dumps(HOSTILE)}')
        self.assertEqual(len(self.targets), 2)
        self.assertEqual(list(self.base.iterdir()), [])
        self.invoke(b"retry", b"retry")
        self.assert_pair(b"retry", b"retry")

    def test_input_change_refusal_then_retry(self):
        with self.assertRaisesRegex(SystemExit, "build inputs changed"):
            self.invoke(during_second=lambda: self.lock.write_bytes(b"changed"))
        self.assertEqual(len(self.targets), 2)
        self.assertEqual(list(self.base.iterdir()), [])
        self.invoke()
        self.assertEqual(self.assert_pair(b"first", b"first")["lockfile_sha256"], digest(b"changed"))

    def test_interrupted_second_build_leaves_no_public_artifacts(self):
        def interrupt():
            self.assertFalse(self.output.exists())
            raise KeyboardInterrupt

        with self.assertRaises(KeyboardInterrupt):
            self.invoke(during_second=interrupt)
        self.assertEqual(list(self.base.iterdir()), [])

    def test_competing_completed_proof_wins_final_rename(self):
        self.invoke(b"winner", b"winner")
        (self.output / "notes").write_bytes(b"retain")
        saved = self.snapshot()
        winner = self.base / "winner"
        self.output.rename(winner)
        with self.assertRaises(SystemExit):
            self.invoke(during_second=lambda: winner.rename(self.output))
        self.assertIn("became occupied", self.stderr.getvalue())
        self.assertEqual(self.snapshot(), saved)
        self.assert_pair(b"winner", b"winner")
        self.assertEqual(list(self.base.iterdir()), [self.output])

    def test_output_confinement(self):
        alias = self.base / "alias"
        alias.symlink_to(self.root, target_is_directory=True)
        with patch.object(proof.subprocess, "run") as command:
            for output in (self.base, self.root / "outside", alias / "outside"):
                with self.subTest(output=output), self.assertRaises(SystemExit):
                    proof.main(["--output", str(output)])
            command.assert_not_called()
        self.assertFalse((self.root / "outside").exists())

    def test_namespace_with_external_interface_refused_before_work(self):
        with patch.object(proof.socket, "if_nameindex", return_value=[(1, "lo"), (2, "eth0")]), \
                patch.object(proof.subprocess, "run") as command, \
                self.assertRaisesRegex(SystemExit, "isolated network namespace"):
            proof.main(["--output", str(self.output)])
        command.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_hostile_source_path_in_diagnostic(self):
        name = "crates/" + HOSTILE
        path = self.root / name
        path.parent.mkdir()
        path.symlink_to(self.lock)
        self.names.append(os.fsencode(name))
        with self.assertRaises(SystemExit) as stopped:
            self.invoke()
        self.assertEqual(str(stopped.exception), f"build input symlink is unsupported: {json.dumps(name)}")
        self.assertEqual(self.targets, [])
