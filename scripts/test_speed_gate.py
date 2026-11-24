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


MAP = b'''{"nodes":[
{"id":"m:group_00/item_00000.rs","kind":"module","path":"group_00/item_00000.rs","lines":[1,1]},
{"id":"s:group_00/item_00000.rs#item_00000","kind":"fn","path":"group_00/item_00000.rs","lines":[1,1]}
],"edges":[]}'''
PACK = b'''<FILE_CONTENT_START ID="F001" PATH="group_00/item_00000.rs" LINES="1">
pub fn item_00000() -> u32 { 29134 }
</FILE_CONTENT_END ID="F001">
'''


STUB = """#!/bin/sh
set -eu
case "$*" in
    --help) printf 'Usage: dctx map <COMMAND>\\n       dctx pack [PATH]\\n'; exit 0 ;;
    'map --help') printf 'Usage: dctx map export [--place PATH]\\n'; exit 0 ;;
    'map export') arm=map; body=$SPEED_GATE_TEST_MAP ;;
    'pack .') arm=pack; body=$SPEED_GATE_TEST_PACK ;;
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
case "$SPEED_GATE_TEST_MODE" in
    "$arm-empty") exit 0 ;;
    "$arm-whitespace") printf ' \\t\\n'; exit 0 ;;
    "$arm-unrelated") printf 'unrelated\\n\\033[31m"\\n'; exit 0 ;;
esac
printf '%s' "$body"
if [ "$SPEED_GATE_TEST_MODE" = nondeterministic ]; then
    counter=0
    if [ -f ../counter ]; then read -r counter < ../counter; fi
    counter=$((counter + 1))
    printf '%s\\n' "$counter" > ../counter
    printf '%*s\\n' "$counter" ''
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
                                    "SPEED_GATE_TEST_MAP": MAP.decode(),
                                    "SPEED_GATE_TEST_PACK": PACK.decode(),
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

    def test_zero_exit_with_useless_output_fails_only_that_workload(self):
        for command, arm in (("map export", "map"), ("pack", "pack")):
            for mode in ("empty", "whitespace", "unrelated"):
                with self.subTest(arm=arm, mode=mode), contextlib.redirect_stderr(io.StringIO()) as err:
                    status, output, _ = self.run_stub(f"{arm}-{mode}", files=40)
                    self.assertEqual(status, 1)
                    row = next(line for line in output.splitlines() if line.startswith(f"| {command} |"))
                    self.assertTrue(row.endswith("| NO | yes | FAIL |"))
                    self.assertEqual(output.count("PASS"), 1)
                    self.assertNotIn("unrelated", output)
                    self.assertNotIn("\x1b", output)
                    self.assertEqual(err.getvalue(), "")

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
        with patch.object(gate, "invoke", side_effect=[PACK, PACK, PACK + b" ", PACK, PACK]):
            result = gate.measure(self.binary, ["pack", "."], self.root,
                                  self.root / "state", lambda: next(clock))
        self.assertEqual(result.median, 3)
        self.assertEqual(result.output_bytes, len(PACK))
        self.assertTrue(result.fixture_evidence)
        self.assertFalse(result.deterministic)

    def test_measurement_uses_distinct_cold_caches(self):
        clock = iter([0, 1] * 5)
        with patch.object(gate, "invoke", return_value=MAP) as invoke:
            result = gate.measure(self.binary, ["map", "export"], self.root,
                                  self.root / "state", lambda: next(clock))
        self.assertTrue(result.deterministic)
        self.assertTrue(result.fixture_evidence)
        caches = [call.args[3]["DCTX_CACHE_DIR"] for call in invoke.call_args_list]
        self.assertEqual(len(set(caches)), 5)
        self.assertTrue(all(Path(cache).is_relative_to(self.root) for cache in caches))

    def test_late_invalid_sample_checked_after_timing(self):
        clock = StubTimer(self.root / "cost")
        clock.cost.write_text("1", encoding="ascii")
        checked_at = []
        validate = gate.has_fixture_output

        def check_output(args, output):
            checked_at.append(clock.calls)
            return validate(args, output)

        with patch.object(gate, "invoke", side_effect=[MAP] * 4 + [b'{"nodes":[],"edges":[]}']), \
                patch.object(gate, "has_fixture_output", side_effect=check_output):
            result = gate.measure(self.binary, ["map", "export"], self.root,
                                  self.root / "state", clock)
        self.assertEqual(checked_at, [2, 10])
        self.assertFalse(result.fixture_evidence)
        self.assertFalse(result.deterministic)

    def test_fixture_evidence_requires_command_specific_content(self):
        for args, body in (
            (["map", "export"], PACK),
            (["map", "export"], MAP.replace(b"#item_00000", b"#different")),
            (["pack", "."], MAP),
            (["pack", "."], PACK.replace(b"pub fn item_00000", b"pub fn different")),
        ):
            with self.subTest(command=args):
                self.assertFalse(gate.has_fixture_output(args, body))

    @unittest.skipUnless("DCTX_TEST_BIN" in os.environ, "real CLI requires DCTX_TEST_BIN")
    def test_real_cli_outputs_match_seeded_fixture(self):
        binary = Path(os.environ["DCTX_TEST_BIN"]).resolve()
        fixture = self.root / "fixture"
        gate.generate_fixture(fixture, gate.DIRECTORIES)
        for name, command in gate.discover_commands(binary, self.root / "help").items():
            with self.subTest(command=name):
                output = gate.invoke(binary, command, fixture, gate.isolated_env(self.root / name))
                self.assertTrue(gate.has_fixture_output(command, output))

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
