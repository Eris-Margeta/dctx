#!/usr/bin/env python3
"""Assert the v1 quality and performance floors against committed benchmark JSON (stdlib only, no network).

usage: bench_floors.py [AGGREGATE_JSON [TASKS_JSON]]
  AGGREGATE_JSON  3-way bench aggregate (default: docs/research/bench-3way-20261030/results/run7-53f8b6b/aggregate.json)
  TASKS_JSON      task protocol aggregate (default: docs/research/bench-3way-20261030/tasks/results/run5-53f8b6b/tasks_aggregate.json)

One line per floor: `PASS|FAIL <floor>: value <v> floor <op> <limit>`, or a diagnostic for invalid evidence.
The last line is `BENCH-FLOORS PASS` or
`BENCH-FLOORS FAIL (<n> failed)`; the exit status is 0 only when every floor passes, 2 when a file cannot be read.

Warm query p50 is the median of every warm wall time (all query kinds x 5 runs, process start included) on one corpus.
The slowest single query kind is printed as an INFO line; it is not a floor.
"""
import json
import math
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

    def fail(self, name, reason):
        self.failed += 1
        print(f"FAIL {name}: {reason}")

    def check(self, name, value, op, limit):
        """Print one floor line; a missing value (None) is a FAIL."""
        if type(limit) is bool:
            if type(value) is not bool:
                self.fail(name, "expected a boolean")
                return
        elif type(value) not in (int, float):
            self.fail(name, "expected a number")
            return
        compare = {"<=": lambda a, b: a <= b, ">=": lambda a, b: a >= b, "==": lambda a, b: a == b}[op]
        ok = compare(value, limit)
        if not ok:
            self.failed += 1
        print(f"{'PASS' if ok else 'FAIL'} {name}: value {value} floor {op} {limit}")


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
    except OSError as error:
        detail = error.strerror
    except json.JSONDecodeError as error:
        detail = f"invalid JSON at line {error.lineno}, column {error.colno}"
    except ValueError:
        detail = "expected UTF-8 JSON"
    print(f"BENCH-FLOORS ERROR: cannot read {path!a}: {detail}")
    sys.exit(2)


def is_timing(value):
    """A timing sample must be a finite, nonnegative JSON number."""
    if type(value) is int:
        return value >= 0
    return type(value) is float and math.isfinite(value) and value >= 0


def warm_walls(supported_queries):
    """All warm wall times of the supported query kinds of one corpus, and the slowest kind by its median."""
    walls, slowest = [], None
    for kind, row in sorted(supported_queries.items()):
        samples = row.get("walls")
        if not isinstance(samples, list) or not samples:
            raise ValueError(f"{kind!a}.walls must be a nonempty list of timing samples")
        for index, sample in enumerate(samples):
            if not is_timing(sample):
                raise ValueError(f"{kind!a}.walls[{index}] must be a finite nonnegative number")
        walls.extend(samples)
        median = row.get("wall_s_median")
        if "wall_s_median" in row and not is_timing(median):
            raise ValueError(f"{kind!a}.wall_s_median must be a finite nonnegative number")
        if median is not None and (slowest is None or median > slowest[1]):
            slowest = (kind, median)
    return walls, slowest


def check_network(floors, network):
    if not isinstance(network, list):
        floors.fail("network attempts", "network must be a list of traced steps")
        return
    traced = [(index, row) for index, row in enumerate(network) if dig(row, "tool") == "dctx"]
    name = f"network attempts ({len(traced)} traced dctx steps)"
    if not traced:
        floors.fail(name, "no traced dctx steps found")
        return
    attempts = 0
    for index, row in traced:
        count = row.get("inet_attempt_count")
        if type(count) is not int or count < 0:
            floors.fail(name, f"network[{index}].inet_attempt_count must be a nonnegative integer")
            return
        attempts += count
    floors.check(name, attempts, "==", 0)


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
    print(f"bench floors: {os.path.relpath(agg_path, ROOT)!a} + {os.path.relpath(tasks_path, ROOT)!a}")
    floors = Floors()
    for corpus in CORPORA:
        row = dig(agg, "index", corpus, "dctx")
        floors.check(f"cold index ok {corpus}", dig(row, "ok"), "==", True)
        name = f"cold index wall {corpus} (s)"
        wall = dig(row, "cold_wall_s_median")
        if is_timing(wall):
            floors.check(name, wall, "<=", COLD_WALL_S)
        else:
            floors.fail(name, "cold_wall_s_median must be a finite nonnegative number")
        floors.check(f"cold index RSS {corpus} (MB)", dig(row, "peak_rss_mb_largest_process"), "<=", COLD_RSS_MB)
    for corpus in CORPORA:
        query = dig(agg, "query", corpus, "dctx")
        query = query if isinstance(query, dict) else {}
        supported = {kind: row for kind, row in query.items() if dig(row, "supported")}
        name = f"warm query p50 {corpus}"
        try:
            walls, slowest = warm_walls(supported)
        except ValueError as error:
            floors.fail(name, error)
        else:
            try:
                p50 = round(statistics.median(walls), 5) if walls else None
            except OverflowError:
                p50 = math.inf
            floors.check(f"{name} (s, {len(walls)} runs)", p50, "<=", WARM_P50_S)
            if slowest:
                print(f"INFO slowest query kind {corpus}: {slowest[0]!a} median {slowest[1]} s")
        unstable = sum(not row.get("identical_across_runs") for row in supported.values())
        floors.check(f"warm answers byte-identical across runs {corpus} (unstable kinds)", unstable if query else None, "==", 0)
    gold = dig(agg, "accuracy", "ALL_GOLD_CORPORA", "dctx") or {}
    floors.check("callers precision page 1", gold.get("precision_micro"), ">=", CALLERS_P)
    floors.check("callers precision 5 pages", gold.get("precision_micro_allpages"), ">=", CALLERS_P)
    floors.check("callers recall page 1", gold.get("recall_page1_micro"), ">=", RECALL_PAGE1)
    floors.check("callers recall 5 pages", gold.get("recall_allpages_micro"), ">=", RECALL_5PAGES)
    for corpus in CORPORA:
        row = dig(agg, "determinism", corpus, "dctx")
        name = f"unrelated edit changes only hubs {corpus} (other kinds changed)"
        changed = dig(row, "changed_after_edit")
        if not isinstance(changed, list) or any(not isinstance(kind, str) for kind in changed):
            floors.fail(name, "changed_after_edit must be a list of query kind strings")
        else:
            floors.check(name, sum(kind != HUBS_LABEL for kind in changed), "==", 0)
    check_network(floors, dig(agg, "network"))
    overall = dig(tasks, "overall", "dctx") or {}
    floors.check("October protocol max score", overall.get("max_score"), "==", PROTOCOL_MAX)
    floors.check("October protocol score", overall.get("score"), ">=", PROTOCOL_MIN)
    print("BENCH-FLOORS PASS" if floors.failed == 0 else f"BENCH-FLOORS FAIL ({floors.failed} failed)")
    return 0 if floors.failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
