"""Facts the pages show, parsed from repository files and the capture. Never retyped.

Each function returns values together with the file (and line) they came from, so the page
can link the source next to the number.
"""
from __future__ import annotations

import json
import re

from common import line_of, read, rec

BENCH = "docs/BENCHMARK-20261030.md"
STAB = "docs/research/stability-20261105/REPORT.md"
RUN7 = "docs/research/bench-3way-20261030/results/run7-53f8b6b/aggregate.json"
CORPORA = ["flask", "ripgrep", "pocketbase", "trailcode", "mixed", "docs"]


def md_tables(text: str) -> list[tuple[int, list[str], list[list[str]]]]:
    """Every Markdown table in text: (first line number, header cells, body rows)."""
    lines = text.split("\n")
    out, i = [], 0
    while i < len(lines) - 1:
        if lines[i].lstrip().startswith("|") and re.match(r"^\s*\|[\s:|-]+\|\s*$", lines[i + 1]):
            head = cells(lines[i])
            rows, j = [], i + 2
            while j < len(lines) and lines[j].lstrip().startswith("|"):
                rows.append(cells(lines[j]))
                j += 1
            out.append((i + 1, head, rows))
            i = j
        else:
            i += 1
    return out


def cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def section(text: str, heading: str) -> str:
    """The body of the Markdown section whose heading line starts with `heading` (fenced code skipped)."""
    lines = text.split("\n")
    fence, start, level = False, None, 0
    for i, line in enumerate(lines):
        if line.startswith("```"):
            fence = not fence
            continue
        if fence:
            continue
        m = re.match(r"^(#+) ", line)
        if not m:
            continue
        if start is None and line[len(m.group(1)) + 1:].startswith(heading):
            start, level = i + 1, len(m.group(1))
        elif start is not None and len(m.group(1)) <= level:
            return "\n".join(lines[start:i])
    if start is None:
        raise SystemExit(f"data.py: no section {heading!r}")
    return "\n".join(lines[start:])


def table_after(rel: str, heading: str, marker: str | None = None) -> tuple[int, list[str], list[list[str]]]:
    """The first table in a section (after `marker` if given), with its absolute line number."""
    text = read(rel)
    body = section(text, heading)
    if marker:
        body = body[body.index(marker):]
    offset = text.index(body)
    line0, head, rows = md_tables(body)[0]
    return text[:offset].count("\n") + line0, head, rows


def num(cell: str) -> float:
    m = re.search(r"-?\d[\d,]*(?:\.\d+)?", cell.replace("**", ""))
    if not m:
        raise ValueError(cell)
    return float(m.group(0).replace(",", ""))


def run7() -> dict:
    """Run 7 task table (BENCHMARK-20261030.md) and cold index medians (run 7 aggregate.json)."""
    line, head, rows = table_after(BENCH, "Run 7", "**Task-level protocol**")
    tools = {}
    for row in rows:
        tools[re.sub(r"[`*]", "", row[0]).split()[0].lower().replace("trail-code", "trail")] = row
    agg = json.loads(read(RUN7))["index"]
    cold = {}
    for tool in ("dctx", "trail", "graphify"):
        key = {"trail": "trail-code"}.get(tool, tool)
        walls = [agg[c][k]["cold_wall_s_median"] for c in CORPORA for k in dict.fromkeys((key, tool)) if k in agg.get(c, {})
                 and agg[c][k].get("cold_wall_s_median") is not None and agg[c][k].get("ok", True)]
        cold[tool] = (min(walls), max(walls), len(walls)) if walls else None
    return {"line": line, "head": head, "rows": rows, "points": {t: int(num(r[1])) for t, r in tools.items()},
            "tokens": {t: int(num(r[3])) for t, r in tools.items()}, "cold": cold}


def stability() -> dict:
    text = read(STAB)
    line0, head, rows = md_tables(text)[0]
    first = rows[0]
    return {"line": line0, "head": head, "rows": rows, "text": first[1], "json": first[2]}


def network_zero() -> tuple[int, int]:
    """'dctx and graphify made 0 socket attempts in every variant' (headline table)."""
    text = read(BENCH)
    m = re.search(r"dctx and graphify made (\d+) socket attempts in every variant", text)
    return int(m.group(1)), line_of(BENCH, m.group(0))


def languages() -> dict:
    """The `dctx languages --markdown` capture: rows and tier counts."""
    _, head, rows = md_tables(rec("languages-md")["stdout"])[0]
    tiers: dict[str, int] = {}
    for row in rows:
        tiers[row[3]] = tiers.get(row[3], 0) + 1
    return {"head": head, "rows": rows, "total": len(rows), "tiers": tiers}


def help_sections(text: str) -> list[tuple[str, str]]:
    """Split `dctx --help` at its own headings (unindented lines ending in ':')."""
    parts, title, buf = [], "Usage", []
    for line in text.rstrip("\n").split("\n"):
        if re.match(r"^[A-Z][A-Za-z ]+:$", line):
            parts.append((title, "\n".join(buf).strip("\n")))
            title, buf = line[:-1], []
        else:
            buf.append(line)
    parts.append((title, "\n".join(buf).strip("\n")))
    return [p for p in parts if p[1]]


ENV = ["DCTX_MAX_SOURCE_BYTES", "DCTX_MAX_RECORDS", "DCTX_MAX_ENTRIES", "DCTX_FRESHNESS", "DCTX_CACHE_DIR",
       "DCTX_AUTO_CLEAN", "DCTX_PROFILE"]


def env_vars() -> list[tuple[str, str, str]]:
    """(variable, the help line that documents it, record id), verbatim from the captures."""
    out = []
    for var in ENV:
        for rid in ("map-help", "help"):
            lines = rec(rid)["stdout"].split("\n")
            hit = [i for i, l in enumerate(lines) if var in l]
            if hit:
                i = hit[0]
                text = lines[i].strip()
                # continuation lines are indented deeper and do not start a new option
                j = i + 1
                while j < len(lines) and re.match(r"^\s{20,}\S", lines[j]) and not re.match(r"^\s{2}\S", lines[j]):
                    text += " " + lines[j].strip()
                    j += 1
                out.append((var, text, rid))
                break
        else:
            raise SystemExit(f"data.py: {var} is not in the captured help")
    return out


def readme_tiers() -> str:
    """The prose of README 'Language tiers' before the generated table."""
    body = section(read("README.md"), "Language tiers")
    return body.split("<!-- languages:begin -->")[0].strip()


def readme_block(heading: str, index: int = 0) -> str:
    body = section(read("README.md"), heading)
    blocks = re.findall(r"```(?:zsh|sh|toml)?\n(.*?)```", body, re.S)
    return blocks[index].rstrip("\n")


def independent() -> tuple[str, int, str]:
    """The independent benchmark section of BENCHMARK-20261030.md: (body, heading line, the Tasks sentence)."""
    text = read(BENCH)
    body = section(text, "Independent benchmark")
    line = line_of(BENCH, "## Independent benchmark")
    tasks = re.search(r"Tasks: .*?\(Trail-Code [^)]*\)\.", body).group(0)
    return body.strip(), line, tasks
