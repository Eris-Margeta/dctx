# SPDX-License-Identifier: Apache-2.0
"""Check wrapper workflow wiring offline, without running the hosted jobs."""

from ast import literal_eval
from itertools import product
from pathlib import Path
import re
import subprocess
import tempfile
from textwrap import dedent
import unittest


ROOT = Path(__file__).resolve().parents[2]
NATIVE_RUNNERS = ("ubuntu-24.04-arm", "macos-15-intel")
ORIGINAL_RUNNERS = ("ubuntu-24.04", "macos-15", "windows-2022")
ENGINE = "target/debug/dctx-cli"
SOURCE_RUST = "source tools/release/toolchain.env"
BUILD = f'{SOURCE_RUST}\ncargo +"$DCTX_CI_RUST" build --locked -p dctx-cli'
INSTALL = f'{SOURCE_RUST}\nrustup toolchain install "$DCTX_CI_RUST" --profile minimal'
PYTHON_FLOOR = "matrix.python == '3.13'"
GUARD = dedent(r"""
    if [[ ! -f "$DCTX_TEST_BIN" || ! -x "$DCTX_TEST_BIN" ]]; then
      printf 'Expected an executable engine at %q\n' "$DCTX_TEST_BIN" >&2
      exit 1
    fi
""").strip()
GO_VERSION_CHECK = dedent(r"""
    if ! version=$(go env GOVERSION 2>&1) || [[ "$GOTOOLCHAIN" != local || "$version" != "go$EXPECTED_GO_VERSION" ]]; then
      printf 'Expected Go %q with GOTOOLCHAIN=local, got %q\n' "$EXPECTED_GO_VERSION" "$version" >&2
      exit 1
    fi
""").strip()


def block(text, indent, key, value=""):
    """Read a named block in the repository's indentation-based workflow checks."""
    header = re.escape(f"{key}:{value}")
    match = re.search(rf"(?m)^ {{{indent}}}{header}\n((?: {{{indent + 1}}}[^\n]*\n)*)", text)
    if match is None:
        raise AssertionError(f"Missing workflow block {key!a}")
    return match[1]


def fields(text, indent):
    return dict(re.findall(rf"(?m)^ {{{indent}}}([\w-]+): *(.*)$", text))


def matrix_rows(job):
    """Expand inline axes and include rows, as used by these wrapper jobs."""
    matrix = block(job, 6, "matrix")
    try:
        axes = {
            name: literal_eval(value)
            for name, value in fields(matrix, 8).items() if name != "include"
        }
        if any(not isinstance(values, list) for values in axes.values()):
            raise ValueError("Expected inline matrix axes")
        base = [dict(zip(axes, values)) for values in product(*axes.values())] if axes else []
        rows = [row.copy() for row in base]
        if "include" in fields(matrix, 8):
            include = block(matrix, 8, "include")
            for item in re.split(r"(?m)^          - ", include)[1:]:
                entry = {
                    key: literal_eval(value) if value.startswith(("'", '"')) else value
                    for key, value in fields(" " * 12 + item, 12).items()
                }
                matches = [
                    row for row, original in zip(rows, base)
                    if all(key not in original or original[key] == value for key, value in entry.items())
                ]
                if matches:
                    for row in matches:
                        row.update(entry)
                else:
                    rows.append(entry)
        return rows
    except (SyntaxError, ValueError):
        raise AssertionError(f"Unsupported matrix syntax in {matrix!a}") from None


def script(step):
    run = fields(step, 8).get("run")
    return dedent(block(step, 8, "run", " |")).strip() if run == "|" else run


class WrapperParityMatrix(unittest.TestCase):
    def same(self, actual, expected):
        if actual != expected:
            self.fail(f"Expected {expected!a}, got {actual!a}")

    def job(self, filename, name):
        try:
            workflow = (ROOT / ".github" / "workflows" / filename).read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            self.fail(f"Cannot read workflow {filename!a}. {str(error)!a}")
        workflow = "".join(line + "\n" for line in workflow.splitlines()
                           if line.strip() and not line.lstrip().startswith("#"))
        job = block(workflow, 2, name)
        self.same(fields(block(job, 6, "run"), 8), {"shell": "bash"})
        self.same(fields(job, 4).get("if"), None)
        self.same(fields(job, 4).get("continue-on-error"), None)
        steps = [" " * 8 + step for step in re.split(r"(?m)^      - ", block(job, 4, "steps"))[1:]]
        return job, steps

    def step(self, steps, value, *, key="name", condition=None, cwd=None, env=None):
        matches = [
            (index, step) for index, step in enumerate(steps)
            if fields(step, 8).get(key) == value
        ]
        if len(matches) != 1:
            self.fail(f"Expected one step with {key!a} set to {value!a}, found {len(matches)}")
        index, step = matches[0]
        properties = fields(step, 8)
        self.same(properties.get("if"), condition)
        self.same(properties.get("continue-on-error"), None)
        self.same(properties.get("shell"), None)
        self.same(properties.get("working-directory"), cwd)
        actual_env = fields(block(step, 8, "env"), 10) if "env" in properties else None
        self.same(actual_env, env)
        return index, step

    def ordered(self, *indices):
        self.same(list(indices), sorted(set(indices)))

    def test_go_matrix_builds_and_runs_required_parity(self):
        job, steps = self.job("go.yml", "transport")
        rows = matrix_rows(job)
        self.same(sorted((row.get("os", ""), row.get("go", "")) for row in rows), sorted([
            (runner, "stable") for runner in (*ORIGINAL_RUNNERS, *NATIVE_RUNNERS)
        ] + [("ubuntu-24.04", "1.26.10"), ("ubuntu-24.04", "1.23.12")]))
        self.same(fields(job, 4).get("runs-on"), "${{ matrix.os }}")
        env = fields(block(job, 4, "env"), 6)
        self.same(env.get("DCTX_TEST_BIN"), f"${{{{ github.workspace }}}}/{ENGINE}${{{{ matrix.exe }}}}")
        self.same(env.get("DCTX_REQUIRE_ENGINE_TESTS"), "'1'")
        self.same(env.get("GOTOOLCHAIN"), "${{ matrix.toolchain || 'auto' }}")
        for row in rows:
            with self.subTest(row=ascii(row)):
                self.same(row.get("exe"), ".exe" if row["os"] == "windows-2022" else "")
                self.same(row.get("toolchain"), "local" if row["go"] == "1.23.12" else None)
        setup, setup_step = self.step(
            steps, "actions/setup-go@4dc6199c7b1a012772edbd06daecab0f50c9053c # v6.1.0", key="uses",
        )
        self.same(fields(block(setup_step, 8, "with"), 10).get("go-version"), "${{ matrix.go }}")
        verify, verify_step = self.step(
            steps, "Verify effective Go floor", condition="matrix.toolchain == 'local'",
            cwd="packages/go", env={"EXPECTED_GO_VERSION": "${{ matrix.go }}"},
        )
        self.same(script(verify_step), GO_VERSION_CHECK)
        install, install_step = self.step(steps, "Install Rust toolchain")
        self.same(script(install_step), INSTALL + " --no-self-update")
        build, build_step = self.step(steps, "Build matching engine")
        self.same(script(build_step), BUILD)
        parity, parity_step = self.step(
            steps, "Transport, race and every-verb CLI parity tests (no skips)", cwd="packages/go",
        )
        self.same(script(parity_step), GUARD + "\ngo test -race -count=1 -timeout 3m -v ./...")
        self.ordered(setup, verify, install, build, parity)

    def test_node_matrix_builds_and_runs_required_parity(self):
        job, steps = self.job("npm.yml", "npm")
        rows = matrix_rows(job)
        self.same(sorted((row.get("os", ""), row.get("node", "")) for row in rows), sorted(
            list(product(ORIGINAL_RUNNERS, ("18", "24"))) + [(runner, "24") for runner in NATIVE_RUNNERS]
        ))
        self.same(fields(job, 4).get("runs-on"), "${{ matrix.os }}")
        setup, setup_step = self.step(
            steps, "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020 # v4.4.0", key="uses",
        )
        self.same(fields(block(setup_step, 8, "with"), 10).get("node-version"), "${{ matrix.node }}")
        install, install_step = self.step(steps, "Install Rust")
        self.same(script(install_step), INSTALL)
        build, build_step = self.step(steps, "Build engine")
        self.same(script(build_step), BUILD)
        parity, parity_step = self.step(steps, "Schema, transport, CLI parity and tarball contents")
        tests = " ".join(f"packaging/npm/test/{suite}.test.cjs" for suite in ("client", "parity", "pack", "names"))
        self.same(script(parity_step), "\n".join([
            "node packaging/npm/scripts/generate-types.mjs --check",
            f'export DCTX_TEST_BIN="$GITHUB_WORKSPACE/{ENGINE}"',
            'if [[ "$RUNNER_OS" == Windows ]]; then export DCTX_TEST_BIN="$DCTX_TEST_BIN.exe"; fi',
            GUARD,
            f"npm exec -- node --test {tests}",
        ]))
        self.ordered(setup, install, build, parity)

    def test_python_313_builds_and_requires_the_engine(self):
        job, steps = self.job("python-packages.yml", "wrapper")
        rows = matrix_rows(job)
        self.same(sorted(row.get("python", "") for row in rows), ["3.11", "3.13"])
        self.same(fields(job, 4).get("runs-on"), "ubuntu-24.04")
        env = fields(block(job, 4, "env"), 6)
        self.same(env.get("DCTX_REQUIRE_ENGINE_TESTS"), f"${{{{ {PYTHON_FLOOR} && '1' || '' }}}}")
        self.same(env.get("DCTX_TEST_BIN"),
                  f"${{{{ {PYTHON_FLOOR} && format('{{0}}/{ENGINE}', github.workspace) || '' }}}}")
        setup, setup_step = self.step(
            steps, "actions/setup-python@a26af69be951a213d495a4c3e4e4022e16d87065 # v5.6.0", key="uses",
        )
        self.same(fields(block(setup_step, 8, "with"), 10).get("python-version"), "${{ matrix.python }}")
        install, install_step = self.step(steps, "Install Rust toolchain", condition=PYTHON_FLOOR)
        self.same(script(install_step), INSTALL + " --no-self-update")
        build, build_step = self.step(steps, "Build matching engine", condition=PYTHON_FLOOR)
        self.same(script(build_step), BUILD + "\n" + GUARD)
        parity, parity_step = self.step(steps, "Schema and transport checks")
        self.same(script(parity_step), "python -W error::ResourceWarning -m unittest discover -s packages/python/tests -v")
        self.ordered(setup, install, build, parity)

    def bash(self, source, **env):
        try:
            return subprocess.run(["bash", "-e", "-c", source], cwd=ROOT,
                                  env={"LC_ALL": "C", **env}, capture_output=True, timeout=10)
        except (OSError, subprocess.TimeoutExpired) as error:
            self.fail(f"Cannot run offline Bash fixture. {str(error)!a}")

    def escaped_diagnostic(self, output):
        self.same(output.count(b"\n"), 1)
        self.assertNotIn(b"\x1b", output)
        self.assertIn(br'\n\E"', output)

    def test_engine_guard_rejects_missing_and_nonexecutable_paths(self):
        with tempfile.TemporaryDirectory(dir=ROOT) as directory:
            binary = Path(directory) / 'engine\n\x1b"'
            for state in ("missing", "nonexecutable", "executable"):
                with self.subTest(state=state):
                    if state == "nonexecutable":
                        binary.touch()
                        binary.chmod(0o644)
                    if state == "executable":
                        binary.chmod(0o755)
                    result = self.bash(GUARD, DCTX_TEST_BIN=str(binary))
                    self.same(result.returncode, 0 if state == "executable" else 1)
                    if state != "executable":
                        self.escaped_diagnostic(result.stderr)

    def test_go_floor_rejects_upgraded_toolchain_and_escapes_output(self):
        stub = 'go() { printf "%s\\n" "$FAKE_GO_VERSION"; }\n'
        env = {"EXPECTED_GO_VERSION": "1.23.12", "GOTOOLCHAIN": "local"}
        result = self.bash(stub + GO_VERSION_CHECK, **env, FAKE_GO_VERSION="go1.23.12")
        self.same(result.returncode, 0)
        result = self.bash(stub + GO_VERSION_CHECK, EXPECTED_GO_VERSION="1.23.12",
                           GOTOOLCHAIN="auto", FAKE_GO_VERSION="go1.23.12")
        self.same(result.returncode, 1)
        result = self.bash(stub + GO_VERSION_CHECK, **env, FAKE_GO_VERSION='go1.26.10\n\x1b"')
        self.same(result.returncode, 1)
        self.escaped_diagnostic(result.stderr)


if __name__ == "__main__":
    unittest.main()
