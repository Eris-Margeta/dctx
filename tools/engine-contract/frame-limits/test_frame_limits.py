# SPDX-License-Identifier: Apache-2.0
"""Exercise request frame limits through all three public clients."""

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(HERE.parent))

from parity import isolated_env

LIMIT = 1_048_576
VERB = "languages"
HELP = "--help"
HOSTILE = '\n\x1b"\\λ&<>\u2028\u2029' + r"\u2028"
FRAME_LIMIT_MESSAGE = "RPC request exceeds the engine's 1,048,576-byte frame limit"


def cases(hello_id):
    padded = [
        {"name": "lambdas", "fill": "λ", "repeat": 200_000, "suffix": ""},
        {"name": "ampersands", "fill": "&", "repeat": 200_000, "suffix": ""},
    ]
    # Hello and the two padded requests precede the boundary requests. All
    # three clients include args here, and member order does not affect size.
    for excess, name in enumerate(("exact limit", "one byte over")):
        number = int(hello_id) + 1 + len(padded)
        request_id = str(number) if isinstance(hello_id, str) else number
        frame = {"id": request_id, "verb": VERB, "args": [HELP, HOSTILE]}
        size = len(json.dumps(frame, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
        repeat, spare = divmod(LIMIT + excess - size, 2)
        padded.append({"name": name, "fill": "λ", "repeat": repeat,
                       "suffix": HOSTILE + "x" * spare, "refuse": bool(excess)})
    return [*padded, {"name": "reuse"}]


def python_adapter():
    sys.path.insert(0, str(ROOT / "packages/python/src"))
    from dctx import Client

    config = json.load(sys.stdin)
    results = []
    with Client(config["project"], binary=config["binary"]) as client:
        for case in config["cases"]:
            args = [HELP]
            if "fill" in case:
                args.append(case["fill"] * case["repeat"] + case["suffix"])
            try:
                results.append(client.languages(*args).decode("utf-8"))
            except ValueError as error:
                if str(error) != FRAME_LIMIT_MESSAGE:
                    raise
                results.append(None)
    print(json.dumps(results))


def run(name, cmd, *, cwd, env, config=None):
    completed = subprocess.run(
        cmd, cwd=cwd, env=env,
        input=b"" if config is None else json.dumps(config).encode("ascii"),
        capture_output=True, timeout=240,
    )
    if completed.returncode or completed.stderr:
        raise AssertionError(f"{name} exited {completed.returncode} with stderr={completed.stderr[:1000]!r}")
    return completed.stdout


def main():
    binary = Path(os.environ["DCTX_TEST_BIN"]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="dctx-frame-limits-") as tmp:
        work = Path(tmp)
        project = work / "project"
        project.mkdir()
        env = isolated_env(work)
        help_bytes = run("CLI", [str(binary), VERB, HELP], cwd=project, env=env)
        if not help_bytes.startswith(b"Usage: dctx languages ") or not help_bytes.endswith(b"\n"):
            raise AssertionError(f"unexpected CLI help bytes {help_bytes[:240]!r}")
        help_text = help_bytes.decode("utf-8")

        drivers = (
            ("Python", [sys.executable, "-B", str(__file__), "--python-adapter"], ROOT, env, 0),
            ("Go", ["go", "run", str(HERE / "go_client.go")], ROOT / "packages/go", None, "1"),
            ("Node", ["node", str(HERE / "node_client.cjs")], ROOT, None, "0"),
        )
        comparisons = 0
        for name, cmd, cwd, driver_env, hello_id in drivers:
            plan = cases(hello_id)
            config = {"binary": str(binary), "project": str(project), "env": env, "cases": plan}
            results = json.loads(run(name, cmd, cwd=cwd, env=driver_env, config=config))
            if not isinstance(results, list) or len(results) != len(plan):
                raise AssertionError(f"{name} returned an unexpected result list")
            for case, actual in zip(plan, results):
                expected = None if case.get("refuse") else help_text
                if actual != expected:
                    raise AssertionError(f"{name} case {case['name']!r} returned {str(actual)[:240]!a}")
                comparisons += 1
    print(f"Frame limits PASS: {comparisons} cases across Python, Go, and Node")


if __name__ == "__main__":
    try:
        if sys.argv[1:] == ["--python-adapter"]:
            python_adapter()
        else:
            main()
    except Exception as error:
        print(f"Frame limits FAIL: {str(error)!a}", file=sys.stderr)
        sys.exit(1)
