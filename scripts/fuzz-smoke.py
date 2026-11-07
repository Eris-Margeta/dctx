#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compile bounded property targets, then run them under one wall-clock deadline.

Uses stable Rust/proptest, including on hosts without nightly or cargo-fuzz.
Compilation is outside the execution budget; CI separately caps the whole job.
A timeout is a failure, never a successful partial campaign. Seeds and minimized
failures are printed by proptest and retained in the CI log/artifact.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

TARGETS = [
    ("dctx", ["--features", "rpc", "--lib"], "robustness"),
    ("dctx", ["--features", "rpc", "--test", "robustness"], ""),
    ("dctx-adapter-cache", ["--test", "robustness"], ""),
    ("dctx-cli", ["--bin", "dctx-cli"], "robustness"),
    ("dctx-cli", ["--lib"], "robustness"),
    ("dctx-map-parse", ["--test", "fuzz_all"], ""),
    ("dctx-map-segment", ["--lib"], "corrupt"),
    ("dctx-map-store", ["--lib"], "robustness"),
    ("dctx-template", ["--lib"], "robustness"),
    ("dctx-lex-rust", ["--test", "fuzz"], ""),
    ("dctx-lex-md", ["--test", "fuzz"], ""),
    ("dctx-plugin-protocol", ["--test", "fuzz"], ""),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seconds", type=int, default=180)
    parser.add_argument("--cases", type=int, default=64)
    parser.add_argument("--seed", type=int, default=445846555)
    args = parser.parse_args()
    if min(args.seconds, args.cases) <= 0 or args.seed < 0:
        parser.error("seconds/cases must be positive and seed nonnegative")
    os.chdir(Path(__file__).resolve().parent.parent)
    env = os.environ.copy()
    env.update(PROPTEST_CASES=str(args.cases), PROPTEST_RNG_SEED=str(args.seed))
    commands = [
        ["cargo", "test", "--locked", "-p", package, *options]
        for package, options, _ in TARGETS
    ]
    executables = []
    for command, (package, _, test_filter) in zip(commands, TARGETS):
        print("BUILD", " ".join(command), flush=True)
        build = subprocess.run(
            [*command, "--no-run", "--message-format=json"], env=env,
            stdout=subprocess.PIPE, text=True, close_fds=True,
        )
        artifacts = []
        for line in build.stdout.splitlines():
            message = json.loads(line)
            if message.get("reason") == "compiler-message":
                print(message["message"].get("rendered", ""), file=sys.stderr, end="")
            if (message.get("reason") == "compiler-artifact"
                    and message.get("executable") and message["profile"].get("test")):
                artifacts.append((message["executable"], Path(message["manifest_path"]).parent))
        build.check_returncode()
        if len(artifacts) != 1:
            raise RuntimeError(f"Expected one test executable for {package}, found {len(artifacts)}")
        executable, directory = artifacts[0]
        executables.append((package, executable, directory, test_filter))
    started = time.monotonic()
    print(f"CAMPAIGN seconds={args.seconds} cases={args.cases} seed={args.seed}", flush=True)
    for package, executable, directory, test_filter in executables:
        remaining = args.seconds - (time.monotonic() - started)
        if remaining <= 0:
            raise subprocess.TimeoutExpired(package, args.seconds)
        # Run the built test directly so timeout kills the test, without leaving
        # a test child behind a killed cargo process or spending budget rebuilding.
        listing = subprocess.run(
            [executable, test_filter, "--list"], env=env, cwd=directory,
            capture_output=True, text=True, check=True, timeout=remaining, close_fds=True,
        )
        if not any(line.endswith(": test") for line in listing.stdout.splitlines()):
            raise RuntimeError(f"No tests selected for {package}: {test_filter}")
        remaining = args.seconds - (time.monotonic() - started)
        if remaining <= 0:
            raise subprocess.TimeoutExpired(package, args.seconds)
        print("RUN", package, test_filter, flush=True)
        subprocess.run(
            [executable, test_filter, "--test-threads=1"],
            env=env, cwd=directory, check=True, timeout=remaining, close_fds=True,
        )
    print(f"PASS {len(TARGETS)} target groups in {time.monotonic() - started:.2f}s", flush=True)


if __name__ == "__main__":
    try:
        main()
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, RuntimeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
