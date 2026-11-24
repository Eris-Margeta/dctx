#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exercise the public clients against two local RPC engine stubs."""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
HOSTILE = 'override\n\x1b"'
RELATIVE = Path(HOSTILE) / "dctx"
CALLER_OUTPUT = "languages: caller λ\n\n".encode()
PROJECT_OUTPUT = "languages: project β\n\n".encode()

STUB = """
import json
import os
import sys

if sys.argv[1:] != ["rpc"] or os.getcwd() != PROJECT:
    sys.exit("expected RPC startup in the project directory")

for line in sys.stdin:
    request = json.loads(line)
    verb = request["verb"]
    if request.get("args", []) != []:
        sys.exit("expected no RPC arguments")
    if verb == "hello":
        result = {"protocol_version": "1.0.0", "verbs": ["hello", "languages"]}
    elif verb == "languages":
        result = {"output": OUTPUT}
    else:
        sys.exit("unexpected RPC verb")
    print(json.dumps({
        "id": request["id"], "engine_version": VERSION, "result": result,
    }), flush=True)
"""

PYTHON_PROBE = """
import sys

source, project, *binary = sys.argv[1:]
sys.path.insert(0, source)
from dctx import Client

with Client(project, **({"binary": binary[0]} if binary else {})) as client:
    sys.stdout.buffer.write(client.languages())
"""

NODE_PROBE = """
const [pkg, cwd, binaryPath] = process.argv.slice(1);
const { connect } = require(pkg);

async function probe() {
  const client = await connect(binaryPath === undefined ? { cwd } : { cwd, binaryPath });
  try {
    const reply = await client.request('languages');
    if (reply.error) throw new Error(reply.error.message);
    process.stdout.write(Buffer.from(reply.result.output, 'utf8'));
  } finally {
    await client.close();
  }
}

probe().catch((error) => {
  console.error(JSON.stringify(String(error)));
  process.exitCode = 1;
});
"""


def run(argv, cwd, env, timeout=20):
    result = subprocess.run(
        argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout,
    )
    if result.returncode or result.stderr:
        # Escape all diagnostic data at the outer reporting boundary.
        raise RuntimeError(
            f"exit {result.returncode} from {argv[0]}; "
            f"stdout {result.stdout.decode('utf-8', errors='backslashreplace')}; "
            f"stderr {result.stderr.decode('utf-8', errors='backslashreplace')}"
        )
    return result.stdout


def diagnostic(error):
    return json.dumps(str(error), ensure_ascii=True)


def check_diagnostic(caller, env):
    command = [
        sys.executable, "-I", "-B", "-c",
        "import sys; sys.stdout.write(sys.argv[1]); sys.stderr.write(sys.argv[1]); sys.exit(7)",
        HOSTILE,
    ]
    try:
        run(command, caller, env)
    except RuntimeError as error:
        rendered = diagnostic(error)
        if (
            not all(" " <= char <= "~" for char in rendered)
            or rendered.count(r'override\n\u001b\"') != 2
            or f"stdout {HOSTILE}; stderr {HOSTILE}" not in json.loads(rendered)
        ):
            raise AssertionError(f"unsafe subprocess diagnostic: {rendered!r}") from None
    else:
        raise AssertionError("failing subprocess was accepted")


def write_stub(directory, project, version, output):
    stub = directory / RELATIVE
    stub.parent.mkdir(parents=True)
    stub.write_text(
        f"#!/usr/bin/env python3\nPROJECT = {str(project)!r}\n"
        f"VERSION = {version!r}\nOUTPUT = {output.decode('utf-8')!r}\n{STUB}",
        encoding="utf-8",
    )
    stub.chmod(0o755)
    return stub


def verify(fixture):
    runtimes = {}
    for name in ("go", "node"):
        runtime = shutil.which(name)
        if runtime is None:
            raise RuntimeError(f"required runtime {name} is missing")
        runtimes[name] = Path(runtime).resolve()

    caller = fixture / "caller"
    project = fixture / "project"
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    caller_stub = write_stub(caller, project, version, CALLER_OUTPUT)
    write_stub(project, project, version, PROJECT_OUTPUT)

    # Keep the configured build caches. The checked-in Go module has no external dependencies.
    build_env = {
        key: os.environ[key]
        for key in ("PATH", "HOME", "GOCACHE", "XDG_CACHE_HOME", "GOMODCACHE")
        if key in os.environ
    }
    build_env.update(
        GOENV="off", GOTOOLCHAIN="local", GOWORK="off", GOPROXY="off", GOSUMDB="off",
        CGO_ENABLED="0", GOTMPDIR=str(fixture),
    )
    go_probe = fixture / "go-probe"
    run(
        [runtimes["go"], "build", "-o", go_probe, HERE / "probe.go"],
        ROOT / "packages/go", build_env, timeout=120,
    )

    env = {"PATH": os.environ["PATH"], "HOME": str(fixture),
           "XDG_CACHE_HOME": str(fixture / "client-cache")}
    check_diagnostic(caller, env)
    probes = {
        "Python": [sys.executable, "-I", "-B", "-c", PYTHON_PROBE, ROOT / "packages/python/src"],
        "Go": [go_probe],
        "Node": [runtimes["node"], "-e", NODE_PROBE, ROOT / "packaging/npm/dctx"],
    }
    failures = []
    comparisons = 0
    for name, probe in probes.items():
        relative_output = PROJECT_OUTPUT if name == "Node" else CALLER_OUTPUT
        for mode, override, expected in (
            ("relative option", RELATIVE, relative_output),
            ("relative DCTX_BIN", RELATIVE, relative_output),
            ("absolute option", caller_stub, CALLER_OUTPUT),
            ("absolute DCTX_BIN", caller_stub, CALLER_OUTPUT),
        ):
            label = f"{name} / {mode}"
            explicit = mode.endswith("option")
            case_env = env if explicit else {**env, "DCTX_BIN": str(override)}
            # Go's zero-value Binary string leaves the option unset.
            option = [str(override)] if explicit else ([""] if name == "Go" else [])
            try:
                actual = run([*probe, project, *option], caller, case_env)
                if actual != expected:
                    raise AssertionError(f"expected {expected!r}, got {actual!r}")
                comparisons += 1
            except (AssertionError, OSError, RuntimeError, subprocess.SubprocessError) as error:
                failures.append(f"{label} failed with {error}")
    if failures:
        raise AssertionError("\n".join(failures))
    if comparisons != 12:
        raise AssertionError(f"expected 12 byte comparisons, ran {comparisons}")
    return comparisons


def main():
    try:
        with tempfile.TemporaryDirectory(prefix=".override-resolution-", dir=HERE) as directory:
            comparisons = verify(Path(directory))
    except Exception as error:
        print(f"Override resolution FAIL {diagnostic(error)}", file=sys.stderr)
        return 1
    print(f"Override resolution PASS ({comparisons} byte comparisons, 1 diagnostic check)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
