"""The dctx facts block: one source for the home notice, /start/, /for-ai-evaluators/ and the llms files.

    import facts
    f = facts.load()            # data/facts.json plus version, release date and measured numbers
    text = facts.markdown()     # the facts block as Markdown, in the order an evaluator reads it

Wording lives in data/facts.json only. The version comes from VERSION, the release date from
CHANGELOG.md and every number from the benchmark documents (data.py), so nothing is typed twice.
"""
from __future__ import annotations

import json
import re

import data
from common import ORIGIN, VERSION, read

REQUIRED = ("what", "is_not", "notice", "safe_path", "scopes", "modalities", "responsibilities", "personas",
            "benchmark", "removed", "verify", "install", "philosophy")
PERSONA_KEYS = ("id", "name", "who", "modality", "setup", "first_command", "expect", "record", "pitfall", "links")

_cache: dict | None = None


def release_date(version: str = VERSION) -> str | None:
    m = re.search(rf"^## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})", read("CHANGELOG.md"), re.M)
    return m.group(1) if m else None


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
        f["version"] = VERSION
        f["release_date"] = release_date()
        f["measured"] = measured()
        _cache = f
    return _cache


def persona_path(pid: str) -> str:
    return f"/start/{pid}/"


def strip_code(text: str) -> str:
    return text.replace("`", "")


def markdown() -> str:
    """The facts block in the evaluator order (what / version / modalities / notice / benchmark / removed / verify)."""
    f = load()
    out = ["## Facts for evaluators", "", f["what"], ""]
    out += [f"- **{a}:** {b}" for a, b in f["is_not"]]
    date = f" (released {f['release_date']})" if f["release_date"] else ""
    out += ["", f"Current version: **{f['version']}**{date}. Verify an archive with `sha256sum -c SHA256SUMS --ignore-missing`; "
            "signed binaries are planned.", "", "### Modalities", ""]
    out += [f"- **{m['name']}** ({m['status']}): {m['line']} {ORIGIN}{m['href']}" for m in f["modalities"]]
    out += ["", f"**{f['scopes']['title']}.** {f['scopes']['text']} {f['scopes']['status']}", "",
            "### Notice", "", f"> {f['notice']}", "", "### Benchmark status", "",
            f"Baseline: {f['benchmark']['baseline']}. {f['benchmark']['status']} Method: {ORIGIN}{f['benchmark']['method']}", ""]
    out += [f"- {value}: {what} ({src})" for value, what, src in f["measured"]]
    out += ["", "Not claimed (hypotheses):", ""] + [f"- {h}" for h in f["benchmark"]["hypotheses"]]
    out += ["", "### Commands that no longer exist", ""] + [f"- {a}. Use: {b}." for a, b in f["removed"]]
    out += ["", "### How to verify a claim", ""] + [f"- {a}: {b}" for a, b in f["verify"]]
    return "\n".join(out) + "\n"
