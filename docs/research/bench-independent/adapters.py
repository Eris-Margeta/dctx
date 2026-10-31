#!/usr/bin/env python3
"""Per-tool command mappings, output parsers, the callers judge and the question scorer (v1 card 6).

The mapping was fixed before the repos were selected: for every question type each tool gets its own natural verb
(listed in questions.md). `go(argv, label)` runs one command in the isolated bench environment and returns stdout
bytes; a "find" label marks an id-resolution step, which never counts for caller facts. The scorer is the October
rule (tasks_score.score_question): 2 points at >= 80 % of facts present, 1 if any, 0 otherwise.
"""
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "bench-3way-20261030", "tasks", "scripts"))
import tasks_score  # noqa: E402

GFY_LINE = re.compile(r"^- .* \[(\w+)\] (.+?):L(\d+)\s*$")


def span(s):
    """Trail "L10-L20" span as (10, 20); unknown spans cover everything."""
    m = re.match(r"L(\d+)(?:-L(\d+))?", s or "")
    if not m:
        return (0, 10 ** 9)
    a = int(m.group(1))
    return (a, int(m.group(2) or a))


def judge(hits, gold):
    """hits: (path, first, last) ranges; gold: {file, line} call sites. A hit is right when it covers a gold site."""
    tp = [h for h in hits if any(h[0] == s["file"] and h[1] <= s["line"] <= h[2] for s in gold)]
    found = [s for s in gold if any(h[0] == s["file"] and h[1] <= s["line"] <= h[2] for h in hits)]
    return {"hits": len(hits), "tp_hits": len(tp), "found": len(found), "gold": len(gold)}


def parse_dctx(out):
    """`map neighbors --json` page: (call records as ranges, next cursor or None)."""
    try:
        js = json.loads(out)
    except ValueError:
        return [], None
    hits = []
    for rec in js.get("records", []):
        if rec.get("edge") not in (None, "calls"):
            continue
        ln = rec.get("lines") or [0, 10 ** 9]
        hits.append((rec.get("path"), ln[0], ln[-1]))
    return hits, (js.get("next") if js.get("more") else None)


def parse_trail(out, file, line):
    """`trail callers NAME --json`: call hits of the match whose symbol is the target definition."""
    try:
        matches = json.loads(out).get("matches", [])
    except ValueError:
        return []
    hits = []
    for m in matches:
        s = m.get("symbol", {})
        a, b = span(s.get("span"))
        if s.get("path") == file and a - 3 <= line <= b:
            for h in m.get("hits", []):
                if h.get("relation") == "calls":
                    ha, hb = span(h.get("span"))
                    hits.append((h.get("path"), ha, hb))
    return hits


def parse_gfy(out):
    """`graphify affected` lines "- name [kind] path:L12" as call-site ranges (+-2 lines for multi-line calls)."""
    hits = []
    for line in out.decode("utf-8", "replace").splitlines():
        m = GFY_LINE.match(line)
        if m:
            ln = int(m.group(3))
            hits.append((m.group(2), ln - 2, ln + 2))
    return hits


def score(facts, outs, labels):
    """(present flags, fraction, points) by the October rule."""
    return tasks_score.score_question(facts, outs, labels)


def norm_label(s):
    return re.sub(r"^[.]|\(\)$", "", s or "").strip()


class G:
    """graphify-out/graph.json node lookups, same conventions as the 3-way harness."""

    def __init__(self, d):
        p = os.path.join(d, "graphify-out", "graph.json")
        self.g = json.load(open(p)) if os.path.exists(p) else {"nodes": [], "links": []}

    def ids(self, name, file, line):
        cands = []
        for n in self.g["nodes"]:
            if norm_label(n.get("label")) != name:
                continue
            m = re.match(r"L(\d+)", n.get("source_location") or "")
            ln = int(m.group(1)) if m else 10 ** 6
            if n.get("source_file") == file:
                cands.append((abs(ln - line), n["id"]))
        cands.sort()
        return cands[0][1] if cands and cands[0][0] <= 5 else None

    def file_node(self, file):
        base = os.path.basename(file)
        for n in self.g["nodes"]:
            if n.get("source_file") == file and norm_label(n.get("label")) in (base, file):
                return n["id"]
        return None


def dctx_id(go, D, d, fallback):
    """The dctx id of definition d from `map find --json`: same path, start within 3 lines of the span."""
    out = go([D, "map", "find", d["name"], "--json", "--limit", "40", "--budget", "2000"], "find")
    try:
        recs = json.loads(out).get("records", [])
    except ValueError:
        return None
    for rec in recs:
        ln = rec.get("lines") or [0, 0]
        if rec.get("path") == d["file"] and ln[0] - 3 <= d["line"] <= ln[-1]:
            return rec.get("id")
    return recs[0].get("id") if (fallback and recs) else None


def ask_dctx(go, q, D):
    k = q["kind"]
    if k == "concept":
        return go([D, "map", "find", q["words"], "--limit", "10"], "answer")
    if k == "outline":
        return go([D, "map", "skeleton", q["file"]], "answer")
    d = q["t"]["def"]
    if k == "def":
        return go([D, "map", "find", d["name"], "--limit", "10"], "answer")
    did = dctx_id(go, D, d, fallback=True)
    if not did:
        return None
    nb = [D, "map", "neighbors", did, "--edge", "calls"]
    if k == "callers":
        return go(nb + ["--dir", "in"], "answer")
    if k == "impact":
        return go(nb + ["--dir", "in", "--depth", "2"], "answer")
    return go(nb + ["--dir", "out"], "answer")


def ask_trail(go, q):
    k = q["kind"]
    if k == "concept":
        return go(["trail", "ask", q["words"]], "answer")
    if k == "outline":
        return go(["trail", "skeleton", q["file"]], "answer")
    n = q["t"]["def"]["name"]
    argv = {"def": ["trail", "ask", n, "-n", "3"], "callers": ["trail", "callers", n], "impact": ["trail", "callers", n, "-d", "2"],
            "callees": ["trail", "callers", n, "--direction", "out"]}[k]
    return go(argv, "answer")


def ask_graphify(go, q, g):
    k = q["kind"]
    if k == "concept":
        return go(["graphify", "query", q["words"], "--budget", "2000"], "answer")
    if k == "outline":
        return go(["graphify", "explain", g.file_node(q["file"]) or q["file"], "--all"], "answer")
    d = q["t"]["def"]
    nid = g.ids(d["name"], d["file"], d["line"]) or d["name"]
    if k == "def":
        return go(["graphify", "explain", nid], "answer")
    if k == "callees":
        return go(["graphify", "explain", nid, "--all"], "answer")
    go(["graphify", "explain", nid], "find")
    depth = "2" if k == "impact" else "1"
    return go(["graphify", "affected", nid, "--relation", "calls", "--depth", depth], "answer")


def callers_dctx(go, t, D, pages=5):
    """(first-page hits, hits over up to `pages` pages) or None when the id does not resolve."""
    did = dctx_id(go, D, t["def"], fallback=False)
    if not did:
        return None
    first, every, cursor = None, [], None
    for _ in range(pages):
        argv = [D, "map", "neighbors", did, "--dir", "in", "--edge", "calls", "--limit", "40", "--json", "--budget", "2000"]
        hits, cursor = parse_dctx(go(argv + (["--cursor", cursor] if cursor else []), "callers"))
        first = hits if first is None else first
        every += hits
        if not cursor:
            break
    return first, every


def callers_trail(go, t):
    d = t["def"]
    hits = parse_trail(go(["trail", "callers", d["name"], "--json"], "callers"), d["file"], d["line"])
    return hits, hits


def callers_graphify(go, t, g):
    d = t["def"]
    nid = g.ids(d["name"], d["file"], d["line"])
    if not nid:
        return None
    hits = parse_gfy(go(["graphify", "affected", nid, "--relation", "calls", "--depth", "1"], "callers"))
    return hits, hits
