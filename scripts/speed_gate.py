#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Gate dctx map/pack scaling at 2,000 and 8,000 seeded small source files.

Five cold-cache runs per size are compared by median process elapsed time.
A ratio above 6x, different stdout bytes across repeated runs, or missing
fixture evidence fails the gate (exit 1). Setup/command errors exit 2.
Automatic discovery skips with a NOTE when it finds no executable.
Fixtures, subprocess state and caches are temporary and stay in this worktree.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import json
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


def cargo_target_dir(worktree: Path) -> Path:
    configured = os.environ.get("CARGO_TARGET_DIR") or os.environ.get("CARGO_BUILD_TARGET_DIR")
    if configured:
        return Path(configured)
    if (worktree / "Cargo.toml").is_file():
        # Let Cargo resolve layered config files and their relative paths.
        try:
            result = subprocess.run(
                ["cargo", "metadata", "--offline", "--locked", "--no-deps", "--format-version", "1"],
                cwd=worktree, capture_output=True, check=True,
            )
        except (FileNotFoundError, subprocess.CalledProcessError):
            pass  # A prebuilt executable can still be used without Cargo metadata.
        else:
            return Path(json.loads(result.stdout)["target_directory"])
    return worktree / "target"


def find_binary(override: Path | None, worktree: Path = WORKTREE, *,
                target_dir: Path | None = None) -> Path | None:
    if override is not None:
        candidates = [override]
    else:
        if target_dir is None:
            target_dir = cargo_target_dir(worktree)
        candidates = [
            target_dir / profile / name
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
    fixture_evidence: bool


def has_fixture_output(args: list[str], output: bytes) -> bool:
    """Look for the first seeded file, which fits even the default pack budget."""
    if args == ["pack", "."]:
        return re.search(
            rb'^<FILE_CONTENT_START ID="[^"\r\n]+" PATH="group_00/item_00000\.rs" LINES="\d+">\n'
            rb'pub fn item_00000\(\) -> u32 \{ \d+ \}$', output, re.MULTILINE
        ) is not None
    if args != ["map", "export"]:
        raise ValueError("unsupported speed gate workload")
    try:
        graph = json.loads(output)
    except (ValueError, RecursionError):
        return False
    if not (isinstance(graph, dict) and isinstance(graph.get("nodes"), list)
            and isinstance(graph.get("edges"), list)):
        return False
    path = "group_00/item_00000.rs"
    missing = {f"m:{path}", f"s:{path}#item_00000"}
    for node in graph["nodes"]:
        if isinstance(node, dict) and node.get("path") == path:
            node_id = node.get("id")
            if isinstance(node_id, str):
                missing.discard(node_id)
                if not missing:
                    return True
    return False


def measure(binary: Path, args: list[str], fixture: Path, state: Path,
            timer: Callable[[], float] = time.perf_counter) -> Measurement:
    samples = []
    baseline = None
    deterministic = True
    fixture_evidence = True
    for run in range(RUNS):
        # A fresh cache for every process measures indexing, not cache reuse.
        env = isolated_env(state / str(run))
        start = timer()
        output = invoke(binary, args, fixture, env)
        elapsed = timer() - start
        if not math.isfinite(elapsed) or elapsed < 0:
            raise ValueError("timer must return finite, nondecreasing readings")
        samples.append(elapsed)
        if baseline is None or output != baseline:
            fixture_evidence = fixture_evidence and has_fixture_output(args, output)
            if baseline is None:
                baseline = output
            else:
                deterministic = False
    return Measurement(statistics.median(samples), len(baseline), deterministic, fixture_evidence)


def time_ratio(small: float, large: float) -> float:
    return large / small if small else (math.inf if large else 1.0)


class SpeedGateParser(argparse.ArgumentParser):
    def error(self, message: str) -> None:
        super().error(json.dumps(message))


def main(argv: list[str] | None = None, *,
         timer: Callable[[], float] = time.perf_counter) -> int:
    parser = SpeedGateParser(prog="speed_gate.py", description=__doc__)
    parser.add_argument("--binary", type=Path, help="path to a built dctx executable")
    parser.add_argument("--target-dir", type=Path,
                        help="Cargo target directory for automatic discovery "
                             "(defaults to CARGO_TARGET_DIR, CARGO_BUILD_TARGET_DIR, "
                             "Cargo configuration, or the worktree's target directory)")
    args = parser.parse_args(argv)
    try:
        binary = find_binary(args.binary, WORKTREE, target_dir=args.target_dir)
        if binary is None:
            if args.binary is not None:
                raise ValueError(f"explicit binary is missing or not executable: {args.binary}")
            print("NOTE: no built dctx binary found; speed gate skipped (use --binary PATH).")
            return 0
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
                  "Ratio | Output bytes (N / 4N) | Fixture evidence | Deterministic | Result |")
            print("| --- | ---: | ---: | ---: | ---: | --- | --- | --- |")
            failed = False
            for name, command in commands.items():
                small, large = [measure(binary, command, fixture,
                                        root / "state" / name.replace(" ", "-") / str(i),
                                        timer)
                                for i, fixture in enumerate(fixtures)]
                ratio = time_ratio(small.median, large.median)
                deterministic = small.deterministic and large.deterministic
                fixture_evidence = small.fixture_evidence and large.fixture_evidence
                passed = ratio <= MAX_RATIO and deterministic and fixture_evidence
                failed = failed or not passed
                print(f"| {name} | {small.median:.6f} | {large.median:.6f} | {ratio:.3f}x | "
                      f"{small.output_bytes} / {large.output_bytes} | "
                      f"{'yes' if fixture_evidence else 'NO'} | "
                      f"{'yes' if deterministic else 'NO'} | {'PASS' if passed else 'FAIL'} |")
            return int(failed)
    except (OSError, subprocess.CalledProcessError, ValueError) as error:
        print(f"ERROR: speed gate could not run: {json.dumps(str(error))}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
