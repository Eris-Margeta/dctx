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

    def preflight_fixture(self):
        self.write("tools/release/toolchain.env", "DCTX_CI_RUST=1.100.0\nDCTX_RELEASE_RUST=1.99.0\n")
        self.write("rust-toolchain.toml", '[toolchain]\nchannel = "1.100.0"\n')
        self.write("packages/python/pyproject.toml", '[project]\nversion = "1.3.1"\n')
        self.write(".github/workflows/ci.yml", f"""
jobs:
  build:
    steps:
      - uses: actions/checkout@{'a' * 40}
      - uses: ./local
      - run: rustup toolchain install "$DCTX_CI_RUST" --profile minimal
""")

    def test_preflight_passes_with_ci_pin_and_unreleased_heading(self):
        self.preflight_fixture()
        self.write("CHANGELOG.md", "## [Unreleased]\n\n## [1.3.1](releases/1.3.1)\n")
        self.write("Cargo.toml", '[workspace]\nmembers = ["crates/*"]\n')
        self.write("crates/cli/Cargo.toml", '[package]\nversion = "1.3.1"\n')
        result = self.assert_status(0, "--preflight")
        self.assertEqual(result.stdout.splitlines(), [
            "PASS versions", "PASS changelog", "PASS rust-toolchain", "PASS action-pins",
        ])

    def test_preflight_accepts_plain_release_heading_after_fenced_example(self):
        self.preflight_fixture()
        self.write("CHANGELOG.md", "# Changelog\n```md\n## 1.3.2\n```\n## 1.3.1 - today\n## 0.1.0\n")
        self.assertIn("PASS changelog", self.assert_status(0, "--preflight").stdout)

    def test_preflight_rejects_newer_heading_before_expected_release(self):
        self.preflight_fixture()
        self.write("CHANGELOG.md", "## [Unreleased]\n\n## [1.3.2]\n\n## [1.3.1]\n")
        result = self.assert_status(1, "--preflight")
        self.assertEqual(result.stdout.splitlines(), [
            "PASS versions", "FAIL changelog: CHANGELOG.md:release='1.3.2' (VERSION='1.3.1')",
            "PASS rust-toolchain", "PASS action-pins",
        ])

    def test_preflight_requires_a_release_heading(self):
        self.preflight_fixture()
        for sections in ("", "## Unreleased\n"):
            with self.subTest(sections=sections):
                self.write("CHANGELOG.md", "# Changelog\n" + sections)
                result = self.assert_status(1, "--preflight")
                self.assertIn("FAIL changelog: CHANGELOG.md:release='<missing>' (VERSION='1.3.1')",
                              result.stdout.splitlines())

    def test_preflight_reports_cargo_npm_and_python_version_drift(self):
        self.preflight_fixture()
        self.write("Cargo.toml", '[workspace.package]\nversion = "1.3.0"\n')
        self.write("packages/client/package.json", '{"version":"1.3.0"}')
        self.write("packages/python/pyproject.toml", '[project]\nversion = "1.3.0"\n')
        failure = self.assert_status(1, "--preflight").stdout.splitlines()[0]
        self.assertTrue(failure.startswith("FAIL versions:"))
        for source in ("Cargo.toml:workspace.package.version", "packages/client/package.json",
                       "packages/python/pyproject.toml"):
            self.assertIn(source, failure)

    def test_preflight_rejects_fenced_changelog_entry(self):
        self.preflight_fixture()
        self.write("CHANGELOG.md", "## [Unreleased]\n```md\n## [1.3.1]\n```\n")
        self.assertIn("FAIL changelog:", self.assert_status(1, "--preflight").stdout)

    def test_preflight_requires_rust_toolchain_file(self):
        self.preflight_fixture()
        (self.root / "rust-toolchain.toml").unlink()
        self.assertIn("FAIL rust-toolchain: rust-toolchain.toml: FileNotFoundError",
                      self.assert_status(1, "--preflight").stdout)

    def test_preflight_rejects_drift_from_workflow_ci_pin(self):
        self.preflight_fixture()
        self.write("rust-toolchain.toml", '[toolchain]\nchannel = "1.99.0"\n')
        output = self.assert_status(1, "--preflight").stdout
        self.assertIn("FAIL rust-toolchain: .github/workflows/ci.yml:", output)
        self.assertIn("expected 1.100.0, found '1.99.0'", output)

    def test_preflight_rejects_drift_from_literal_toolchain(self):
        self.preflight_fixture()
        self.write(".github/workflows/other.yaml", "env:\n  RUSTUP_TOOLCHAIN: '1.97.1'\n")
        output = self.assert_status(1, "--preflight").stdout
        self.assertIn("FAIL rust-toolchain: .github/workflows/other.yaml:2:", output)

    def test_preflight_rejects_unpinned_tag_when_other_workflow_pins_sha(self):
        self.preflight_fixture()
        self.write(".github/workflows/other.yml", 'jobs:\n  call:\n    uses: "owner/repo/.github/workflows/run.yml@v1"\n')
        output = self.assert_status(1, "--preflight").stdout
        self.assertIn("FAIL action-pins: .github/workflows/other.yml:3:", output)
        self.assertIn("@v1 needs a full commit SHA", output)

    def test_release_script_fails_after_printing_all_preflight_results(self):
        self.preflight_fixture()
        (self.root / "rust-toolchain.toml").unlink()
        for source in (SCRIPT, SCRIPT.with_name("release-check.sh")):
            self.write(f"scripts/{source.name}", source.read_text(encoding="utf-8"))
        command = ["bash", str(self.root / "scripts/release-check.sh"), "--preflight-only"]
        result = subprocess.run(command, cwd=self.root, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("PASS versions", result.stdout)
        self.assertIn("PASS changelog", result.stdout)
        self.assertIn("FAIL rust-toolchain:", result.stdout)
        self.assertIn("PASS action-pins", result.stdout)
        self.assertTrue(result.stdout.endswith("RELEASE-CHECK FAIL: offline-preflight\n"))

    def test_usage_errors_exit_two(self):
        self.assert_status(2, "--unknown")
        self.assert_status(2, "--root")


if __name__ == "__main__":
    unittest.main()
