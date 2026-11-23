# SPDX-License-Identifier: Apache-2.0
"""Fast gate tests using a shell CLI and injected clocks, never timing sleeps."""
import contextlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts import speed_gate as gate


STUB = """#!/bin/sh
set -eu
case "$*" in
    --help) printf 'Usage: dctx map <COMMAND>\\n       dctx pack [PATH]\\n'; exit 0 ;;
    'map --help') printf 'Usage: dctx map export [--place PATH]\\n'; exit 0 ;;
    'map export'|'pack .') ;;
    *) exit 2 ;;
esac
set -- group_*/*.rs
count=$#
case "$SPEED_GATE_TEST_MODE" in
    quadratic) cost=$((count * count)) ;;
    boundary) cost=$((count - 32)) ;;
    *) cost=$count ;;
esac
printf '%s\\n' "$cost" > "$SPEED_GATE_TEST_COST"
if [ "$SPEED_GATE_TEST_MODE" = nondeterministic ]; then
    counter=0
    if [ -f ../counter ]; then read -r counter < ../counter; fi
    counter=$((counter + 1))
    printf '%s\\n' "$counter" > ../counter
    printf 'run %s\\n' "$counter"
else
    printf 'files %s\\n' "$count"
fi
"""


class StubTimer:
    """The shell reports synthetic work; elapsed time never depends on load."""

    def __init__(self, cost):
        self.cost = cost
        self.start = True
        self.calls = 0

    def __call__(self):
        self.calls += 1
        self.start = not self.start
        return 0.0 if not self.start else float(self.cost.read_text())


class SpeedGateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="speed-test-", dir=gate.WORKTREE)
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = self.root / "dctx-stub"
        self.binary.write_text(STUB, encoding="utf-8")
        self.binary.chmod(0o755)

    def run_stub(self, mode, files=80, *, argv=None):
        clock = StubTimer(self.root / "cost")
        output = io.StringIO()
        with patch.dict(os.environ, {"SPEED_GATE_TEST_MODE": mode,
                                    "SPEED_GATE_TEST_COST": str(clock.cost)}), patch.object(
            gate, "BASE_FILES", files
        ), contextlib.redirect_stdout(output):
            if argv is None:
                argv = ["--binary", str(self.binary)]
            status = gate.main(argv, timer=clock)
        return status, output.getvalue(), clock.calls

    def test_linear_passes_both_commands_and_all_five_runs(self):
        status, output, calls = self.run_stub("linear")
        self.assertEqual(status, 0)
        self.assertEqual(output.count("PASS"), 2)
        self.assertEqual(output.count("4.000x"), 2)
        self.assertIn("map export", output)
        self.assertIn("pack", output)
        self.assertEqual(calls, 2 * 2 * 5 * 2)

    def test_quadratic_fails_both_commands(self):
        status, output, _ = self.run_stub("quadratic")
        self.assertEqual(status, 1)
        self.assertEqual(output.count("16.000x"), 2)
        self.assertEqual(output.count("FAIL"), 2)

    def test_nondeterministic_fails_despite_linear_time(self):
        status, output, _ = self.run_stub("nondeterministic")
        self.assertEqual(status, 1)
        self.assertEqual(output.count("4.000x"), 2)
        self.assertEqual(output.count("NO | FAIL"), 2)

    def test_exactly_six_times_is_allowed(self):
        status, output, _ = self.run_stub("boundary")
        self.assertEqual(status, 0)
        self.assertEqual(output.count("6.000x"), 2)

    def test_median_ignores_outliers_and_compares_output_bytes(self):
        clock = iter([0, 100, 0, 4, 0, 2, 0, 3, 0, 1])
        with patch.object(gate, "invoke", side_effect=[b"aa", b"aa", b"ab", b"aa", b"aa"]):
            result = gate.measure(self.binary, ["pack", "."], self.root,
                                  self.root / "state", lambda: next(clock))
        self.assertEqual(result.median, 3)
        self.assertEqual(result.output_bytes, 2)
        self.assertFalse(result.deterministic)

    def test_measurement_uses_distinct_cold_caches(self):
        clock = iter([0, 1] * 5)
        with patch.object(gate, "invoke", return_value=b"") as invoke:
            result = gate.measure(self.binary, ["map", "export"], self.root,
                                  self.root / "state", lambda: next(clock))
        self.assertTrue(result.deterministic)
        caches = [call.args[3]["DCTX_CACHE_DIR"] for call in invoke.call_args_list]
        self.assertEqual(len(set(caches)), 5)
        self.assertTrue(all(Path(cache).is_relative_to(self.root) for cache in caches))

    def test_default_fixture_has_2000_small_files_and_40_dirs(self):
        root = self.root / "fixture"
        gate.generate_fixture(root)
        sources = list(root.rglob("*.rs"))
        self.assertEqual(len(sources), 2000)
        self.assertEqual(len(list(root.iterdir())), 40)
        self.assertTrue(all(source.stat().st_size < 100 for source in sources))

    def test_seeded_fixture_is_identical_and_scales_fourfold(self):
        trees = []
        for name, count in (("a", 40), ("b", 40), ("large", 160)):
            root = self.root / name
            gate.generate_fixture(root, count)
            trees.append({p.relative_to(root): p.read_bytes() for p in root.rglob("*.rs")})
            self.assertEqual(len(list(root.iterdir())), 40)
        self.assertEqual(trees[0], trees[1])
        self.assertEqual(len(trees[2]), 4 * len(trees[0]))
        self.assertTrue(all(trees[2][path] == data for path, data in trees[0].items()))

    def test_release_preferred_and_cargo_binary_name_supported(self):
        for profile, name in (("debug", "dctx"), ("release", "dctx-cli")):
            destination = self.root / "target" / profile / name
            destination.parent.mkdir(parents=True)
            destination.write_text(STUB, encoding="utf-8")
            destination.chmod(0o755)
        expected = self.root / "target" / "release" / "dctx-cli"
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "", "CARGO_BUILD_TARGET_DIR": ""}):
            self.assertEqual(gate.find_binary(None, self.root), expected)
        self.assertEqual(gate.find_binary(self.binary, self.root, target_dir=self.root / "target"),
                         self.binary)

    def assert_invalid_binary(self, binary):
        with patch.object(gate, "generate_fixture") as generate, \
                contextlib.redirect_stdout(io.StringIO()) as stdout, \
                contextlib.redirect_stderr(io.StringIO()) as stderr:
            status = gate.main(["--binary", str(binary)])
        self.assertEqual(status, 2)
        detail = json.dumps(f"explicit binary is missing or not executable: {binary}")
        self.assertEqual(stderr.getvalue(), f"ERROR: speed gate could not run: {detail}\n")
        self.assertEqual(stdout.getvalue(), "")
        generate.assert_not_called()

    def test_missing_explicit_binary_is_setup_error(self):
        self.assert_invalid_binary(self.root / "absent")

    def test_missing_explicit_binary_escapes_hostile_path(self):
        self.assert_invalid_binary(self.root / 'absent\n\x1b[31m"')

    def test_nonexecutable_explicit_binary_is_setup_error(self):
        self.binary.chmod(0o644)
        self.assert_invalid_binary(self.binary)

    def test_automatic_discovery_uses_alternate_target(self):
        target = self.root / "alternate-target"
        destination = target / "debug" / "dctx-cli"
        destination.parent.mkdir(parents=True)
        self.binary.rename(destination)
        for argv, env in (
            (["--target-dir", str(target)], {"CARGO_TARGET_DIR": str(self.root / "absent")}),
            ([], {"CARGO_TARGET_DIR": str(target), "CARGO_BUILD_TARGET_DIR": str(self.root / "absent")}),
            ([], {"CARGO_TARGET_DIR": "", "CARGO_BUILD_TARGET_DIR": str(target)}),
        ):
            with self.subTest(argv=argv, env=env), patch.dict(os.environ, env):
                status, output, calls = self.run_stub("linear", argv=argv)
                self.assertEqual(status, 0)
                self.assertEqual(output.count("PASS"), 2)
                self.assertEqual(calls, 2 * 2 * 5 * 2)

    def test_automatic_discovery_uses_cargo_file_configuration(self):
        (self.root / "Cargo.toml").write_text('[workspace]\nresolver = "2"\n', encoding="utf-8")
        config_dir = self.root / ".cargo"
        config_dir.mkdir()
        (config_dir / "config.toml").write_text(
            '[build]\ntarget-dir = "alternate-target"\n', encoding="utf-8"
        )
        destination = self.root / "alternate-target" / "debug" / "dctx"
        destination.parent.mkdir(parents=True)
        self.binary.rename(destination)
        with patch.dict(os.environ), patch.object(gate, "WORKTREE", self.root):
            os.environ.pop("CARGO_TARGET_DIR", None)
            os.environ.pop("CARGO_BUILD_TARGET_DIR", None)
            status, output, calls = self.run_stub("linear", argv=[])
        self.assertEqual(status, 0)
        self.assertEqual(output.count("PASS"), 2)
        self.assertEqual(calls, 2 * 2 * 5 * 2)

    def test_no_implicit_binary_skips_with_note(self):
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "", "CARGO_BUILD_TARGET_DIR": ""}), \
                patch.object(gate, "WORKTREE", self.root), \
                patch.object(gate, "generate_fixture") as generate, \
                contextlib.redirect_stdout(io.StringIO()) as output:
            status = gate.main([])
        self.assertEqual(status, 0)
        self.assertIn("NOTE", output.getvalue())
        generate.assert_not_called()

    def test_command_error_is_not_a_regression_verdict(self):
        self.binary.write_text("#!/bin/sh\nexit 3\n", encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()) as output:
            status = gate.main(["--binary", str(self.binary)])
        self.assertEqual(status, 2)
        self.assertIn("ERROR", output.getvalue())

    def test_unsupported_help_is_an_error(self):
        self.binary.write_text("#!/bin/sh\nprintf 'unknown CLI\\n'\n", encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()):
            status = gate.main(["--binary", str(self.binary)])
        self.assertEqual(status, 2)

    def test_help_exits_successfully(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaises(SystemExit) as caught:
                gate.main(["--help"])
        self.assertEqual(caught.exception.code, 0)
        self.assertIn("--binary", output.getvalue())

    def test_invalid_argument_escapes_hostile_text(self):
        argument = '--unexpected\n\x1b[31m"'
        with contextlib.redirect_stderr(io.StringIO()) as output:
            with self.assertRaises(SystemExit) as caught:
                gate.main([argument])
        self.assertEqual(caught.exception.code, 2)
        diagnostic = json.dumps(f"unrecognized arguments: {argument}")
        self.assertIn(f"speed_gate.py: error: {diagnostic}\n", output.getvalue())
        self.assertNotIn("\x1b", output.getvalue())

    def test_zero_time_ratio_is_defined(self):
        self.assertEqual(gate.time_ratio(0, 0), 1)
        self.assertEqual(gate.time_ratio(0, 1), float("inf"))


if __name__ == "__main__":
    unittest.main()
