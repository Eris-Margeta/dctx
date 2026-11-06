#!/usr/bin/env python3
"""Recompute the published stability and M7 results from the complete raw answers."""
import collections
import csv
import hashlib
import json
from pathlib import Path
import statistics
import subprocess

OUT = Path(__file__).resolve().parent
ROOT = OUT.parents[2]


def prefix(a, b):
    for i in range(min(len(a), len(b))):
        if a[i] != b[i]:
            return i
    return min(len(a), len(b))


def main():
    runs = [json.loads(line) for line in (OUT / "runs.jsonl").read_text().splitlines()]
    summary = json.loads((OUT / "summary.json").read_text())
    manifest = json.loads((OUT / "corpora.json").read_text())
    controls = json.loads((OUT / "edit-controls.json").read_text())
    assert len(runs) == summary["commands"] == 405
    assert collections.Counter(r["exit"] for r in runs) == {0: 405}
    grouped = collections.defaultdict(list)
    chronological = collections.defaultdict(list)
    for r in runs:
        data = r["stdout"].encode("utf-8")
        assert len(data) == r["stdout_bytes"]
        assert hashlib.sha256(data).hexdigest() == r["stdout_sha256"]
        assert r["stderr"] == ""
        if r["phase"].startswith("warm-") or r["phase"] == "unrelated":
            grouped[r["corpus"], r["query"], r["format"]].append(data)
            chronological[r["corpus"], r["format"]].append((r, data))
            if r["format"] == "json":
                j = json.loads(data)
                assert "snapshot" not in j and "generation" not in j
                assert "age_s" not in j.get("freshness", {})
                if r["query"] != "callees":
                    assert j["records"], r
                if r["query"] == "path":
                    assert j["found"], r
            else:
                verb = {"callers1": "neighbors", "callers2": "neighbors", "callees": "neighbors", "outline": "map"}.get(r["query"], r["query"])
                assert data.splitlines()[0].decode() == f"# {r['corpus']} {verb}"
    rows = list(csv.DictReader((OUT / "stability.csv").open()))
    assert len(rows) == len(grouped) == summary["cases"] == 66
    for row in rows:
        outputs = grouped[row["corpus"], row["query"], row["format"]]
        assert len(outputs) == 6 and len(set(outputs)) == 1
        assert row["5x identical"] == row["identical after unrelated edit"] == "True"
        assert int(row["common prefix bytes"]) == int(row["output bytes"]) == int(row["after bytes"]) == len(outputs[0])
        assert float(row["M7"]) == 1 and int(row["changed lines"]) == 0
        assert row["baseline_sha256"] == row["after_sha256"] == hashlib.sha256(outputs[0]).hexdigest()
    actual_traces = []
    for (corpus, query, fmt), outputs in grouped.items():
        for i, (a, b) in enumerate(zip(outputs, outputs[1:])):
            actual_traces.append(("same-request", corpus, fmt, f"{query}/{i}", f"{query}/{i + 1}", prefix(a, b), len(a), len(b)))
    for (corpus, fmt), sequence in chronological.items():
        for (ar, a), (br, b) in zip(sequence, sequence[1:]):
            actual_traces.append(("changing-question", corpus, fmt, f"{ar['phase']}/{ar['query']}", f"{br['phase']}/{br['query']}", prefix(a, b), len(a), len(b)))
    published_traces = list(csv.DictReader((OUT / "turn-prefixes.csv").open()))
    assert len(actual_traces) == len(published_traces) == 720
    for row in published_traces:
        values = (row["trace"], row["corpus"], row["format"], row["previous"], row["current"],
                  int(row["prefix_bytes"]), int(row["previous_bytes"]), int(row["current_bytes"]))
        assert values in actual_traces
        actual_traces.remove(values)
        assert float(row["M7"]) == int(row["prefix_bytes"]) / int(row["current_bytes"])
    assert not actual_traces
    for key, stats in summary["traces"].items():
        trace, fmt = key.split("/")
        selected = [r for r in published_traces if r["trace"] == trace and r["format"] == fmt]
        ratios = [float(r["M7"]) for r in selected]
        assert stats["transitions"] == len(selected)
        assert stats["mean"] == statistics.mean(ratios)
        assert stats["median"] == statistics.median(ratios)
        assert stats["minimum"] == min(ratios)
        assert stats["weighted"] == sum(int(r["prefix_bytes"]) for r in selected) / sum(int(r["current_bytes"]) for r in selected)
    assert len(manifest["files"]) == summary["files"] == 66
    for item in manifest["files"]:
        data = subprocess.check_output(["git", "show", f"{manifest['commit']}:{item['source']}"], cwd=ROOT)
        assert len(data) == item["bytes"] and hashlib.sha256(data).hexdigest() == item["sha256"]
    assert len(controls) == 3
    for control in controls:
        assert control["restored"] and control["before_meta"]["snapshot"] != control["after_meta"]["snapshot"]
    historical = ROOT / "docs/research/trail-code-2026-10/data/stability.csv"
    assert historical.read_bytes() == (OUT / "stability-original.csv").read_bytes()
    assert hashlib.sha256(historical.read_bytes()).hexdigest() == summary["historical_sha256"]
    print("PASS: 405 exit-0 invocations; 66 pinned files; 33/33 text + 33/33 JSON stable over five runs and edits; 720 M7 transitions; 3 changed-snapshot controls; historical CSV unchanged")


if __name__ == "__main__":
    main()
