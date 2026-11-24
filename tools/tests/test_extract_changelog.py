# SPDX-License-Identifier: Apache-2.0
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


TOOLS = Path(__file__).resolve().parents[1]


class ExtractChangelogTest(unittest.TestCase):
    def setUp(self):
        fixture = tempfile.TemporaryDirectory(dir=TOOLS.parent)
        self.addCleanup(fixture.cleanup)
        root = Path(fixture.name)
        (root / "tools").mkdir()
        self.script = root / "tools" / "extract-changelog.sh"
        shutil.copy2(TOOLS / self.script.name, self.script)
        self.changelog = root / "CHANGELOG.md"

    def extract(self, contents, *args):
        self.changelog.write_bytes(contents)
        return subprocess.run([self.script, *args], capture_output=True)

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")

    def assert_notes(self, result, expected):
        self.assertEqual(result.returncode, 0, repr(result.stderr))
        self.assertEqual(result.stdout, expected)

    def test_numbered_release_preserves_body_bytes(self):
        body = b"\r\n### Fixed\r\n\t- caf\xc3\xa9  \r\n\r\n"
        contents = b"## [1.2.3](release-link) - 2026-01-01\r\n" + body + b"## History\nold\n"
        for version in ("1.2.3", "v1.2.3"):
            with self.subTest(version=version):
                self.assert_notes(self.extract(contents, version), body)

    def test_latest_skips_unreleased(self):
        contents = b"## [Unreleased]\nnew\n## [2.0.0-rc.1]\nlatest\n## [1.9.0]\nold\n"
        self.assert_notes(self.extract(contents), b"latest\n")

    def test_body_at_eof_without_final_newline(self):
        self.assert_notes(self.extract(b"## [1.0.0]\nlast line"), b"last line")

    def test_blank_numbered_release_refused(self):
        for body in (b"", b" \n\n", b"\t\r\n \t\n", "\u00a0\n".encode()):
            contents = b"## [1.0.0]\n" + body + b"## [0.9.0]\nolder notes\n"
            for args in ((), ("1.0.0",)):
                with self.subTest(body=body, args=args):
                    self.assert_refused(self.extract(contents, *args))

    def test_missing_release_refused(self):
        self.assert_refused(self.extract(b"## [2.0.0]\nother\n", "1.0.0"))
        self.assert_refused(self.extract(b"## [Unreleased]\ndevelopment\n"))

    def test_later_duplicate_refused_before_output(self):
        contents = b"## [1.0.0]\nfirst\n## [0.9.0]\nolder\n## [1.0.0](link)\nsecond\n"
        for args in ((), ("v1.0.0",)):
            with self.subTest(args=args):
                self.assert_refused(self.extract(contents, *args))

    def test_only_complete_literal_label_matches(self):
        neighbors = b"## [1.2.30]\nprefix\n## [1x2x3]\ndots\n## [1.2.3\nunclosed\n"
        self.assert_refused(self.extract(neighbors, "1.2.3"))
        self.assert_notes(self.extract(neighbors + b"## [1.2.3]\nexact\n", "1.2.3"), b"exact\n")

    def test_empty_unreleased_inspection(self):
        for body in (b"", b" \n\n"):
            with self.subTest(body=body):
                contents = b"## [Unreleased]\n" + body + b"## [1.0.0]\nstable\n"
                self.assert_notes(self.extract(contents, "Unreleased"), body)

    def test_hostile_version_omitted_from_diagnostic(self):
        result = self.extract(b"## [1.0.0]\nnotes\n", '1.0.0\n\x1b[31m"')
        self.assert_refused(result)
        self.assertEqual(result.stderr, b"Error: requested version not found in CHANGELOG.md\n")
