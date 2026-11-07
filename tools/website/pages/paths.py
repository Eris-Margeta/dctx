"""Choose your path (/start/ and one page per persona) and /for-ai-evaluators/, all from data/facts.json."""
from __future__ import annotations

import re

import facts
from common import BLOB, REPO, VERSION, e, rec, src
from components import code, pretty, table, term, transcript


def md_inline(text: str) -> str:
    """Escape, then `code` spans; the only markup facts.json uses."""
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text, quote=False))


def notice(heading_id: str | None = None, level: int = 2) -> str:
    """The advanced-user notice, verbatim from facts.json."""
    f = facts.load()
    head = (f'<h{level} id="{heading_id}" class="notice-h">Advanced tool, several ways to use it</h{level}>' if heading_id else "")
    return (f'<div class="callout" role="note">{head}<p class="callout-text">{md_inline(f["notice"])}</p>'
            f'<p class="callout-sub">Status: {md_inline(f["notice_status"])}</p></div>')


def selector(level: int = 3) -> str:
    """Four cards, one per persona; site.js remembers the choice (localStorage, optional)."""
    cards = []
    for p in facts.load()["personas"]:
        href = facts.persona_path(p["id"])
        cards.append(f'<li class="path-card" data-path="{p["id"]}"><h{level}><a href="{href}">{e(p["name"])}</a></h{level}>'
                     f'<p>{md_inline(p["who"])}</p><p class="path-mod">{md_inline(p["modality"])}</p>'
                     f'<p class="path-pit"><b>Pitfall:</b> {md_inline(p["pitfall"].split(". ")[0].rstrip("."))}.</p></li>')
    return ('<ul class="paths" data-path-select aria-label="Choose your path">' + "".join(cards) + '</ul>')


def responsibilities(caption: bool = True) -> str:
    r = facts.load()["responsibilities"]
    rows = [[f"<b>{e(a)}</b>", md_inline(b), md_inline(c)] for a, b, c in r["rows"]]
    cap = 'Items marked planned or publishing soon are not in dctx 1.2.x yet.' if caption else ""
    return table(r["head"], rows, raw=True, caption=cap, cls="resp")


def safe_path() -> str:
    return '<ol class="safe">' + "".join(f"<li><b>{e(a)}.</b> {md_inline(b)}</li>" for a, b in facts.load()["safe_path"]) + "</ol>"


def scopes() -> str:
    return '<ul class="facts-list">' + "".join(f"<li>{md_inline(x)}</li>" for x in facts.load()["scopes"]) + "</ul>"


def start() -> str:
    return f"""<div class="wrap narrow"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Start</span><h1>Choose your path</h1>
<p class="lede">dctx is one binary used in very different ways. Each way has its own setup, its own first command and its own way to go wrong. Pick the one closest to your work.</p></div>
{notice("notice-h")}
<section class="sec" aria-labelledby="paths-h"><h2 id="paths-h">Four paths</h2>{selector(3)}</section>
<section class="sec" aria-labelledby="safe-h"><h2 id="safe-h">The safe path, if you are not sure</h2>{safe_path()}
<p>Then pick the path above that matches your work. <a href="/docs/modalities/">Choosing how to use dctx</a> compares every mode with grep plus file reads.</p></section>
<section class="sec" id="two-scopes" aria-labelledby="scopes-h"><h2 id="scopes-h">Two scopes, one binary</h2>{scopes()}</section>
<section class="sec" id="responsibilities" aria-labelledby="resp-h"><h2 id="resp-h">Who is responsible for what</h2>
<p>Each level of use has its own responsibilities. dctx provides the left column; the right column is yours.</p>{responsibilities()}</section>
</div>"""


def persona(pid: str) -> str:
    f = facts.load()
    p = next(x for x in f["personas"] if x["id"] == pid)
    r = rec(p["record"])
    stdout = pretty(r["stdout"]) if p["record"].endswith("-json") else None
    run = term(transcript(r, stdout=stdout), "~/kiosk  ·  dctx " + VERSION)
    others = "".join(f'<li><a href="{facts.persona_path(x["id"])}">{e(x["name"])}</a></li>' for x in f["personas"] if x["id"] != pid)
    links = " &middot; ".join(f'<a href="{h}">{e(t)}</a>' for t, h in p["links"])
    steps = "".join(f"<li>{md_inline(s)}</li>" for s in p["setup"])
    return f"""<div class="wrap narrow" data-path-page="{pid}"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Choose your path</span><h1>{e(p["name"])}</h1>
<p class="lede">{md_inline(p["who"])}</p></div>
<dl class="kv path-kv"><dt>modality</dt><dd>{md_inline(p["modality"])}</dd><dt>version</dt><dd>dctx {VERSION}</dd></dl>
<section class="sec" aria-labelledby="setup-h"><h2 id="setup-h">Setup</h2><ol class="safe">{steps}</ol></section>
<section class="sec" aria-labelledby="first-h"><h2 id="first-h">First command</h2>{code("shell", p["first_command"], "shell")}</section>
<section class="sec" aria-labelledby="expect-h"><h2 id="expect-h">What to expect</h2><p>{md_inline(p["expect"])}</p>
<p class="note">A real run on the <a href="/examples/">kiosk sample project</a>, recorded from the release binary by <code>capture.py</code>:</p>{run}</section>
<section class="sec" aria-labelledby="pit-h"><h2 id="pit-h">The pitfall</h2><div class="callout callout-warn"><p class="callout-text">{md_inline(p["pitfall"])}</p></div></section>
<section class="sec" aria-labelledby="next-h"><h2 id="next-h">Read next</h2><p>{links} &middot; <a href="/start/#responsibilities">Who is responsible for what</a></p>
<p>Other paths:</p><ul class="inline-list">{others}</ul>{notice()}</section>
</div>"""


def evaluators() -> tuple[str, list]:
    f = facts.load()
    v, b, r = f["verify_download"], f["benchmarks"], f["removed"]
    isnot = "".join(f"<li>{md_inline(x)}</li>" for x in f["is_not"])
    mods = "".join(f'<li><a href="{m["link"]}"><b>{e(m["name"])}</b></a> <span class="state">{e(m["status"])}</span>: {md_inline(m["line"])}</li>'
                   for m in f["modalities"])
    meas = table(["Value", "What it measures", "Source"],
                 [[f"<b>{e(val)}</b>", e(w), src(s) if "/" in s else e(s)] for val, w, s in f["measured"]], raw=True,
                 caption="Parsed from the documents at build time; nothing is retyped.")
    claims = "".join(f'<li><span class="state">{e(c["status"])}</span> {md_inline(c["text"])} <a href="{c["link"]}">Source</a></li>'
                     for c in b["claims"])
    removed = table(["No longer exists", "Use instead"], [[md_inline(x["was"]), md_inline(x["use"])] for x in r["items"]], raw=True)
    verify = table(["Claim", "How to check it"], [
        [f'<a href="{x["link"]}">{e(x["what"])}</a>', f'<code>{e(x["command"])}</code>' if "command" in x else md_inline(x["text"])]
        for x in f["verify"]], raw=True)
    body = f"""<div class="wrap narrow"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">For AI evaluators</span><h1>Facts and how to verify them</h1>
<p class="lede">For people and models deciding what dctx is. Every line below comes from one file, <a href="{BLOB}tools/website/data/facts.json"><code>tools/website/data/facts.json</code></a>, plus the benchmark documents; <a href="/llms-small.txt">llms-small.txt</a>, <a href="/llms.txt">llms.txt</a> and <a href="/llms-full.txt">llms-full.txt</a> start with the same facts.</p></div>
<section class="sec" aria-labelledby="what-h"><h2 id="what-h">What dctx is, and is not</h2><p>{md_inline(f["is"])}</p><ul class="facts-list">{isnot}</ul></section>
<section class="sec" aria-labelledby="ver-h"><h2 id="ver-h">Current version</h2><p><b>dctx {e(f["version"])}</b>, released <time datetime="{f["released"]}">{f["released"]}</time> (tag <code>{e(f["release_tag"])}</code>), Apache-2.0. <a href="{v["link"]}">Release on GitHub</a> &middot; <a href="/changelog/">Changelog</a></p>
<p>{md_inline(v["available"])} Verify a download with <code>{e(v["command"])}</code>. {md_inline(v["planned"])}</p></section>
<section class="sec" aria-labelledby="mod-h"><h2 id="mod-h">Modalities</h2><ul class="facts-list">{mods}</ul>
<h3>Two scopes, one binary</h3>{scopes()}</section>
<section class="sec" aria-labelledby="notice-h"><h2 id="notice-h">The notice</h2><p>This is the canonical wording; the home page and <a href="/start/">Choose your path</a> carry it verbatim:</p>{notice()}</section>
<section class="sec" aria-labelledby="resp-h"><h2 id="resp-h">Responsibilities</h2>{responsibilities()}</section>
<section class="sec" aria-labelledby="bench-h"><h2 id="bench-h">Benchmark status</h2><p>Baseline: <b>{e(b["baseline"])}</b>.</p><ul class="facts-list">{claims}</ul>
<p>Measured values, as the documents state them today:</p>{meas}</section>
<section class="sec" data-facts="removed" aria-labelledby="gone-h"><h2 id="gone-h">Removed in {e(r["since"])}</h2><p>{md_inline(r["intro"])}</p>{removed}</section>
<section class="sec" aria-labelledby="verify-h"><h2 id="verify-h">How to verify a claim</h2>{verify}</section>
</div>"""
    return body, []
