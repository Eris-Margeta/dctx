# SPDX-License-Identifier: Apache-2.0
"""Fixture trees and CLI exit contracts for public status claims."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts import check_status_claims as checker


WORKTREE = Path(__file__).resolve().parents[1]


class StatusClaimsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=WORKTREE)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("VERSION", "1.3.1\n")
        self.write("README.md", "# Project\n**dctx is not yet stable and not enterprise-ready.**\n")
        self.write("website/index.html", "<p>dctx is <strong>not yet stable</strong>.</p>\n")
        self.write("docs/guide.md", "Measure your own workload.\n")
        self.write("scripts/status_claims_allow.txt", "# No exceptions.\n")

    def write(self, name, contents):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def cli(self, *args):
        return subprocess.run([sys.executable, "-B", str(WORKTREE / "scripts/check_status_claims.py"),
                               "--root", str(self.root), *args],
                              capture_output=True, text=True, check=False)

    def test_clean(self):
        result = self.cli()
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""))

    def test_each_forbidden_phrase_and_line(self):
        for phrase in checker.PHRASES:
            with self.subTest(phrase=phrase):
                self.write("docs/guide.md", "# Guide\n" + phrase.upper() + "\n")
                self.assertEqual(checker.check(self.root), [checker.Violation("docs/guide.md", 2, phrase)])

    def test_not_enterprise_ready(self):
        self.write("docs/guide.md", "It is NOT enterprise-ready.\nIt is not\nenterprise-ready.\n"
                   "It is **not** enterprise-ready.\nIt is not **enterprise-ready**.\n")
        self.write("website/sub/index.html", "<p>not <strong>enterprise-ready</strong></p>")
        self.assertEqual(checker.check(self.root), [])

    def test_not_prefix_must_be_a_word(self):
        self.write("docs/guide.md", "It is cannot enterprise-ready.\n")
        self.assertEqual(len(checker.check(self.root)), 1)

    def test_missing_landing_status(self):
        self.write("website/index.html", "<p>Welcome.</p>\n")
        self.assertEqual(checker.check(self.root), [checker.Violation("website/index.html", 1, "missing-status")])

    def test_missing_readme_status(self):
        self.write("README.md", "# Project\n")
        self.assertEqual(checker.check(self.root), [checker.Violation("README.md", 1, "missing-status")])

    def test_literal_not_stable_and_status_not_required_on_every_page(self):
        self.write("README.md", "**NOT STABLE**\n")
        self.write("website/index.html", "<strong>NOT STABLE</strong>\n")
        self.write("website/other/index.md", "Other page.\n")
        self.assertEqual(checker.check(self.root), [])

    def test_emphasized_status_words(self):
        self.write("README.md", "It is **not** yet **stable**.\n")
        self.assertEqual(checker.check(self.root), [])

    def test_version_two_relaxes_status_but_keeps_claim_rules(self):
        self.write("VERSION", "2.0.0\n")
        self.write("README.md", "# Project\n")
        self.write("website/index.html", "<p>Welcome.</p>\n")
        self.assertEqual(checker.check(self.root), [])
        self.write("website/other/index.md", "# Offer\nGuaranteed savings\n")
        result = self.cli()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout, "website/other/index.md:2: forbidden claim: guaranteed savings\n")

    def test_quoted_examples(self):
        self.write("docs/guide.md", '''> production-ready
> enterprise-ready
"stable release" and 'battle-tested' and “guaranteed savings” and ‘saves tokens by default’
`production-ready`
```text
enterprise-ready
```
~~~
stable release
~~~
<blockquote>battle-tested</blockquote><q>guaranteed savings</q>
<pre>saves tokens by default</pre><code>production-ready</code>
<!-- enterprise-ready -->
''')
        self.assertEqual(checker.check(self.root), [])

    def test_quote_does_not_hide_following_claim(self):
        self.write("docs/guide.md", '"production-ready"\nBattle-tested\n')
        self.assertEqual(checker.check(self.root), [checker.Violation("docs/guide.md", 2, "battle-tested")])

    def test_quoted_or_hidden_status_is_not_a_notice(self):
        for text in ('> NOT STABLE', '`NOT STABLE`', '"NOT STABLE"',
                     '<!-- NOT STABLE -->', '<q>NOT STABLE</q>',
                     '<script>"NOT STABLE"</script>', '<meta content="NOT STABLE">'):
            with self.subTest(text=text):
                self.write("README.md", text)
                self.assertEqual(checker.check(self.root), [checker.Violation("README.md", 1, "missing-status")])

    def test_html_attributes_entities_and_split_claim(self):
        self.write("website/other/index.html", '<meta content="guaranteed savings">\n'
                   '<p>production&#45;ready and stable <b>release</b></p>\n')
        self.assertEqual(checker.check(self.root), [
            checker.Violation("website/other/index.html", 1, "guaranteed savings"),
            checker.Violation("website/other/index.html", 2, "production-ready"),
            checker.Violation("website/other/index.html", 2, "stable release"),
        ])

    def test_allowlist_is_exact_and_requires_reason(self):
        self.write("docs/guide.md", "production-ready\nproduction-ready\n")
        self.write("scripts/status_claims_allow.txt", "docs/guide.md:1:production-ready | Historical example under review\n")
        self.assertEqual(checker.check(self.root), [checker.Violation("docs/guide.md", 2, "production-ready")])
        for entry in ("docs/guide.md:1:production-ready |", "docs/*.md:1:production-ready | Reason",
                      "../guide.md:1:production-ready | Reason", "docs/guide.md:1:unknown | Reason"):
            with self.subTest(entry=entry):
                self.write("scripts/status_claims_allow.txt", entry)
                result = self.cli()
                self.assertEqual(result.returncode, 2)
                self.assertIn("invalid exception", result.stderr)

    def test_scan_scope(self):
        for name in ("docs/nested/ignored.md", "other.md", "website/data.json"):
            self.write(name, "guaranteed savings")
        self.write("website/nested/claim.markdown", "battle-tested")
        self.assertEqual(checker.check(self.root), [checker.Violation("website/nested/claim.markdown", 1, "battle-tested")])

    def test_usage_errors(self):
        for version in ("invalid", "1", "1.2", "-1.2.3"):
            with self.subTest(version=version):
                self.write("VERSION", version)
                self.assertEqual(self.cli().returncode, 2)
        self.write("VERSION", "1.3.1")
        (self.root / "VERSION").unlink()
        self.assertEqual(self.cli().returncode, 2)
        self.assertEqual(self.cli("--unknown").returncode, 2)


if __name__ == "__main__":
    unittest.main()
