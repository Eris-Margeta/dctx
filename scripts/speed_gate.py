#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate dctx map/pack scaling at 2,000 and 8,000 seeded small source files.

Five cold-cache runs per size are compared by median process elapsed time.
Only a ratio above 6x or different stdout bytes across repeated runs fails the
gate (exit 1). Setup/command errors exit 2; a missing binary skips with a NOTE.
Fixtures, subprocess state and caches are temporary and stay in this worktree.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import math
import os
from pathlib import Path
import random
import re
import statistics
import subprocess
import sys
import tempfile
import time
from typing import Callable

WORKTREE = Path(__file__).resolve().parents[1]
BASE_FILES = 2000
DIRECTORIES = 40
RUNS = 5
MAX_RATIO = 6.0
SEED = 1113


def generate_fixture(root: Path, files: int = BASE_FILES) -> None:
    """Generate reproducible Rust sources, distributed across exactly 40 dirs."""
    if files < DIRECTORIES:
        raise ValueError("fixture needs at least 40 files")
    root.mkdir(parents=True)
    rng = random.Random(SEED)
    directories = [root / f"group_{index:02d}" for index in range(DIRECTORIES)]
    for directory in directories:
        directory.mkdir()
    for index in range(files):
        directory = directories[index % DIRECTORIES]
        source = directory / f"item_{index:05d}.rs"
        source.write_bytes(
            f"pub fn item_{index:05d}() -> u32 {{ {rng.randrange(1_000_000)} }}\n".encode()
        )
        # Stable, settled metadata avoids freshness windows without sleeping.
        os.utime(source, (1_600_000_000, 1_600_000_000))


def find_binary(override: Path | None, worktree: Path = WORKTREE) -> Path | None:
    candidates = [override] if override is not None else [
        worktree / "target" / profile / name
        for profile in ("release", "debug")
        for name in ("dctx", "dctx-cli", "dctx.exe", "dctx-cli.exe")
    ]
    return next((path.resolve() for path in candidates
                 if path.is_file() and os.access(path, os.X_OK)), None)


def isolated_env(state: Path) -> dict[str, str]:
    # Ignore inherited dctx tuning so local configuration cannot change the gate.
    env = {key: value for key, value in os.environ.items()
           if not key.startswith("DCTX_")}
    for variable, directory in (
        ("DCTX_CACHE_DIR", "cache"), ("XDG_CACHE_HOME", "cache"),
        ("XDG_CONFIG_HOME", "config"), ("APPDATA", "config"),
        ("XDG_DATA_HOME", "data"), ("LOCALAPPDATA", "local"),
        ("TMPDIR", "tmp"), ("TMP", "tmp"), ("TEMP", "tmp"),
    ):
        path = state / directory
        path.mkdir(parents=True, exist_ok=True)
        env[variable] = str(path)
    env.update(DCTX_AUTO_CLEAN="0", NO_COLOR="1", LC_ALL="C")
    return env


def invoke(binary: Path, args: list[str], cwd: Path, env: dict[str, str]) -> bytes:
    result = subprocess.run([str(binary), *args], cwd=cwd, env=env,
                            capture_output=True, check=True)
    return result.stdout


def discover_commands(binary: Path, state: Path) -> dict[str, list[str]]:
    """Check the actual executable's help before selecting documented verbs."""
    env = isolated_env(state)
    help_text = invoke(binary, ["--help"], state, env).decode(errors="replace")
    if not all(re.search(r"\bdctx\s+" + verb + r"\b", help_text)
               for verb in ("map", "pack")):
        raise ValueError("binary --help must advertise dctx map and dctx pack")
    map_help = invoke(binary, ["map", "--help"], state, env).decode(errors="replace")
    if not re.search(r"\bexport\b", map_help):
        raise ValueError("binary map --help must advertise export")
    # Export covers the whole graph without a bounded answer's output cap.
    return {"map export": ["map", "export"], "pack": ["pack", "."]}


@dataclass
class Measurement:
    median: float
    output_bytes: int
    deterministic: bool


def measure(binary: Path, args: list[str], fixture: Path, state: Path,
            timer: Callable[[], float] = time.perf_counter) -> Measurement:
    samples = []
    baseline = None
    deterministic = True
    for run in range(RUNS):
        # A fresh cache for every process measures indexing, not cache reuse.
        env = isolated_env(state / str(run))
        start = timer()
        output = invoke(binary, args, fixture, env)
        elapsed = timer() - start
        if not math.isfinite(elapsed) or elapsed < 0:
            raise ValueError("timer must return finite, nondecreasing readings")
        samples.append(elapsed)
        if baseline is None:
            baseline = output
        else:
            deterministic = deterministic and output == baseline
    return Measurement(statistics.median(samples), len(baseline), deterministic)


def time_ratio(small: float, large: float) -> float:
    return large / small if small else (math.inf if large else 1.0)


def main(argv: list[str] | None = None, *,
         timer: Callable[[], float] = time.perf_counter) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="path to a built dctx executable")
    args = parser.parse_args(argv)
    binary = find_binary(args.binary)
    if binary is None:
        print("NOTE: no built dctx binary found; speed gate skipped (use --binary PATH).")
        return 0
    try:
        with tempfile.TemporaryDirectory(prefix="speed-gate-", dir=WORKTREE) as tmp:
            root = Path(tmp)
            commands = discover_commands(binary, root / "help")
            fixtures = []
            for files in (BASE_FILES, 4 * BASE_FILES):
                fixture = root / f"repo-{files}"
                generate_fixture(fixture, files)
                fixtures.append(fixture)
            print(f"Median of {RUNS} cold-cache runs; 4x files; ratio limit {MAX_RATIO:g}x.")
            print(f"| Command | {BASE_FILES} files (s) | {4 * BASE_FILES} files (s) | "
                  "Ratio | Output bytes (N / 4N) | Deterministic | Result |")
            print("| --- | ---: | ---: | ---: | ---: | --- | --- |")
            failed = False
            for name, command in commands.items():
                small, large = [measure(binary, command, fixture,
                                        root / "state" / name.replace(" ", "-") / str(i),
                                        timer)
                                for i, fixture in enumerate(fixtures)]
                ratio = time_ratio(small.median, large.median)
                deterministic = small.deterministic and large.deterministic
                passed = ratio <= MAX_RATIO and deterministic
                failed = failed or not passed
                print(f"| {name} | {small.median:.6f} | {large.median:.6f} | {ratio:.3f}x | "
                      f"{small.output_bytes} / {large.output_bytes} | "
                      f"{'yes' if deterministic else 'NO'} | {'PASS' if passed else 'FAIL'} |")
            return int(failed)
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f"ERROR: speed gate could not run: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
