#!/usr/bin/env python3
"""Offline STAB-01 protocol; outputs go beside this script, corpora/cache to /tmp."""
import collections
import csv
import hashlib
import json
import os
from pathlib import Path
import shlex
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
PIN = "0495f5f87717df5385209d306b92328a8cd96cf6"
BIN = ROOT / ".target-stab/debug/dctx-cli"
CORPORA = [
    dict(name="parser-mixed", source="crates/dctx-map-parse/tests/fixtures/",
         target="rust/src/support.rs", symbol="helper", edit="ts/module.ts"),
    dict(name="lexer-go", source="crates/dctx-lex-go/", target="tests/fixtures/basic.go",
         symbol="Add", edit="src/lib.rs"),
    dict(name="map-query", source="crates/dctx-map-query/src/", target="adapters/output.rs",
         symbol="render_answer_at", edit="domain/ranking.rs"),
]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write_json(name, value):
    (OUT / name).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def write_csv(name, rows):
    with (OUT / name).open("w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def prefix(a, b):
    return next((i for i, (x, y) in enumerate(zip(a, b)) if x != y), min(len(a), len(b)))


def main():
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip() == PIN
    assert BIN.is_file(), "Build the pinned CLI first"
    runs, manifest, rows, traces, controls = [], [], [], [], []
    # No benchmark host, network, external corpora, user config or shared cache required.
    with tempfile.TemporaryDirectory(prefix="dctx-stab-01-") as scratch:
        scratch = Path(scratch)
        env = {k: v for k, v in os.environ.items() if not k.startswith("DCTX_")}
        env.update(XDG_CONFIG_HOME=str(scratch / "config"), XDG_CACHE_HOME=str(scratch / "xdg-cache"))

        def run(c, label, phase, args, fmt="json"):
            argv = [str(BIN), "map", *args]
            local_env = dict(env, DCTX_CACHE_DIR=str(scratch / "cache" / c["name"]))
            result = subprocess.run(argv, cwd=scratch / c["name"], env=local_env,
                                    capture_output=True, timeout=120)
            record = dict(corpus=c["name"], query=label, phase=phase, format=fmt,
                          argv=argv, command=shlex.join(argv), cwd=str(scratch / c["name"]),
                          env={k: local_env[k] for k in ("DCTX_CACHE_DIR", "XDG_CONFIG_HOME", "XDG_CACHE_HOME")},
                          exit=result.returncode, stdout_bytes=len(result.stdout),
                          stdout_sha256=sha(result.stdout), stdout=result.stdout.decode("utf-8"),
                          stderr=result.stderr.decode("utf-8"))
            runs.append(record)
            with (OUT / "runs.jsonl").open("a") as f:
                f.write(json.dumps(record, sort_keys=True) + "\n")
            assert result.returncode == 0, record
            assert result.stdout, record
            if fmt == "json":
                json.loads(result.stdout)
            return result.stdout

        for c in CORPORA:
            dest = scratch / c["name"]
            paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", PIN, "--", c["source"]], cwd=ROOT).decode().splitlines()
            # All tracked files beneath the named prefix, including configuration and tests.
            for source in paths:
                relative = source.removeprefix(c["source"])
                data = subprocess.check_output(["git", "show", f"{PIN}:{source}"], cwd=ROOT)
                path = dest / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                manifest.append(dict(corpus=c["name"], source=source, path=relative,
                                     bytes=len(data), sha256=sha(data)))
            resolved = json.loads(run(c, "resolve", "setup", ["find", c["symbol"], "--limit", "40", "--json"]))
            matches = [r for r in resolved["records"] if r.get("path") == c["target"]
                       and r["id"].split("#")[-1] == c["symbol"]]
            assert len(matches) == 1, resolved
            did = matches[0]["id"]
            c["id"] = did
            queries = [
                ("find", ["find", c["symbol"], "--limit", "10"]),
                ("callers1", ["neighbors", did, "--dir", "in", "--edge", "calls"]),
                ("callers2", ["neighbors", did, "--dir", "in", "--edge", "calls", "--depth", "2"]),
                ("callees", ["neighbors", did, "--dir", "out", "--edge", "calls"]),
                ("outline", ["map", c["target"], "--depth", "2", "--limit", "40"]),
                ("read", ["read", did]),
                ("explain", ["explain", did]),
                ("skeleton", ["skeleton", c["target"]]),
                ("grep", ["grep", c["symbol"], "--in", c["target"]]),
                ("hubs", ["hubs"]),
                ("path", ["path", "m:" + c["target"], did]),
            ]
            answers = collections.defaultdict(list)
            chronological = collections.defaultdict(list)
            before_meta = json.loads(run(c, "snapshot-control", "before", ["find", c["symbol"], "--json", "--meta"]))
            for repetition in range(5):
                for label, args in queries:
                    for fmt in ("text", "json"):
                        data = run(c, label, f"warm-{repetition}", args + (["--json"] if fmt == "json" else []), fmt)
                        answers[label, fmt].append(data)
                        chronological[fmt].append((f"warm-{repetition}/{label}", data))
                if repetition < 4:
                    time.sleep(1.05)  # Cross wall-clock seconds as well as separate processes.
            edited = dest / c["edit"]
            original = edited.read_bytes()
            mutated = original + b"\n// STAB-01 unrelated comment\n"
            edited.write_bytes(mutated)
            try:
                for label, args in queries:
                    for fmt in ("text", "json"):
                        data = run(c, label, "unrelated", args + (["--json"] if fmt == "json" else []), fmt)
                        answers[label, fmt].append(data)
                        chronological[fmt].append((f"unrelated/{label}", data))
                after_meta = json.loads(run(c, "snapshot-control", "after", ["find", c["symbol"], "--json", "--meta"]))
            finally:
                edited.write_bytes(original)
            assert edited.read_bytes() == original
            # Prove that the edit reached the snapshot rather than an ignored/stale file.
            assert before_meta["snapshot"] != after_meta["snapshot"], (before_meta, after_meta)
            controls.append(dict(corpus=c["name"], edit=c["edit"], before_sha256=sha(original),
                                 after_sha256=sha(mutated), before_meta=before_meta, after_meta=after_meta,
                                 restored=True))
            for (label, fmt), outputs in answers.items():
                base, after = outputs[0], outputs[-1]
                lcp = prefix(base, after)
                bl, al = base.split(b"\n"), after.split(b"\n")
                rows.append(dict(corpus=c["name"], query=label, tool="dctx", format=fmt,
                                 **{"5x identical": len(set(outputs[:5])) == 1,
                                    "identical after unrelated edit": base == after,
                                    "common prefix bytes": lcp, "output bytes": len(base),
                                    "after bytes": len(after), "M7": lcp / len(after),
                                    "changed lines": sum(x != y for x, y in zip(bl, al)) + abs(len(bl) - len(al))},
                                 warm_exits="0;0;0;0;0", after_exit=0,
                                 baseline_sha256=sha(base), after_sha256=sha(after)))
                for i, (a, b) in enumerate(zip(outputs, outputs[1:])):
                    traces.append(dict(trace="same-request", corpus=c["name"], format=fmt,
                                       previous=f"{label}/{i}", current=f"{label}/{i + 1}",
                                       prefix_bytes=prefix(a, b), previous_bytes=len(a), current_bytes=len(b),
                                       M7=prefix(a, b) / len(b)))
            for fmt, sequence in chronological.items():
                for (an, a), (bn, b) in zip(sequence, sequence[1:]):
                    traces.append(dict(trace="changing-question", corpus=c["name"], format=fmt,
                                       previous=an, current=bn, prefix_bytes=prefix(a, b),
                                       previous_bytes=len(a), current_bytes=len(b), M7=prefix(a, b) / len(b)))
            print(f"{c['name']}: {len(queries) * 2} answer cases, all commands exited 0", flush=True)

    write_json("corpora.json", dict(commit=PIN, binary_sha256=sha(BIN.read_bytes()), corpora=CORPORA, files=manifest))
    write_json("edit-controls.json", controls)
    write_csv("stability.csv", rows)
    write_csv("turn-prefixes.csv", traces)
    historical = ROOT / "docs/research/trail-code-2026-10/data/stability.csv"
    shutil.copyfile(historical, OUT / "stability-original.csv")
    summary = dict(commit=PIN, cases=len(rows), commands=len(runs),
                   exits=dict(collections.Counter(r["exit"] for r in runs)),
                   files=len(manifest), historical_sha256=sha(historical.read_bytes()), formats={}, traces={})
    for fmt in ("text", "json"):
        selected = [r for r in rows if r["format"] == fmt]
        summary["formats"][fmt] = dict(cases=len(selected),
            identical_after_edit=sum(r["identical after unrelated edit"] for r in selected),
            identical_five_runs=sum(r["5x identical"] for r in selected),
            M7_min=min(r["M7"] for r in selected), M7_mean=statistics.mean(r["M7"] for r in selected))
    for trace in ("same-request", "changing-question"):
        for fmt in ("text", "json"):
            selected = [r for r in traces if r["trace"] == trace and r["format"] == fmt]
            summary["traces"][f"{trace}/{fmt}"] = dict(transitions=len(selected),
                minimum=min(r["M7"] for r in selected), median=statistics.median(r["M7"] for r in selected),
                mean=statistics.mean(r["M7"] for r in selected),
                weighted=sum(r["prefix_bytes"] for r in selected) / sum(r["current_bytes"] for r in selected))
    write_json("summary.json", summary)
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    # Fail rather than append to evidence from an earlier run.
    assert not (OUT / "runs.jsonl").exists(), "Use a fresh evidence directory for a new run"
    main()
