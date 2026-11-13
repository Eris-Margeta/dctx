# SPDX-License-Identifier: Apache-2.0
"""Exercise the docs gate with an executable help-only stub and source fixtures."""

from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap
import unittest


CHECKER = Path(__file__).with_name("check_cli_docs.py").resolve()
STUB = '''#!/usr/bin/env python3
import sys
pages = {
    (): """Usage: dctx <COMMAND> [OPTIONS]
       dctx pack [PATH] [--output PATH] [--json]
       dctx map <find|read> <TARGET> [--json]
       dctx cache status [--json]
       dctx cache purge --root PATH [--dry-run]
       dctx plugin info <id> [--json]
       dctx trace <A> <B> [--json]

General Options:
  -h, --help     Show help
  -v, --version  Version
""",
    ('pack',): """Usage: dctx pack [PATH] [OPTIONS]

Options:
  --output, -o PATH  Destination
  --for TEXT        Task
  --json            JSON
  --budget N        Tokens
""",
    ('map',): """Usage: dctx map <find|read> <TARGET> [OPTIONS]

Options:
  --json       JSON
  --budget N   Tokens
""",
    ('map', 'find'): """Usage: dctx map find <QUERY> [--limit N] [--json]
""",
    ('map', 'read'): """Usage: dctx map read <ID> [--full] [--json]
""",
    ('plugin',): """Usage: dctx plugin info <id> [--json]
""",
    ('trace',): """Usage: dctx trace <A> <B> [--json]
Example: dctx trace main run
""",
}
assert sys.argv[-1:] == ['--help'], 'documented commands must never execute'
key = tuple(sys.argv[1:-1])
if key[:1] == ('cache',):
    key = ()  # dctx's cache help repeats the global synopsis.
if key not in pages:
    sys.exit(2)
print(pages[key])
'''


class DocsGateTests(unittest.TestCase):
    def setUp(self):
        # Keep fixtures and all subprocess state inside this worktree.
        self.temp = tempfile.TemporaryDirectory(dir=CHECKER.parent.parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.binary = self.root / "stub-dctx"
        self.binary.write_text(STUB, encoding="utf-8")
        self.binary.chmod(0o755)

    def write(self, name, content):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(content).lstrip("\n"), encoding="utf-8")

    def run_gate(self, *args, default=False):
        command = [sys.executable, "-B", str(CHECKER)]
        if not default:
            command.extend(["--binary", str(self.binary)])
        return subprocess.run([*command, *args], cwd=self.root, capture_output=True,
                              text=True, check=False)

    def test_ok(self):
        self.write("README.md", '''
            ```sh
            dctx --version
            dctx pack . --output result.txt --json
            dctx map find 'two words' --limit 5 --budget 500
            dctx cache status --json
            dctx cache purge --root . --dry-run
            ```
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checked 5 commands; 0 drift", result.stdout)

    def test_typo_verb(self):
        self.write("docs/usage.md", "```sh\ndctx pakk .\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("docs/usage.md:2: unknown verb: pakk", result.stdout)

    def test_typo_nested_verb(self):
        self.write("README.md", "```sh\ndctx map fnd NAME\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("README.md:2: unknown verb: fnd", result.stdout)

    def test_unknown_flag(self):
        self.write("README.md", "```sh\ndctx pack . --jsno\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("README.md:2: unknown flag: --jsno", result.stdout)

    def test_flags_are_scoped_to_command(self):
        self.write("README.md", "```sh\ndctx cache status --dry-run\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("unknown flag: --dry-run", result.stdout)

    def test_legacy_skipped(self):
        self.write("README.md", '''
            Legacy example with
            a description on another line:

            ```sh
            dctx obsolete --bad
            ```
            ```sh legacy
            dctx obsolete --bad
            ```
            ```sh
            # legacy
            dctx obsolete --bad
            dctx pack .
            ```
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checked 1 commands", result.stdout)

    def test_text_blocks_and_inline_prose_skipped(self):
        self.write("README.md", '''
            `dctx obsolete --bad` is prose.
            ```text
            dctx obsolete --bad
            ```
            ~~~text
            dctx obsolete --bad
            ~~~
            ```toml
            dctx = { version = "1" }
            ```
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checked 0 commands", result.stdout)

    def test_html_code_blocks_parsed(self):
        self.write("website/docs/index.html", '''
            <p>Example</p>
            <pre><code>$ <span>dctx</span> map <b>find</b> &quot;two words&quot; --json</code></pre>
            <pre><code>
            dctx <span>pakk</span> .
            dctx pack . <span>--jsno</span>
            </code></pre>
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("website/docs/index.html:4: unknown verb: pakk", result.stdout)
        self.assertIn("website/docs/index.html:5: unknown flag: --jsno", result.stdout)
        self.assertIn("Checked 3 commands; 2 drift", result.stdout)

    def test_html_text_and_legacy_skipped(self):
        self.write("website/index.html", '''
            <pre><code class="language-text">dctx obsolete --bad</code></pre>
            <p>Legacy <strong>example</strong>:</p>
            <pre><code>dctx obsolete --bad</code></pre>
            <pre class="legacy"><code>dctx obsolete --bad</code></pre>
            <pre><code>dctx pack . --json</code></pre>
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checked 1 commands", result.stdout)

    def test_html_terminal_line_spans_and_output(self):
        self.write("website/index.html", '''
            <pre><span class="ln cmd">$ <span>dctx</span> --version</span><span class="ln out">dctx 1.3.1</span></pre>
            <pre><span class="ln cmd">$ dctx pack . --json</span><span class="ln out">--bad</span><span class="ln cmd">$ dctx typo</span></pre>
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("website/index.html:2: unknown verb: typo", result.stdout)
        self.assertIn("Checked 3 commands; 1 drift", result.stdout)

    def test_parent_synopsis_when_leaf_help_unavailable(self):
        self.write("README.md", "```sh\ndctx plugin info example/plugin --json\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_menu_banner_and_version_output_are_not_commands(self):
        self.write("README.md", "```\n  dctx - Directory Context Generator\ndctx 1.3.1\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("Checked 0 commands", result.stdout)

    def test_example_prefix_is_checked(self):
        self.write("README.md", "```sh\nExample: dctx pack . --typo\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("README.md:2: unknown flag: --typo", result.stdout)

    def test_help_examples_do_not_turn_arguments_into_verbs(self):
        self.write("README.md", "```sh\ndctx trace main dispatch --json\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_multiline_command_reports_flag_line(self):
        self.write("README.md", "```sh\ndctx map find NAME \\\n  --budget 50 \\\n  --jsno\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertIn("README.md:4: unknown flag: --jsno", result.stdout)

    def test_values_equals_comments_and_double_dash(self):
        self.write("README.md", '''
            ```sh
            dctx pack . --for '--not-a-flag' --budget=500 -o result.txt # --bad
            dctx pack . --for='words|with|pipes'
            dctx map find -- --not-a-flag
            dctx pack . --json | other-tool --not-a-dctx-flag
            ```
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_synopsis_choices(self):
        self.write("README.md", '''
            ```
            Usage: dctx <COMMAND> [OPTIONS]
                   dctx map <find|read> <TARGET> [--json]
                   dctx pack [PATH] [--output PATH]
            ```
        ''')
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_scans_only_requested_sources(self):
        for name in ("docs/guide.md", "website/deep/example.md", "website/page.html"):
            self.write(name, "<pre>dctx typo</pre>" if name.endswith("html") else "```sh\ndctx typo\n```\n")
        self.write("docs/archive/old.md", "```sh\ndctx ignored\n```\n")
        self.write("other.md", "```sh\ndctx ignored\n```\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stdout.count("unknown verb: typo"), 3)
        self.assertNotIn("ignored", result.stdout)

    def test_missing_default_binary_is_note_and_success(self):
        result = self.run_gate(default=True)
        self.assertEqual(result.returncode, 0)
        self.assertIn("NOTE:", result.stdout)

    def test_default_binary_debug_then_release(self):
        for profile in ("release", "debug"):
            with self.subTest(profile=profile):
                path = self.root / "target" / profile / "dctx"
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(STUB, encoding="utf-8")
                path.chmod(0o755)
                result = self.run_gate(default=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn("NOTE:", result.stdout)

    def test_usage_errors(self):
        for args in (("--unknown",), ("--binary",), ("--binary", "absent")):
            with self.subTest(args=args):
                result = self.run_gate(*args)
                self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
