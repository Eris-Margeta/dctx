"""The site's view of data/facts.json, the one facts source shared with the llms files (llms.facts()).

    import facts
    f = facts.load()   # data/facts.json plus "measured": numbers parsed from the benchmark documents

llms.py renders the evaluator facts block (llms.facts_markdown()); the pages here render the same file:
the home notice and selector, /start/ and the persona pages, /for-ai-evaluators/. Numbers are never
typed into facts.json; they come from data.py at build time.
"""
from __future__ import annotations

import json

import data
from common import VERSION, read

REQUIRED = ("version", "released", "is", "is_not", "verify_download", "modalities", "scopes", "notice", "benchmarks",
            "removed", "verify", "notice_status", "safe_path", "ways", "responsibilities", "personas", "install", "philosophy")
PERSONA_KEYS = ("id", "name", "who", "modality", "setup", "first_command", "expect", "record", "pitfall", "links")

_cache: dict | None = None


def measured() -> list[tuple[str, str, str]]:
    """(value, what it measures, source document) for the numbers the documents state today."""
    r7, stab, langs = data.run7(), data.stability(), data.languages()
    net, _ = data.network_zero()
    lo, hi, n = r7["cold"]["dctx"]
    return [
        (f"{stab['text']} text, {stab['json']} JSON", "map answers byte-identical after unrelated edits and over five runs, three Linux corpora", data.STAB),
        (str(net), "socket attempts by dctx in every traced benchmark variant", data.BENCH),
        (f"{lo:.2f}-{hi:.2f} s", f"cold index of {n} benchmark corpora, median of 3 runs", data.RUN7),
        (str(langs["total"]), f"language families: {langs['tiers']['full']} full, {langs['tiers']['outline']} outline, {langs['tiers']['docs']} docs", "dctx languages"),
    ]


def load() -> dict:
    global _cache
    if _cache is None:
        f = json.loads(read("tools/website/data/facts.json"))
        missing = [k for k in REQUIRED if k not in f]
        if missing:
            raise SystemExit(f"facts.json: missing {', '.join(missing)}")
        for p in f["personas"]:
            gaps = [k for k in PERSONA_KEYS if not p.get(k)]
            if gaps:
                raise SystemExit(f"facts.json: persona {p.get('id')!r} lacks {', '.join(gaps)}")
        if len(f["personas"]) != 4:
            raise SystemExit("facts.json: the selector shows exactly four personas")
        if f["version"] != VERSION:
            raise SystemExit(f"facts.json is for dctx {f['version']}, VERSION is {VERSION}")
        f["measured"] = measured()
        _cache = f
    return _cache


def persona_path(pid: str) -> str:
    return f"/start/{pid}/"
