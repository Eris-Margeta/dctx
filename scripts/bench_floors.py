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

    def check(self, name, value, op, limit, *, display_digits=None):
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
        shown = value if display_digits is None else round(value, display_digits)
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
    except OSError as error:
        detail = ascii(error.strerror)
    except json.JSONDecodeError as error:
        detail = f"invalid JSON at line {error.lineno}, column {error.colno}"
    except ValueError:
        detail = "expected UTF-8 JSON"
    print(f"BENCH-FLOORS ERROR: cannot read {path!a}: {detail}")
    sys.exit(2)


def is_nonnegative_number(value):
    """Accept finite, nonnegative JSON numbers, including arbitrary-size integers."""
    if type(value) is int:
        return value >= 0
    return type(value) is float and math.isfinite(value) and value >= 0


def supported_queries(query):
    if not isinstance(query, dict):
        raise ValueError("query metrics must be an object")
    supported = {}
    for kind, row in query.items():
        if not isinstance(row, dict):
            raise ValueError(f"{kind!a} query metrics must be an object")
        if type(row.get("supported")) is not bool:
            raise ValueError(f"{kind!a}.supported must be a boolean")
        if row["supported"]:
            supported[kind] = row
    return supported


def warm_walls(supported_queries):
    """All warm wall times of the supported query kinds of one corpus, and the slowest kind by its median."""
    walls, slowest = [], None
    for kind, row in sorted(supported_queries.items()):
        samples = row.get("walls")
        if not isinstance(samples, list) or not samples:
            raise ValueError(f"{kind!a}.walls must be a nonempty list of timing samples")
        for index, sample in enumerate(samples):
            if not is_nonnegative_number(sample):
                raise ValueError(f"{kind!a}.walls[{index}] must be a finite nonnegative number")
        walls.extend(samples)
        median = row.get("wall_s_median")
        if "wall_s_median" in row and not is_nonnegative_number(median):
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


def check_protocol(floors, overall):
    maximum = dig(overall, "max_score")
    if type(maximum) is not int:
        floors.fail("October protocol max score", "max_score must be an integer")
        return
    floors.check("October protocol max score", maximum, "==", PROTOCOL_MAX)
    if maximum != PROTOCOL_MAX:
        return
    score = dig(overall, "score")
    if type(score) is not int or not 0 <= score <= maximum:
        floors.fail("October protocol score", f"score must be an integer between 0 and {maximum}")
    else:
        floors.check("October protocol score", score, ">=", PROTOCOL_MIN)


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
        for field, name, limit in (
            ("cold_wall_s_median", f"cold index wall {corpus} (s)", COLD_WALL_S),
            ("peak_rss_mb_largest_process", f"cold index RSS {corpus} (MB)", COLD_RSS_MB),
        ):
            value = dig(row, field)
            if is_nonnegative_number(value):
                floors.check(name, value, "<=", limit)
            else:
                floors.fail(name, f"{field} must be a finite nonnegative number")
    for corpus in CORPORA:
        query = dig(agg, "query", corpus, "dctx")
        name = f"warm query p50 {corpus}"
        try:
            supported = supported_queries(query)
        except ValueError as error:
            floors.fail(name, error)
            continue
        try:
            walls, slowest = warm_walls(supported)
        except ValueError as error:
            floors.fail(name, error)
        else:
            try:
                p50 = statistics.median(walls) if walls else None
            except OverflowError:
                p50 = math.inf
            floors.check(f"{name} (s, {len(walls)} runs)", p50, "<=", WARM_P50_S, display_digits=5)
            if slowest:
                print(f"INFO slowest query kind {corpus}: {slowest[0]!a} median {slowest[1]} s")
        unstable = sum(not row.get("identical_across_runs") for row in supported.values())
        floors.check(f"warm answers byte-identical across runs {corpus} (unstable kinds)", unstable if query else None, "==", 0)
    gold = dig(agg, "accuracy", "ALL_GOLD_CORPORA", "dctx")
    for field, name, limit in (
        ("precision_micro", "callers precision page 1", CALLERS_P),
        ("precision_micro_allpages", "callers precision 5 pages", CALLERS_P),
        ("recall_page1_micro", "callers recall page 1", RECALL_PAGE1),
        ("recall_allpages_micro", "callers recall 5 pages", RECALL_5PAGES),
    ):
        value = dig(gold, field)
        if is_nonnegative_number(value) and value <= 1:
            floors.check(name, value, ">=", limit)
        else:
            floors.fail(name, f"{field} must be a finite number in [0, 1]")
    for corpus in CORPORA:
        row = dig(agg, "determinism", corpus, "dctx")
        name = f"unrelated edit changes only hubs {corpus} (other kinds changed)"
        changed = dig(row, "changed_after_edit")
        if not isinstance(changed, list) or any(not isinstance(kind, str) for kind in changed):
            floors.fail(name, "changed_after_edit must be a list of query kind strings")
        else:
            floors.check(name, sum(kind != HUBS_LABEL for kind in changed), "==", 0)
    check_network(floors, dig(agg, "network"))
    check_protocol(floors, dig(tasks, "overall", "dctx"))
    print("BENCH-FLOORS PASS" if floors.failed == 0 else f"BENCH-FLOORS FAIL ({floors.failed} failed)")
    return 0 if floors.failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
