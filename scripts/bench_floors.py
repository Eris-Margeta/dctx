#!/usr/bin/env python3
"""Assert the v1 quality and performance floors against committed benchmark JSON (stdlib only, no network).

usage: bench_floors.py [AGGREGATE_JSON [TASKS_JSON]]
  AGGREGATE_JSON  3-way bench aggregate (default: docs/research/bench-3way-20261030/results/run7-53f8b6b/aggregate.json)
  TASKS_JSON      task protocol aggregate (default: docs/research/bench-3way-20261030/tasks/results/run5-53f8b6b/tasks_aggregate.json)

One line per floor: `PASS|FAIL <floor>: value <v> floor <op> <limit>`. The last line is `BENCH-FLOORS PASS` or
`BENCH-FLOORS FAIL (<n> failed)`; the exit status is 0 only when every floor passes, 2 when a file cannot be read.

Warm query p50 is the median of every warm wall time (all query kinds x 5 runs, process start included) on one corpus.
The slowest single query kind is printed as an INFO line; it is not a floor.
"""
import json
import os
import statistics
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BENCH = os.path.join(ROOT, "docs", "research", "bench-3way-20261030")
DEFAULT_AGG = os.path.join(BENCH, "results", "run7-53f8b6b", "aggregate.json")
DEFAULT_TASKS = os.path.join(BENCH, "tasks", "results", "run5-53f8b6b", "tasks_aggregate.json")
CORPORA = ["flask", "ripgrep", "pocketbase", "trailcode", "mixed", "docs"]
COLD_WALL_S, COLD_RSS_MB, WARM_P50_S = 2.0, 100.0, 0.300
CALLERS_P, RECALL_PAGE1, RECALL_5PAGES = 0.99, 0.85, 0.95
PROTOCOL_MIN, PROTOCOL_MAX = 138, 140
HUBS_LABEL = "overview"   # the bench names the `dctx map hubs` query "overview"


class Floors:
    """Collects one PASS/FAIL line per floor and counts the failures."""

    def __init__(self):
        self.failed = 0

    def check(self, name, value, op, limit):
        """Print one floor line; a missing value (None) is a FAIL."""
        compare = {"<=": lambda a, b: a <= b, ">=": lambda a, b: a >= b, "==": lambda a, b: a == b}[op]
        try:
            ok = value is not None and compare(value, limit)
        except TypeError:
            ok = False
        if not ok:
            self.failed += 1
        shown = "missing" if value is None else value
        print(f"{'PASS' if ok else 'FAIL'} {name}: value {shown} floor {op} {limit}")


def dig(doc, *keys):
    """Nested lookup that returns None instead of raising on a missing key."""
    for key in keys:
        if not isinstance(doc, dict) or key not in doc:
            return None
        doc = doc[key]
    return doc


def load(path):
    """Read one JSON file or exit 2 naming it."""
    try:
        with open(path, encoding="utf-8") as handle:
            return json.load(handle)
    except (OSError, ValueError) as error:
        print(f"BENCH-FLOORS ERROR: cannot read {path}: {error}")
        sys.exit(2)


def warm_walls(query):
    """All warm wall times of the supported query kinds of one corpus, and the slowest kind by its median."""
    walls, slowest = [], None
    for kind, row in sorted((query or {}).items()):
        if not isinstance(row, dict) or not row.get("supported"):
            continue
        walls.extend(row.get("walls") or [])
        median = row.get("wall_s_median")
        if median is not None and (slowest is None or median > slowest[1]):
            slowest = (kind, median)
    return walls, slowest


def main(argv):
    """Check every floor and return the process exit status."""
    if any(arg in ("-h", "--help") for arg in argv):
        print(__doc__.strip())
        return 0
    if len(argv) > 2:
        print(__doc__.strip())
        return 2
    agg_path = argv[0] if argv else DEFAULT_AGG
    tasks_path = argv[1] if len(argv) > 1 else DEFAULT_TASKS
    agg, tasks = load(agg_path), load(tasks_path)
    print(f"bench floors: {os.path.relpath(agg_path, ROOT)} + {os.path.relpath(tasks_path, ROOT)}")
    floors = Floors()
    for corpus in CORPORA:
        row = dig(agg, "index", corpus, "dctx") or {}
        floors.check(f"cold index ok {corpus}", row.get("ok"), "==", True)
        floors.check(f"cold index wall {corpus} (s)", row.get("cold_wall_s_median"), "<=", COLD_WALL_S)
        floors.check(f"cold index RSS {corpus} (MB)", row.get("peak_rss_mb_largest_process"), "<=", COLD_RSS_MB)
    for corpus in CORPORA:
        query = dig(agg, "query", corpus, "dctx")
        walls, slowest = warm_walls(query)
        p50 = round(statistics.median(walls), 5) if walls else None
        floors.check(f"warm query p50 {corpus} (s, {len(walls)} runs)", p50, "<=", WARM_P50_S)
        if slowest:
            print(f"INFO slowest query kind {corpus}: {slowest[0]} median {slowest[1]} s")
        unstable = sorted(k for k, r in (query or {}).items() if isinstance(r, dict) and r.get("supported") and not r.get("identical_across_runs"))
        floors.check(f"warm answers byte-identical across runs {corpus} (unstable kinds)", len(unstable) if query else None, "==", 0)
    gold = dig(agg, "accuracy", "ALL_GOLD_CORPORA", "dctx") or {}
    floors.check("callers precision page 1", gold.get("precision_micro"), ">=", CALLERS_P)
    floors.check("callers precision 5 pages", gold.get("precision_micro_allpages"), ">=", CALLERS_P)
    floors.check("callers recall page 1", gold.get("recall_page1_micro"), ">=", RECALL_PAGE1)
    floors.check("callers recall 5 pages", gold.get("recall_allpages_micro"), ">=", RECALL_5PAGES)
    for corpus in CORPORA:
        row = dig(agg, "determinism", corpus, "dctx")
        changed = None if row is None else [k for k in row.get("changed_after_edit", []) if k != HUBS_LABEL]
        floors.check(f"unrelated edit changes only hubs {corpus} (other kinds changed)", None if changed is None else len(changed), "==", 0)
    net = [r for r in agg.get("network", []) if isinstance(r, dict) and r.get("tool") == "dctx"]
    attempts = sum(int(r.get("inet_attempt_count") or 0) for r in net) if net else None
    floors.check(f"network attempts ({len(net)} traced dctx steps)", attempts, "==", 0)
    overall = dig(tasks, "overall", "dctx") or {}
    floors.check("October protocol max score", overall.get("max_score"), "==", PROTOCOL_MAX)
    floors.check("October protocol score", overall.get("score"), ">=", PROTOCOL_MIN)
    print("BENCH-FLOORS PASS" if floors.failed == 0 else f"BENCH-FLOORS FAIL ({floors.failed} failed)")
    return 0 if floors.failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
