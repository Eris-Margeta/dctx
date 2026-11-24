# SPDX-License-Identifier: Apache-2.0
"""Drive the release recipe with a fixture CLI and inert build and archive tools."""
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


RELEASE = Path(__file__).resolve().parent
TARGET = "x86_64-unknown-linux-musl"
HEAD = "123abcd" + "0" * 33
IDENTITY = b"dctx 2.10.3 (123abcd)"


def executable(path, script):
    path.write_text("#!/bin/sh\n" + script, encoding="ascii")
    path.chmod(0o755)


class BuildIdentity(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory(dir=RELEASE.parents[1])
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        release = self.root / "tools/release"
        release.mkdir(parents=True)
        shutil.copyfile(RELEASE / "toolchain.env", release / "toolchain.env")
        (self.root / "VERSION").write_bytes(b"2.10.3\n")
        (release / "supply-chain.py").write_text(
            'import json, sys\nfrom pathlib import Path\n'
            'Path("archive.json").write_text(json.dumps(sys.argv[1:]))\n',
            encoding="ascii",
        )
        stubs = self.root / ".cargo/bin"
        stubs.mkdir(parents=True)
        executable(stubs / "cargo", 'printf "%s\\n" "$DCTX_GIT_SHA" > build-sha\n')
        executable(stubs / "git", f'''case "$*" in
  "rev-parse HEAD") echo {HEAD} ;;
  "log -1 --format=%ct") echo 123456 ;;
  *) exit 1 ;;
esac
''')
        self.artifacts = self.root / "fixture binaries"
        self.binary = self.artifacts / TARGET / "release/dctx-cli"
        self.binary.parent.mkdir(parents=True)
        executable(self.binary, '''[ "$#" = 1 ] && [ "$1" = --version ] || exit 2
cat fixture.stdout
cat fixture.stderr >&2
exit "$FIXTURE_STATUS"
''')
        self.archive = self.root / "archive.json"

    def build(self, stdout, *, stderr=b"", status=0, sha=None, epoch=None):
        (self.root / "fixture.stdout").write_bytes(stdout)
        (self.root / "fixture.stderr").write_bytes(stderr)
        env = os.environ.copy()
        env.pop("DCTX_GIT_SHA", None)
        env.pop("SOURCE_DATE_EPOCH", None)
        if sha is not None:
            env["DCTX_GIT_SHA"] = sha
        if epoch is not None:
            env["SOURCE_DATE_EPOCH"] = epoch
        env.update(HOME=str(self.root), CARGO_TARGET_DIR=str(self.artifacts), FIXTURE_STATUS=str(status))
        return subprocess.run(
            ["bash", RELEASE / "build.sh", TARGET, "release output"],
            cwd=self.root, env=env, capture_output=True,
        )

    def assert_archived(self, result, stdout, sha, epoch):
        self.assertEqual(result.returncode, 0, repr(result.stderr))
        self.assertEqual(result.stdout, stdout)
        self.assertEqual((self.root / "build-sha").read_text().splitlines(), [sha])
        self.assertEqual(json.loads(self.archive.read_text()), [
            "archive", "--target", TARGET, "--binary", str(self.binary),
            "--out", "release output", "--epoch", epoch,
        ])

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertFalse(self.archive.exists())

    def test_checkout_identity(self):
        stdout = IDENTITY + b"\n"
        self.assert_archived(self.build(stdout), stdout, HEAD, "123456")

    def test_crlf_and_reproducibility_overrides(self):
        sha = "FEDCBA9" + "8" * 33
        stdout = b"dctx 2.10.3 (FEDCBA9)\r\n"
        self.assert_archived(self.build(stdout, sha=sha, epoch="987"), stdout, sha, "987")

    def test_mismatched_records(self):
        for stdout in (
            b"dctx 12.10.3 (123abcd)\n", b"dctx 2.10.30 (123abcd)\n",
            b"other 2.10.3 (123abcd)\n", b"dctx 2.10.3 (123abce)\n",
            b"dctx 2.10.3 (" + HEAD.encode() + b")\n", b"dctx 2.10.3 (unknown)\n",
            b"dctx 2.10.3\n", b"", IDENTITY, IDENTITY + b"\n\n",
            IDENTITY + b"\n" + IDENTITY + b"\n", IDENTITY + b"\x00\n",
        ):
            with self.subTest(stdout=stdout):
                self.assert_refused(self.build(stdout))

    def test_override_must_match_and_be_hexadecimal(self):
        for sha, reported in (("abcdef0", b"123abcd"), ("123abcd!", b"123abcd"), ("123abc", b"123abc")):
            with self.subTest(sha=sha):
                self.assert_refused(self.build(b"dctx 2.10.3 (" + reported + b")\n", sha=sha))

    def test_nonzero_exit_with_matching_stdout(self):
        self.assert_refused(self.build(IDENTITY + b"\n", status=3))

    def test_missing_executable(self):
        self.binary.unlink()
        self.assert_refused(self.build(b""))

    def test_hostile_output_is_quoted(self):
        hostile = b'line\n\x1b[31m"\xff'
        result = self.build(hostile, stderr=hostile)
        self.assert_refused(result)
        self.assertEqual(len(result.stderr.splitlines()), 2)
        for line in result.stderr.splitlines():
            self.assertIn(repr(hostile).encode(), line)
            self.assertNotIn(b"\x1b", line)


if __name__ == "__main__":
    unittest.main()
