"""Fixture and adapter controls for the PP6 normal-closure gate.

Uses unittest and mocks from the standard library, with no Cargo or network.
PL-V runs these controls before collecting the three real isolation verdicts.
"""

from contextlib import redirect_stdout
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import isolation


FACADE = "dctx v0.1.0 (/workspace/crates/dctx)\ndctx-core v0.1.0\n"
STRICT = "dctx-cli v0.25.0 (/workspace/crates/dctx-cli)\n" + FACADE
DEFAULT = STRICT + "dctx-plugin-git v0.1.0 (/workspace/plugins/dctx-plugin-git)\n"


class ParserTests(unittest.TestCase):
    def test_passing_closures(self):
        self.assertEqual(isolation.parse_cargo_tree(FACADE), {"dctx", "dctx-core"})
        isolation.check_closure(FACADE, "dctx", True)
        isolation.check_closure(STRICT, "dctx-cli", True)
        isolation.check_closure(DEFAULT, "dctx-cli", False)

    def test_versions_paths_duplicates_and_feature_suffixes(self):
        output = (
            "sample v1.2.3-alpha.1+build.2 (/a path/(nested)/sample) [default, std]\n"
            "sample v1.2.3 (*)\n"
            "macros v0.2.0 (proc-macro)\n"
            "serde v1.0.219 [features: derive, std]\n"
        )
        self.assertEqual(isolation.parse_cargo_tree(output), {"sample", "macros", "serde"})

    def test_displayed_unicode_and_ascii_trees(self):
        output = (
            "dctx v0.1.0\n├── alpha v1.0.0\n│   └── beta v2.0.0\n"
            "└── gamma v3.0.0 (*)\n"
            "|-- delta v1.0.0\n|   `-- epsilon v1.0.0\n"
        )
        self.assertEqual(len(isolation.parse_cargo_tree(output)), 6)

    def test_empty_is_not_evidence(self):
        for output in ("", "\n \t\n"):
            with self.subTest(output=output), self.assertRaises(isolation.InvalidTree):
                isolation.parse_cargo_tree(output)

    def test_malformed_lines_fail_even_after_valid_root(self):
        for line in ("garbage", "dctx-plugin-host", "host v1.2", "host vbad",
                     "host v1.2.3 unexpected", "host v1.2.3 [unfinished",
                     'host feature "default"', "warning: no closure"):
            with self.subTest(line=line), self.assertRaises(isolation.InvalidTree):
                isolation.parse_cargo_tree(FACADE + line)

    def test_each_forbidden_dependency_is_rejected(self):
        for name in sorted(isolation.FORBIDDEN):
            with self.subTest(name=name), self.assertRaisesRegex(
                isolation.InvalidTree, name,
            ):
                isolation.check_closure(STRICT + f"{name} v0.1.0 (*)\n", "dctx-cli", True)

    def test_missing_positive_control_fails(self):
        with self.assertRaisesRegex(isolation.InvalidTree, "positive control missing"):
            isolation.check_closure(STRICT, "dctx-cli", False)

    def test_wrong_root_fails(self):
        with self.assertRaisesRegex(isolation.InvalidTree, "missing selected root"):
            isolation.check_closure("innocent v1.0.0\n", "dctx", True)

    def test_names_are_exact_not_substrings(self):
        isolation.check_closure(FACADE + "dctx-plugin-host-helper v1.0.0\n", "dctx", True)
        with self.assertRaises(isolation.InvalidTree):
            isolation.check_closure(STRICT + "dctx-plugin-git-helper v1.0.0\n",
                                    "dctx-cli", False)


class GateTests(unittest.TestCase):
    def test_lockfile_backup_restores_exact_bytes_on_success_and_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            lockfile = Path(directory) / "Cargo.lock"
            original = b"# original\r\nversion = 4\n\x00\xff"
            lockfile.write_bytes(original)
            for fail in (False, True):
                with self.subTest(fail=fail):
                    try:
                        with isolation._preserve_lockfile(lockfile):
                            self.assertEqual(lockfile.read_bytes(), original)
                            lockfile.write_bytes(b"updated resolution\n")
                            if fail:
                                raise RuntimeError("resolution interrupted")
                    except RuntimeError:
                        self.assertTrue(fail)
                    self.assertEqual(lockfile.read_bytes(), original)

    def run_fixture_gate(self, outputs, require_strict=True):
        stream = io.StringIO()
        results = [subprocess.CompletedProcess([], 0, output, "") for output in outputs]
        with patch.object(isolation.shutil, "which", return_value="/fake/cargo"), \
             patch.object(isolation, "_cargo_environment", return_value={}), \
             patch.object(isolation.subprocess, "run", side_effect=results) as runner, \
             redirect_stdout(stream):
            status = isolation.run_gate(require_strict)
        return status, stream.getvalue().splitlines(), runner.call_args_list

    def test_three_separate_commands_pass(self):
        status, lines, calls = self.run_fixture_gate([FACADE, STRICT, DEFAULT])
        self.assertEqual(status, 0)
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(line.startswith("PASS ") for line in lines))
        self.assertEqual(len(calls), 3)
        for call, (_, root, features, _) in zip(calls, isolation.PROFILES):
            command = call.args[0]
            self.assertEqual(command, [
                "/fake/cargo", "tree", "-p", root, *features, "-e", "normal",
                "--prefix", "none", "--color", "never", "--offline",
            ])
            self.assertNotIn("shell", call.kwargs)
        self.assertNotIn("--no-default-features", calls[2].args[0])

    def test_seeded_forbidden_dependency_fails_each_strict_invocation(self):
        for index in (0, 1):
            for name in ("dctx-plugin-host", "dctx-plugin-platform", "dctx-plugin-git",
                         "dctx-plugin-git-helper", "dctx-plugin-git2"):
                outputs = [FACADE, STRICT, DEFAULT]
                outputs[index] += f"{name} v0.1.0\n"
                with self.subTest(index=index, name=name):
                    status, lines, calls = self.run_fixture_gate(outputs)
                    self.assertEqual(status, 1)
                    self.assertTrue(lines[index].startswith("FAIL "))
                    self.assertIn(name, lines[index])
                    self.assertEqual(len(calls), 3)

    def test_management_and_wire_dependencies_pass_both_strict_invocations(self):
        management = ("dctx-plugin-distribution v1.0.0\n"
                      "dctx-plugin-protocol v1.0.0\n")
        status, lines, calls = self.run_fixture_gate(
            [FACADE + management, STRICT + management, DEFAULT],
        )
        self.assertEqual(status, 0)
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(line.startswith("PASS ") for line in lines))
        self.assertEqual(len(calls), 3)

    def test_default_positive_control_missing_fails_gate(self):
        status, lines, _ = self.run_fixture_gate([FACADE, STRICT, STRICT])
        self.assertEqual(status, 1)
        self.assertIn("positive control missing", lines[2])

    def test_empty_and_malformed_successful_cargo_output_fail_gate(self):
        for output in ("", FACADE + "unrecognized line"):
            with self.subTest(output=output):
                status, lines, _ = self.run_fixture_gate([output, STRICT, DEFAULT])
                self.assertEqual(status, 1)
                self.assertTrue(lines[0].startswith("FAIL "))

    def test_missing_cargo_required_fails_optional_explicitly_skips(self):
        for required, expected, verdict in ((True, 1, "FAIL"), (False, 0, "SKIP")):
            stream = io.StringIO()
            with patch.object(isolation.shutil, "which", return_value=None), \
                 redirect_stdout(stream):
                self.assertEqual(isolation.run_gate(required), expected)
            lines = stream.getvalue().splitlines()
            self.assertEqual(len(lines), 3)
            self.assertTrue(all(line.startswith(verdict) for line in lines))

    def test_invocation_failure_never_passes_and_remaining_profiles_run(self):
        failures = (
            subprocess.CompletedProcess([], 101, FACADE, "offline\nresolution failed"),
            subprocess.TimeoutExpired("cargo", 300),
            FileNotFoundError("cargo disappeared"),
        )
        for required in (True, False):
            for failure in failures:
                stream = io.StringIO()
                results = [failure, subprocess.CompletedProcess([], 0, STRICT, ""),
                           subprocess.CompletedProcess([], 0, DEFAULT, "")]
                with self.subTest(required=required, failure=failure), \
                     patch.object(isolation.shutil, "which", return_value="/fake/cargo"), \
                     patch.object(isolation, "_cargo_environment", return_value={}), \
                     patch.object(isolation.subprocess, "run", side_effect=results) as runner, \
                     redirect_stdout(stream):
                    self.assertEqual(isolation.run_gate(required), 1)
                self.assertEqual(len(stream.getvalue().splitlines()), 3)
                self.assertEqual(runner.call_count, 3)

    def test_environment_copies_only_registry_and_confines_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / "original"
            registry = original / "registry"
            registry.mkdir(parents=True)
            (registry / "cached").write_text("offline fixture")
            (original / "credentials.toml").write_text("not copied")
            scratch = root / "scratch"
            scratch.mkdir()
            with patch.dict(isolation.os.environ, {"CARGO_HOME": str(original)}):
                env = isolation._cargo_environment(scratch)
            copied = Path(env["CARGO_HOME"])
            self.assertEqual((copied / "registry" / "cached").read_text(), "offline fixture")
            self.assertFalse((copied / "credentials.toml").exists())
            self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
            for key in ("CARGO_HOME", "CARGO_TARGET_DIR", "TMPDIR", "TMP", "TEMP"):
                self.assertTrue(Path(env[key]).is_relative_to(scratch))

    def test_cache_error_is_explicit_for_every_profile(self):
        stream = io.StringIO()
        with patch.object(isolation.shutil, "which", return_value="/fake/cargo"), \
             patch.object(isolation, "_cargo_environment", side_effect=OSError("disk full")), \
             redirect_stdout(stream):
            self.assertEqual(isolation.run_gate(True), 1)
        self.assertEqual(len(stream.getvalue().splitlines()), 3)
        self.assertEqual(stream.getvalue().count("FAIL"), 3)


if __name__ == "__main__":
    unittest.main()
