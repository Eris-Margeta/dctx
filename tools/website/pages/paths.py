"""Choose your path (/start/ and one page per persona) and /for-ai-evaluators/, all from data/facts.json."""
from __future__ import annotations

import re

import facts
import highlight
from common import BLOB, REPO, VERSION, e, rec, src
from components import code, pretty, table, term, transcript


def md_inline(text: str) -> str:
    """Escape, then `code` spans; the only markup facts.json uses."""
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text, quote=False))


def notice(heading_id: str | None = None, level: int = 2, title: str | None = None) -> str:
    """The advanced-user notice, verbatim from facts.json. With a title (the home page), the heading is that
    title and the ways figure is left out: the home showcase covers the ways in."""
    f = facts.load()
    head = (f'<h{level} id="{heading_id}" class="notice-h">{e(title or "Advanced tool, several ways to use it")}</h{level}>'
            if heading_id else "")
    figure = ways() if heading_id == "notice-h" and title is None else ""
    return (f'<div class="callout{" callout-soft" if title else ""}" role="note">{head}<p class="callout-text">{md_inline(f["notice"])}</p>'
            f'<p class="callout-sub">Status: {md_inline(f["notice_status"])}</p>{figure}</div>')


# The four ways in, for the figure under the home notice: (name, command, facts.json "ways" entry for the
# line and status). Commands are the documented entry points (docs/MODALITIES.md, the CLI reference).
WAYS = [
    ("CLI pack", "dctx pack .", "CLI"),
    ("Map mode for agents", "dctx map find greet", "Agents, long session"),
    ("MCP server", "dctx mcp --profile lean", "MCP and ACP"),
    ("Library and wrappers", "Rust · Python · Node · Go", "Libraries"),
]


def ways() -> str:
    """One binary, four ways in: a small figure; the pulse runs only with motion on (CSS)."""
    by = {w["name"]: w for w in facts.load()["ways"]}
    items = []
    for name, cmd, key in WAYS:
        w = by[key]
        shown = (f'<code class="way-cmd"><span class="p">$ </span>{highlight.highlight(cmd, "sh")}</code>' if cmd.startswith("dctx")
                 else f'<code class="way-cmd">{e(cmd)}</code>')
        items.append(f'<li class="way"><a href="{w["link"]}"><b>{e(name)}</b></a>{shown}'
                     f'<p>{md_inline(w["line"][:1].upper() + w["line"][1:])}</p><p class="way-st">{e(w["status"])}</p></li>')
    return (f'<figure class="ways"><figcaption class="ways-hub"><b>dctx</b> one binary, local and offline</figcaption>'
            f'<ol class="ways-list">{"".join(items)}</ol></figure>')


PATHS_INTRO = ("dctx fits into your work in a few different ways. Pick the situation that sounds most like yours: "
               "each path says what you get, what to set up, the first command to run and the one mistake to avoid.")

# One small line icon per persona (24x24, stroke only, coloured by CSS); decorative.
ICONS = {
    "giant-folder": '<path d="M3 6.5A1.5 1.5 0 0 1 4.5 5H9l2 2h8.5A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"/>'
                    '<circle cx="12" cy="13" r="2.5"/><path d="m14 15 2.5 2.5"/>',
    "many-projects": '<rect x="3" y="4" width="8" height="7" rx="1.5"/><rect x="13" y="4" width="8" height="7" rx="1.5"/>'
                     '<rect x="3" y="13" width="8" height="7" rx="1.5"/><rect x="13" y="13" width="8" height="7" rx="1.5"/>',
    "library": '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M13.5 5l-3 14"/>',
    "team": '<circle cx="9" cy="8" r="3"/><path d="M3.5 19a5.5 5.5 0 0 1 11 0"/><circle cx="17" cy="9" r="2.3"/><path d="M15.5 14.2A4.5 4.5 0 0 1 21 18.5"/>',
}


def selector(level: int = 3) -> str:
    """Four cards, one per persona: who it is for and what you get up front; setup, the first command and the
    pitfall folded into a details element (all of facts.json, nothing dropped). site.js remembers the choice."""
    cards = []
    for p in facts.load()["personas"]:
        href = facts.persona_path(p["id"])
        steps = "".join(f"<li>{md_inline(x)}</li>" for x in p["setup"])
        icon = (f'<svg class="path-ico" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false">'
                f'{ICONS.get(p["id"], ICONS["many-projects"])}</svg>')
        cards.append(
            f'<li class="path-card" data-path="{p["id"]}"><div class="path-top">{icon}<h{level}><a href="{href}">{e(p["name"])}</a></h{level}></div>'
            f'<dl class="path-dl"><div><dt>Who it&rsquo;s for</dt><dd>{md_inline(p["who"])}</dd></div>'
            f'<div><dt>What you get</dt><dd>{md_inline(p["gets"])}</dd></div>'
            f'<div><dt>What you use</dt><dd class="path-mod">{md_inline(p["modality"])}</dd></div></dl>'
            f'<details class="path-more"><summary><svg class="path-chevron" width="11" height="11" viewBox="0 0 11 11" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true" focusable="false"><path d="m3 1 4 4-4 4"/></svg>Setup, first command and the pitfall</summary>'
            f'<p class="path-h">Setup</p><ol class="path-steps">{steps}</ol>'
            f'<p class="path-h">First command</p><pre class="path-cmd"><code><span class="p">$ </span>{highlight.highlight(p["first_command"], "sh")}</code></pre>'
            f'<p class="path-h path-h-warn">Watch out for</p><p class="path-pit">{md_inline(p["pitfall"])}</p></details>'
            f'<p class="path-go"><a href="{href}" aria-label="The full path: {e(p["name"])}">Open the full path &rarr;</a></p></li>')
    return '<ul class="paths" data-path-select aria-label="Choose your path">' + "".join(cards) + '</ul>'


def responsibilities(caption: bool = True) -> str:
    r = facts.load()["responsibilities"]
    rows = [[f"<b>{e(a)}</b>", md_inline(b), md_inline(c)] for a, b, c in r["rows"]]
    cap = (f'Items marked planned or pending are not in dctx {".".join(VERSION.split(".")[:2])}.x yet.'
           if caption else "")
    return table(r["head"], rows, raw=True, caption=cap, cls="resp")


def safe_path() -> str:
    return '<ol class="safe">' + "".join(f"<li><b>{e(a)}.</b> {md_inline(b)}</li>" for a, b in facts.load()["safe_path"]) + "</ol>"


def scopes() -> str:
    return '<ul class="facts-list">' + "".join(f"<li>{md_inline(x)}</li>" for x in facts.load()["scopes"]) + "</ul>"


def start() -> str:
    return f"""<div class="wrap narrow"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Start</span><h1>Choose your path</h1>
<p class="lede">{PATHS_INTRO}</p></div>
<section class="sec" aria-labelledby="paths-h"><h2 id="paths-h">Four paths</h2>{selector(3)}</section>
{notice("notice-h")}
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
<dl class="kv path-kv"><dt>you get</dt><dd>{md_inline(p["gets"])}</dd><dt>modality</dt><dd>{md_inline(p["modality"])}</dd><dt>version</dt><dd>dctx {VERSION}</dd></dl>
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
<section class="sec" aria-labelledby="notice-h"><h2 id="notice-h">The notice</h2><p>This is the canonical wording, from <a href="/docs/start/">What dctx is</a> (<code>docs/START.md</code>) and the README; the home page and <a href="/start/">Choose your path</a> carry it verbatim:</p>{notice()}</section>
<section class="sec" aria-labelledby="resp-h"><h2 id="resp-h">Responsibilities</h2>{responsibilities()}</section>
<section class="sec" aria-labelledby="bench-h"><h2 id="bench-h">Benchmark status</h2><p>Baseline: <b>{e(b["baseline"])}</b>.</p><ul class="facts-list">{claims}</ul>
<p>Measured values, as the documents state them today:</p>{meas}</section>
<section class="sec" data-facts="removed" aria-labelledby="gone-h"><h2 id="gone-h">Removed in {e(r["since"])}</h2><p>{md_inline(r["intro"])}</p>{removed}</section>
<section class="sec" aria-labelledby="verify-h"><h2 id="verify-h">How to verify a claim</h2>{verify}</section>
</div>"""
    return body, []
