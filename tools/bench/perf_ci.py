#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Offline, bounded CLI regression measurements; see PERF-CI.md."""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
BASELINE = Path(__file__).with_name("perf-baseline.json")
CASES = ("cold_build", "warm_read", "warm_find", "pack", "catalog_build")
SAMPLES = 5
INSTRUCTION_SAMPLES = 3
POLICY = {"instructions_ratio": 1.25, "wall_ratio": 3.0, "wall_slack_seconds": 0.15}


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n",
                    encoding="utf-8")


def generate(root: Path) -> str:
    """Fixed content, paths and aged stamps; never trust a newly written stamp."""
    digest = hashlib.sha256()
    for index in range(256):
        name = f"src{index // 16:02}/module{index:04}.rs"
        body = "".join(f"pub fn work_{index}_{n}(x: u64) -> u64 {{ x.wrapping_add({n}) }}\n"
                       for n in range(32))
        if index == 0:
            body = "pub fn perf_target(x: u64) -> u64 { x.wrapping_add(7) }\n" + body
            body += "// deterministic padding for a bounded read of a large file\n" * 7400
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        raw = body.encode()
        path.write_bytes(raw)
        os.utime(path, (1_600_000_000, 1_600_000_000))
        digest.update(name.encode() + b"\0" + raw)
    return digest.hexdigest()


def invoke(command: list[str], env: dict, cwd: Path) -> tuple[bytes, bytes, float]:
    start = time.perf_counter()
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                            timeout=180, check=False, close_fds=True)
    elapsed = time.perf_counter() - start
    if result.returncode:
        raise ValueError(f"{Path(command[0]).name} exited {result.returncode}: "
                         + result.stderr.decode(errors="replace")[-4000:])
    return result.stdout, result.stderr, elapsed


def instruction_count(path: Path) -> int:
    events = None
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("events:"):
            events = line.split()[1:]
        if line.startswith("summary:") and events == ["Ir"]:
            count = int(line.split()[1])
            if count > 0:
                return count
    raise ValueError("missing positive Cachegrind Ir summary (cache simulation must be disabled)")


def validate_output(case: str, stdout: bytes, stderr: bytes) -> None:
    if case in ("cold_build", "warm_find", "warm_read"):
        page = json.loads(stdout)
        if not page.get("records") or b"perf_target" not in stdout:
            raise ValueError(f"{case}: missing sentinel result")
        if case == "warm_read" and b"wrapping_add(7)" not in stdout:
            raise ValueError("warm_read: missing target function body")
        if case == "warm_find" and b"generation reused" not in stderr:
            raise ValueError(f"{case}: expected a reused generation")
        if case == "warm_read":
            reads = [int(line.split()[1]) for line in stderr.splitlines()
                     if line.startswith(b"content_reads ")]
            if len(reads) != 1 or not 0 <= reads[0] <= 1:
                raise ValueError("warm_read: expected at most one indexed source read")
    elif case == "pack":
        if b"perf_target" not in stdout or any(
                f"pub fn work_{index}_31".encode() not in stdout for index in range(256)):
            raise ValueError("pack: incomplete synthetic directory")
    else:
        values = dict(line.split(b"=", 1) for line in stdout.splitlines() if b"=" in line)
        if values.get(b"walk_files") != b"256" or values.get(b"walk_errors") != b"0":
            raise ValueError("catalog_build: incomplete synthetic directory")


def measure(dctx: Path, catalog: Path, scratch: Path, valgrind: str | None) -> dict:
    # All generated trees, caches and instrument outputs stay in this worktree.
    with tempfile.TemporaryDirectory(prefix="run-", dir=scratch) as temporary:
        base = Path(temporary)
        tree = base / "tree"
        tree.mkdir()
        corpus = generate(tree)
        env = {"LANG": "C", "LC_ALL": "C", "TZ": "UTC", "NO_COLOR": "1",
               "DCTX_AUTO_CLEAN": "0", "DCTX_REUSE_REPORT": "1",
               "RAYON_NUM_THREADS": "1", "PATH": str(base / "empty-path")}
        # Keep the loader setting needed by locally extracted Valgrind distributions.
        if "VALGRIND_LIB" in os.environ:
            env["VALGRIND_LIB"] = os.environ["VALGRIND_LIB"]
        for key, folder in (("HOME", "home"), ("XDG_CONFIG_HOME", "config"),
                            ("XDG_CACHE_HOME", "xdg-cache"), ("TMPDIR", "tmp"),
                            ("DCTX_CACHE_DIR", "cache"), ("DCTX_MAP_CACHE", "map-cache"),
                            ("DCTX_PLUGIN_ROOT", "plugins")):
            path = base / folder
            path.mkdir()
            env[key] = str(path)
        find = [str(dctx), "map", "find", "perf_target", "--json"]
        commands = {
            "cold_build": find,
            "warm_read": [str(dctx), "map", "read", "s:src00/module0000.rs#perf_target", "--json"],
            "warm_find": find,
            "pack": [str(dctx), "pack", ".", "--ext", "rs"],
            "catalog_build": [str(catalog), "build", str(tree), str(base / "catalog-output/catalog.cat"), "1"],
        }
        (base / "catalog-output").mkdir()
        rows = {}
        for case, command in commands.items():
            print(f"Measuring {case}: {SAMPLES} native / "
                  f"{INSTRUCTION_SAMPLES if valgrind else 0} instruction samples", flush=True)
            rows[case] = {"wall_seconds": [], "instructions": []}
            if case.startswith("warm_") or case == "pack":
                stdout, stderr, _ = invoke(command, env, tree)
                validate_output("pack" if case == "pack" else "cold_build", stdout, stderr)
            for instrumented, count in ((False, SAMPLES), (True, INSTRUCTION_SAMPLES if valgrind else 0)):
                for _ in range(count):
                    if case == "cold_build":
                        for key in ("DCTX_CACHE_DIR", "DCTX_MAP_CACHE"):
                            shutil.rmtree(env[key])
                            Path(env[key]).mkdir()
                    (base / "catalog-output/catalog.cat").unlink(missing_ok=True)
                    args = command
                    counter = base / "cachegrind.out"
                    if instrumented:
                        counter.unlink(missing_ok=True)
                        args = [valgrind, "--tool=cachegrind", "--cache-sim=no", "--branch-sim=no",
                                f"--cachegrind-out-file={counter}", *command]
                    stdout, stderr, elapsed = invoke(args, env, tree)
                    validate_output(case, stdout, stderr)
                    metric = "instructions" if instrumented else "wall_seconds"
                    rows[case][metric].append(instruction_count(counter) if instrumented else elapsed)
            rows[case]["wall_seconds_median"] = statistics.median(rows[case]["wall_seconds"])
            if valgrind:
                rows[case]["instructions_median"] = statistics.median(rows[case]["instructions"])
        return {"schema_version": 1, "corpus_sha256": corpus, "cases": rows,
                "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                                    text=True).strip(),
                "policy": POLICY, "platform": f"{platform.system()}-{platform.machine()}",
                "libc": list(platform.libc_ver()),
                "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                "rustflags": os.environ.get("RUSTFLAGS", ""),
                "valgrind": subprocess.check_output([valgrind, "--version"], text=True).strip() if valgrind else None,
                "binary_sha256": {"dctx": hashlib.sha256(dctx.read_bytes()).hexdigest(),
                                  "catalog": hashlib.sha256(catalog.read_bytes()).hexdigest()}}


def compare(baseline: dict, report: dict, metric: str) -> list[str]:
    for key in ("schema_version", "corpus_sha256", "runner_sha256", "platform", "rustc", "rustflags", "policy"):
        if baseline.get(key) != report.get(key):
            raise ValueError(f"baseline {key} differs; intentional refresh required")
    if baseline["policy"] != POLICY:
        raise ValueError("baseline policy differs from runner")
    if set(baseline["cases"]) != set(CASES) or set(report["cases"]) != set(CASES):
        raise ValueError("baseline/report must contain every benchmark case exactly once")
    failures = []
    for case in CASES:
        # Native wall time is always checked; instruction mode adds the stable gate.
        for field in (["wall_seconds_median", "instructions_median"] if metric == "instructions"
                      else ["wall_seconds_median"]):
            old, new = baseline["cases"][case][field], report["cases"][case][field]
            if any(isinstance(v, bool) or not isinstance(v, (float, int)) or not math.isfinite(v) or v <= 0
                   for v in (old, new)):
                raise ValueError(f"{case}: invalid {field}")
            limit = (old * POLICY["instructions_ratio"] if field == "instructions_median"
                     else max(old * POLICY["wall_ratio"], old + POLICY["wall_slack_seconds"]))
            ok = new <= limit
            print(f"{'PASS' if ok else 'FAIL'} {case} {field}: {new:.6f} <= {limit:.6f}")
            if not ok:
                failures.append(f"{case}/{field}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "refresh"))
    parser.add_argument("--dctx", type=Path, default=ROOT / "target/release/dctx-cli")
    parser.add_argument("--catalog", type=Path, default=ROOT / "tools/catalog-bench/target/release/catalog-bench")
    parser.add_argument("--baseline", type=Path, default=BASELINE)
    parser.add_argument("--report", type=Path, default=ROOT / "target/perfci/report.json")
    parser.add_argument("--scratch", type=Path, default=ROOT / "target/perfci/scratch")
    parser.add_argument("--metric", choices=("instructions", "wall"), default="instructions")
    args = parser.parse_args()
    try:
        for path in (args.baseline, args.report, args.scratch):
            if not path.resolve().is_relative_to(ROOT):
                raise ValueError("output and scratch paths must stay inside the worktree")
        if args.baseline.resolve() == args.report.resolve():
            raise ValueError("report must not overwrite the baseline")
        # A failed measurement must not leave a previous run's report to upload.
        args.report.unlink(missing_ok=True)
        valgrind = shutil.which("valgrind") if args.metric == "instructions" else None
        if args.metric == "instructions" and not valgrind:
            raise ValueError("Valgrind is required; wall-only fallback must be selected explicitly")
        if args.command == "refresh" and not valgrind:
            raise ValueError("refresh requires instruction measurements")
        args.scratch.mkdir(parents=True, exist_ok=True)
        report = measure(args.dctx.resolve(strict=True), args.catalog.resolve(strict=True),
                         args.scratch.resolve(), valgrind)
        write_json(args.report, report)
        if args.command == "refresh":
            write_json(args.baseline, report)
            print("Baseline refreshed; review the JSON diff before committing.")
            return 0
        failures = compare(json.loads(args.baseline.read_text(encoding="utf-8")), report, args.metric)
        report["failures"] = failures
        write_json(args.report, report)
        return int(bool(failures))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"perf-ci: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
