# SPDX-License-Identifier: Apache-2.0
"""Fixture-tree tests of the release version gate's public CLI."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("check_versions.py").resolve()


class CheckVersionsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=SCRIPT.parent.parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.fixture()

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def fixture(self, version="1.3.1"):
        self.write("VERSION", version + "\n")
        self.write("CHANGELOG.md", f"# Changelog\n\n## [{version}] - 2026-11-09\n\n## [0.1.0]\n")
        self.write("Cargo.toml", f'[workspace.package]\nversion = "{version}"\n')
        for relative in ("packages/client/package.json", "packages/other/package.json",
                         "packaging/npm/dctx/package.json"):
            self.write(relative, json.dumps({"version": version}))
        for relative in ("tools/website/data/facts.json", "tools/website/data/demo.json",
                         "tools/website/data/llms.json", "website/build.json"):
            self.write(relative, json.dumps({"version": version, "release_tag": "v" + version}))
        self.write("website/index.html", f'<script type="application/ld+json">'
                   f'{{"softwareVersion": "{version}"}}</script>\n'
                   f'<span class="ver">{version}</span>\n'
                   f'<p class="mono small">dctx {version} &middot; Apache-2.0</p>\n'
                   '<p>Historical release 0.1.0</p>')
        self.write("website/llms.txt", f"- Current version: {version}, released today\n"
                   f"- Engine: dctx {version}. Document captures\n")

    def run_check(self, *args):
        return subprocess.run([sys.executable, "-B", str(SCRIPT), "--root", str(self.root), *args],
                              cwd=self.root, capture_output=True, text=True, check=False)

    def assert_status(self, status, *args):
        result = self.run_check(*args)
        self.assertEqual(result.returncode, status, result.stdout + result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        return result

    def test_all_equal_and_read_only(self):
        before = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        result = self.assert_status(0)
        self.assertTrue(result.stdout.startswith("source\tvalue\tok\n"))
        self.assertIn('packages/other/package.json:version\t"1.3.1"\tyes', result.stdout)
        after = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def test_one_npm_package_behind(self):
        self.write("packages/other/package.json", '{"version":"1.3.0"}')
        result = self.assert_status(1)
        self.assertIn('packages/other/package.json:version\t"1.3.0"\tNO', result.stdout)
        self.assertIn("Disagreeing or invalid sources: packages/other/package.json:version", result.stdout)

    def test_published_npm_package_behind(self):
        self.write("packaging/npm/dctx/package.json", '{"version":"1.3.0"}')
        self.assertIn("packaging/npm/dctx/package.json:version", self.assert_status(1).stdout)

    def test_changelog_behind(self):
        self.write("CHANGELOG.md", "# Changelog\n## [1.3.0] - 2026-11-01\n## [1.2.0]\n")
        self.assertIn('CHANGELOG.md:release\t"1.3.0"\tNO', self.assert_status(1).stdout)

    def test_unreleased_without_flag(self):
        self.write("CHANGELOG.md", "# Changelog\n## [Unreleased]\n\n## [1.3.1]\n")
        self.assertIn('CHANGELOG.md:top\t"Unreleased"\tNO', self.assert_status(1).stdout)

    def test_unreleased_with_flag(self):
        self.write("CHANGELOG.md", "# Changelog\n## [Unreleased]\n\n## [1.3.1]\n")
        self.assert_status(0, "--allow-unreleased")

    def test_unreleased_does_not_hide_release_drift(self):
        self.write("CHANGELOG.md", "# Changelog\n## [Unreleased]\n\n## [1.3.0]\n")
        self.assertIn('CHANGELOG.md:release\t"1.3.0"\tNO',
                      self.assert_status(1, "--allow-unreleased").stdout)

    def test_unreleased_only(self):
        self.write("CHANGELOG.md", "# Changelog\n## Unreleased\n")
        self.assert_status(1)
        self.assert_status(0, "--allow-unreleased")

    def test_prerelease_and_build_suffixes_are_exact_strings(self):
        self.fixture("2.0.0-rc.1+build.7")
        self.assert_status(0)
        for value in ("2.0.0", "2.0.0-rc.2+build.7", "2.0.0-rc.1+build.8", "2.0.0-RC.1+build.7"):
            with self.subTest(value=value):
                self.write("packages/client/package.json", json.dumps({"version": value}))
                self.assert_status(1)

    def test_website_stale_values_are_found_without_grepping_expected(self):
        for relative in ("tools/website/data/facts.json", "tools/website/data/demo.json",
                         "tools/website/data/llms.json", "website/build.json", "website/index.html",
                         "website/llms.txt"):
            with self.subTest(relative=relative):
                path = self.root / relative
                before = path.read_text(encoding="utf-8")
                self.write(relative, before.replace("1.3.1", "1.3.0"))
                self.assertIn(relative, self.assert_status(1).stdout.split("Disagreeing or invalid sources: ")[1])
                self.write(relative, before)

    def test_missing_workspace_version(self):
        self.write("Cargo.toml", '[workspace.package]\nlicense = "Apache-2.0"\n')
        self.assertIn('Cargo.toml:workspace.package.version\t"<missing>"\tNO',
                      self.assert_status(1).stdout)

    def test_missing_or_malformed_sources_fail_cleanly(self):
        for relative, invalid in (("VERSION", ""), ("CHANGELOG.md", "# Changelog\n"),
                                  ("Cargo.toml", "[broken"), ("packages/client/package.json", "{"),
                                  ("tools/website/data/facts.json", "[]")):
            with self.subTest(relative=relative):
                self.fixture()
                self.write(relative, invalid)
                self.assert_status(1)
        self.fixture()
        (self.root / "VERSION").unlink()
        self.assert_status(1)

    def test_plain_headings_and_fenced_examples(self):
        self.write("CHANGELOG.md", "# Changelog\n```md\n## [0.0.0]\n```\n## 1.3.1 - today\n## 0.1.0\n")
        self.assert_status(0)

    def test_usage_errors_exit_two(self):
        self.assert_status(2, "--unknown")
        self.assert_status(2, "--root")


if __name__ == "__main__":
    unittest.main()
