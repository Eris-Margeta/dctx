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
                    else:
                        self.assertFalse(fail)
                    self.assertEqual(lockfile.read_bytes(), original)

    def run_fixture_gate(self, outputs, require_strict=True):
        stream = io.StringIO()
        results = [subprocess.CompletedProcess([], 0, output, "") for output in outputs]
        with patch.object(isolation.shutil, "which", return_value="/fake/cargo"), \
             patch.object(isolation, "_cargo_environment", return_value={}), \
             patch.object(isolation, "_run_cargo", side_effect=results) as runner, \
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
                     patch.object(isolation, "_run_cargo", side_effect=results) as runner, \
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

    def test_concurrent_lockfile_edit_is_kept_and_warned(self):
        with tempfile.TemporaryDirectory() as directory:
            lockfile = Path(directory) / "Cargo.lock"
            lockfile.write_bytes(b"ORIGINAL")
            real_read = Path.read_bytes

            def raced(self):
                data = real_read(self)
                if self == lockfile and data == b"GATE":
                    self.write_bytes(b"OTHER")
                    return b"GATE"
                return data

            stream = io.StringIO()
            with patch.object(Path, "read_bytes", raced), redirect_stdout(stream):
                with isolation._preserve_lockfile(lockfile):
                    lockfile.write_bytes(b"GATE")
            self.assertEqual(real_read(lockfile), b"OTHER")
            self.assertIn(isolation._LOCK_WARNING, stream.getvalue())

    def test_absent_lockfile_is_never_written(self):
        with tempfile.TemporaryDirectory() as directory:
            lockfile = Path(directory) / "Cargo.lock"
            real_write, writes = Path.write_bytes, []

            def spy(self, data):
                writes.append(bytes(data))
                return real_write(self, data)

            with patch.object(Path, "write_bytes", spy):
                with isolation._preserve_lockfile(lockfile):
                    self.assertFalse(lockfile.exists())
                with isolation._preserve_lockfile(lockfile):
                    lockfile.write_bytes(b"from-cargo")
            self.assertEqual(writes, [b"from-cargo"])
            self.assertEqual(lockfile.read_bytes(), b"from-cargo")

    def test_ps_cpu_time_uses_no_shell(self):
        samples = (
            ("1:02.50", 62.5, 0), ("01:02:03", 3723.0, 0),
            ("1-00:00:01", 86401.0, 0), ("", None, 0), ("bogus", None, 1),
        )
        for raw, expected, code in samples:
            completed = subprocess.CompletedProcess([], code, raw, "")
            with self.subTest(raw=raw), patch.object(
                isolation.subprocess, "run", return_value=completed,
            ) as run:
                self.assertEqual(isolation._process_cpu_seconds(9), expected)
            self.assertEqual(run.call_args.args[0], ["ps", "-o", "time=", "-p", "9"])
            self.assertIs(run.call_args.kwargs.get("shell"), False)

    def test_progressing_child_finishes_and_silent_child_is_killed(self):
        for kind in ("output", "cpu"):
            child, clock, outcome = _drive(kind, isolation._IDLE_SECONDS + 50)
            self.assertFalse(child.killed)
            self.assertGreaterEqual(clock["t"], isolation._IDLE_SECONDS)
            self.assertEqual(outcome[0], 0)
            if kind == "output":
                self.assertGreater(len(outcome[1]), 0)
        child, clock, error = _drive("silent", None)
        self.assertTrue(child.killed)
        self.assertEqual(error.rule, "no-progress")
        self.assertIn("liveness rule no-progress fired", str(error))
        self.assertGreaterEqual(clock["t"], isolation._IDLE_SECONDS)
        self.assertLess(clock["t"], isolation._CEILING_SECONDS)
        _child, clock, error = _drive("grow", None)
        self.assertEqual(error.rule, "hard-ceiling")
        self.assertIn("hard-ceiling", str(error))
        self.assertGreaterEqual(clock["t"], isolation._CEILING_SECONDS)
        stream = io.StringIO()
        with patch.object(isolation.shutil, "which", return_value="/fake/cargo"), \
             patch.object(isolation, "_cargo_environment", return_value={}), \
             patch.object(
                 isolation, "_run_cargo",
                 side_effect=isolation.CargoLivenessExpired("no-progress", 600),
             ), redirect_stdout(stream):
            self.assertEqual(isolation.run_gate(True), 1)
        self.assertIn("liveness rule no-progress fired", stream.getvalue())


class _Child:
    def __init__(self):
        self.pid, self.returncode, self.killed = 50, None, False

    def poll(self):
        return self.returncode

    def kill(self):
        self.killed, self.returncode = True, -9

    def wait(self, timeout=None):
        return self.returncode


def _drive(kind, exit_at):
    child, clock, ticks = _Child(), {"t": 0.0}, {"cpu": 0.0}

    def sleep(seconds):
        if kind == "silent":
            assert child.killed is False
        clock["t"] += seconds

    def read_output(proc):
        if exit_at is not None and clock["t"] >= exit_at:
            proc.returncode = 0
            return b"", b""
        if kind == "output":
            return b".", b""
        if kind == "grow":
            return b"x", b""
        return b"", b""

    def cpu(pid):
        if kind == "cpu":
            ticks["cpu"] += 1.0
            return ticks["cpu"]
        return 1.0 if kind == "silent" else None

    try:
        outcome = isolation._wait_lively(
            child, read_output, cpu, lambda: clock["t"], sleep,
        )
    except isolation.CargoLivenessExpired as error:
        return child, clock, error
    return child, clock, outcome


if __name__ == "__main__":
    unittest.main()
