#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Measure CLI stdout sizes with default bounds.

Uses the existing deterministic 1,551-file latency fixture, not the tester's
private repository. Captures complete stdout without a shell/client truncator.
Stdlib only; run with --binary PATH --output target/D1112-BASHPATH/sizes.json.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile

from map_find_latency import generate_fixture, percentile

FILES = 1551
THRESHOLD_BYTES = 30_000
TERMS = (
    "latency_probe", "helper", "scale_item_000001", "scale_item_000004",
    "scale_item_000005", "scale_item_000100", "scale_item_000401",
    "scale_item_001000", "scale_item_001548", "scale_item_001549",
)
COMMANDS = (
    "map", "find", "neighbors", "explain", "read", "hubs", "grep",
    "skeleton", "path", "trace", "impact", "export", "pack",
)
MODES = ("cli_defaults",)


def targets(place: Path) -> dict[str, tuple[str, str]]:
    """Locate generated files without duplicating the generator's layout logic."""
    files = {path.stem: path.relative_to(place).as_posix()
             for path in place.rglob("*") if path.is_file()}
    result = {}
    for term in TERMS:
        index = 0 if term in ("latency_probe", "helper") else int(term.rsplit("_", 1)[1])
        file = files[f"source_{index:06d}"]
        result[term] = (file, f"s:{file}#{term}")
    return result


def arguments(command: str, mode: str, term: str,
              selected: dict[str, tuple[str, str]]) -> list[str]:
    file, node = selected[term]
    if command == "pack":
        return ["pack", "--for", f"`{term}`"]
    args = ["map", command]
    if command in ("find", "grep"):
        return [*args, "--", term]
    if command in ("map", "skeleton"):
        return [*args, "--", file]
    if command in ("path", "trace"):
        # The fixture has only one call edge. Probe both directions for its
        # endpoints; other independent symbols are paired with latency_probe.
        other = selected["helper" if term == "latency_probe" else "latency_probe"][1]
        return [*args, "--", node, other]
    if command == "hubs":
        return args
    if command == "export":
        return [*args, "--format", "json"]
    return [*args, "--", node]


def summarize(samples: list[dict]) -> list[dict]:
    summary = []
    for command in COMMANDS:
        for mode in MODES:
            rows = [row for row in samples if row["command"] == command and row["mode"] == mode]
            sizes = [row["stdout_bytes"] for row in rows]
            summary.append({
                "command": command, "mode": mode, "calls": len(rows),
                "median_bytes": statistics.median(sizes),
                "p95_bytes": percentile(sizes, 0.95), "max_bytes": max(sizes),
                "over_30000": sum(row["over_30000"] for row in rows),
            })
    return summary


def measure(binary: Path, root: Path) -> dict:
    root = root.resolve()
    place = root / "fixture"
    shape = generate_fixture(place, FILES)
    selected = targets(place)
    # Keep host query overrides and global ignores out of this comparison.
    env = {key: value for key, value in os.environ.items() if not key.startswith("DCTX_")}
    env.update({"DCTX_AUTO_CLEAN": "0", "DCTX_CACHE_DIR": str(root / "cache")})
    for key, directory in {
        "XDG_CONFIG_HOME": "config", "APPDATA": "config", "LOCALAPPDATA": "data",
        "XDG_CACHE_HOME": "xdg-cache", "XDG_DATA_HOME": "data",
        "TMPDIR": "tmp", "TMP": "tmp", "TEMP": "tmp",
    }.items():
        path = root / directory
        path.mkdir(exist_ok=True)
        env[key] = str(path)
    samples = []
    for command in COMMANDS:
        for term in TERMS:
            for mode in MODES:
                args = arguments(command, mode, term, selected)
                completed = subprocess.run([str(binary), *args], cwd=place, env=env,
                                           capture_output=True, check=True)
                payload = completed.stdout
                samples.append({
                    "command": command, "mode": mode, "term": term, "arguments": args,
                    "stdout_bytes": len(payload), "stdout_lines": len(payload.splitlines()),
                    "over_30000": len(payload) > THRESHOLD_BYTES,
                })
    return {
        "schema": 1, "fixture_shape": shape, "terms": list(TERMS),
        "commands": list(COMMANDS), "modes": list(MODES),
        "threshold_bytes": THRESHOLD_BYTES, "calls": len(samples),
        "encoding": "text except export (JSON)",
        "median_method": "arithmetic median", "percentile_method": "nearest-rank",
        "samples": samples, "summary": summarize(samples),
    }


def table(report: dict) -> str:
    lines = ["| Command | Mode | Median bytes | p95 bytes | Max bytes | >30,000 / 10 |",
             "| --- | --- | ---: | ---: | ---: | ---: |"]
    for row in report["summary"]:
        median = format(row["median_bytes"], ",.1f").removesuffix(".0")
        lines.append(f"| {row['command']} | {row['mode']} | {median} "
                     f"| {row['p95_bytes']:,} | {row['max_bytes']:,} | {row['over_30000']} |")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.unlink(missing_ok=True)
    binary = args.binary.resolve(strict=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="shell-size-", dir=args.output.parent.resolve()) as tmp:
        report = measure(binary, Path(tmp))
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(table(report))


if __name__ == "__main__":
    main()
