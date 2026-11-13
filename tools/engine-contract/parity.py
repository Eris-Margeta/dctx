#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exact engine-output parity corpus; new transports implement output(context, case)."""
import argparse
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPUS = Path(__file__).with_name("goldens")


@dataclass
class Context:
    binary: Path
    repo: Path
    env: dict

    def run(self, args, messages=None):
        wire = None if messages is None else b"".join(
            json.dumps(message, ensure_ascii=False).encode() + b"\n" for message in messages
        )
        result = subprocess.run(
            [str(self.binary), *args], cwd=self.repo, env=self.env,
            input=wire, capture_output=True, timeout=30, close_fds=True,
        )
        if result.returncode or result.stderr:
            raise AssertionError(f"{args}: exit {result.returncode}: {result.stderr.decode(errors='replace')}")
        return result.stdout


def isolated_env(root):
    # Do not inherit user configuration, cache, plugins or formatting overrides.
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "WINDIR") if key in os.environ}
    for key in ("HOME", "USERPROFILE", "XDG_CONFIG_HOME", "APPDATA", "XDG_CACHE_HOME",
                "XDG_DATA_HOME", "LOCALAPPDATA", "TMPDIR", "TMP", "TEMP", "DCTX_CACHE_DIR", "DCTX_MAP_CACHE"):
        directory = root / key.lower()
        directory.mkdir(parents=True, exist_ok=True)
        env[key] = str(directory)
    env.update(DCTX_AUTO_CLEAN="0", NO_COLOR="1", LC_ALL="C.UTF-8")
    return env


def cli(context, case):
    return context.run(case["cli"])


def rpc(context, case):
    replies = context.run(["rpc"], [
        {"id": "hello", "verb": "hello"},
        {"id": case["name"], **case["rpc"]},
    ]).splitlines()
    if len(replies) != 2:
        raise AssertionError(f"RPC: expected two replies, got {len(replies)}")
    hello, reply = map(json.loads, replies)
    if hello["result"]["protocol_version"].split(".")[0] != "1" or reply["id"] != case["name"]:
        raise AssertionError(f"RPC envelope mismatch: {hello}, {reply}")
    return reply["result"]["output"].encode("utf-8")


def load_adapter(spec):
    name, filename = spec.split("=", 1)
    module_spec = importlib.util.spec_from_file_location(f"parity_adapter_{name}", filename)
    if module_spec is None or module_spec.loader is None:
        raise ValueError(f"cannot load adapter {spec}")
    module = importlib.util.module_from_spec(module_spec)
    sys.modules[module_spec.name] = module
    module_spec.loader.exec_module(module)
    return name, module.output


def verify(context, cases, surfaces):
    comparisons = 0
    for case in cases:
        expected = (CORPUS / case["golden"]).read_bytes()
        for name, output in surfaces.items():
            actual = output(context, case)
            if not isinstance(actual, bytes) or actual != expected:
                raise AssertionError(f"{case['name']}/{name}: exact byte parity failed\nexpected {expected!r}\nactual   {actual!r}")
            comparisons += 1
    return comparisons


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--adapter", action="append", default=[], metavar="NAME=FILE.py")
    args = parser.parse_args()
    surfaces = {"cli": cli, "rpc": rpc}
    for spec in args.adapter:
        name, adapter = load_adapter(spec)
        if name in surfaces:
            parser.error(f"duplicate adapter {name}")
        surfaces[name] = adapter
    cases = json.loads((CORPUS / "cases.json").read_text())
    if not isinstance(cases, list) or not cases:
        raise ValueError("golden corpus must contain at least one case")
    with tempfile.TemporaryDirectory(prefix="dctx-parity-", dir=ROOT / "target") as tmp:
        context = Context(args.binary.resolve(), ROOT / "crates/dctx/tests/fixtures/repository", isolated_env(Path(tmp)))
        count = verify(context, cases, surfaces)
    print(f"Golden parity PASS: {len(cases)} cases, {len(surfaces)} surfaces, {count} byte comparisons")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, KeyError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"Golden parity FAIL: {error}", file=sys.stderr)
        sys.exit(1)
