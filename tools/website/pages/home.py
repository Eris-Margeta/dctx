"""The home page, in the order of the design review (R3, section 2): claim and hero animation; the
advanced-user notice with the choose-your-path selector; real captured output; what has been measured;
the modalities; responsibilities; philosophy; install per scope. The footer (shell.py) links llms.txt.

Animation hook: pages/animations.py is written on its own card. When it exists, its functions
two_scopes(), calls_times_context(), prefix_stability(), giant_folder() and safe_path() each return an
HTML string that is placed in the matching slot below (see anim()); when it does not, the slots are empty."""
from __future__ import annotations

import datetime
import importlib
import importlib.util
import json
import re

import data
import facts as factsrc
from common import REPO, VERSION, e, line_of, read, rec, src
from components import code, output_examples, term, transcript
from pages.flow import flow
from pages.paths import md_inline, notice, responsibilities, scopes, selector
from routes import has_why

HERO_RECORDS = ["pack-for", "find", "neighbors-in", "read-json"]

# The first commit of the repository. The age shown on the home page is counted from here to the release
# date in facts.json, never to the clock, so the build stays reproducible and the number moves with each release.
FIRST_COMMIT = datetime.date(2026, 1, 8)


def age_at_release(released: str) -> int:
    """Whole months (rounded) from the first commit to the release date."""
    days = (datetime.date.fromisoformat(released) - FIRST_COMMIT).days
    return round(days / 30.4375)


def status(f: dict) -> str:
    """The honest project status as a quiet aside: a timeline (first commit, today, 2.0), then why, where
    it comes from, the promise and the goal. On a phone the longer text is folded into a details element."""
    months = age_at_release(f["released"])
    started = FIRST_COMMIT.strftime("%B %Y")
    first = f"{FIRST_COMMIT.day} {FIRST_COMMIT.strftime('%B %Y')}"
    timeline = [
        ("tl-done", started, f"First commit {first}"),
        ("tl-now", f"{VERSION} &middot; today", f"{months} months in development at the {VERSION} release; not yet stable"),
        ("tl-way", "denoising", "Breaking changes are expected often until 2.0"),
        ("tl-goal", "2.0 &middot; stable", "Stability is the goal of version 2.0, and the project is working toward it."),
    ]
    tl = '<ol class="timeline" aria-label="Project timeline">' + "".join(
        f'<li class="{c}"><b>{a}</b><span>{e(b)}</span></li>' for c, a, b in timeline) + "</ol>"
    rows = [
        ("Promise", "Zero dependencies", "The principle dctx was founded on. Today dctx still has dependencies."),
        ("Goal", "The single best context management tool", "That is what dctx is being built to become."),
    ]
    kv = '<dl class="status-kv">' + "".join(
        f"<div><dt>{a}</dt><dd><b>{e(b)}</b><span>{e(c)}</span></dd></div>" for a, b, c in rows) + "</dl>"
    more = f"""<p>Why: AI providers keep changing how they optimize context on their side. dctx adapts to those changes quickly, but not in real time. Stability arrives with 2.0, and the project is working toward it.</p>
    <p>Where it comes from: dctx began as an internal tool that was never meant to be open-sourced, so parts of it are still messy. It was opened up because colleagues who rely on it needed it in the open, and development continues in public.</p>
    <p class="status-thesis">We see an ultimate solution to this problem, and we are denoising it.</p>
    {kv}"""
    return f"""<aside class="sec-status" id="status" aria-labelledby="status-h">
 <div class="wrap">
  <div class="status-panel">
   <div class="status-main">
    <p class="eyebrow status-eyebrow">Project status &middot; in development since {started}</p>
    <h2 id="status-h">Not yet stable. Working toward 2.0.</h2>
    <p>dctx is <strong>not yet stable</strong> and <strong>not enterprise-ready</strong>. Until version 2.0, expect breaking changes often.</p>
    {tl}
   </div>
   <div class="status-more status-wide">{more}</div>
   <details class="status-more status-narrow"><summary>Why, where it comes from, and the goal</summary>{more}</details>
  </div>
 </div>
</aside>"""


def hero_terminal() -> str:
    parts = []
    for rid in HERO_RECORDS:
        r = rec(rid)
        if rid == "read-json":
            pretty = json.dumps(json.loads(r["stdout"]), ensure_ascii=False, indent=1) + "\n"
            parts.append(transcript(r, stdout=pretty, limit=6, more='<a href="/examples/#read-json">full output</a>',
                                    note="# one JSON line; pretty-printed here"))
        else:
            parts.append(transcript(r))
    return '<span class="ln gap"></span>'.join(parts)


def facts() -> tuple[str, int]:
    r7 = data.run7()
    stab = data.stability()
    langs = data.languages()
    net, net_line = data.network_zero()
    lo, hi, n = r7["cold"]["dctx"]
    rows = [
        ("grep + read", 'the baseline for choosing a mode; count startup and every agent turn. '
         + '<a class="src" href="/docs/modalities/">Choosing how to use dctx</a>'),
        (f"{lo:.2f}&ndash;{hi:.2f}&nbsp;s", f"cold index of {n} benchmark corpora, median of 3 runs. " + src(data.RUN7, "run 7")),
        (f'{langs["total"]}', f'language families: {langs["tiers"]["full"]} full, {langs["tiers"]["outline"]} outline, '
         f'{langs["tiers"]["docs"]} docs. <a class="src" href="/languages/">dctx languages</a>'),
        (stab["text"], f'text and {stab["json"]} JSON answers byte-identical after unrelated edits. ' + src(data.STAB, "report", stab["line"])),
        (str(net), "socket attempts by dctx in every traced benchmark variant. " + src(data.BENCH, "network", net_line)),
    ]
    html = '<dl class="facts">' + "".join(f"<div><dt>{a}</dt><dd>{b}</dd></div>" for a, b in rows) + "</dl>"
    return html, len(rows)


def bench_table() -> str:
    return ('<p>The baseline is grep plus reading files. A controlled whole-session benchmark against it is being '
            'prepared; see <a href="/benchmarks/">benchmarks</a> for what has been measured so far.</p>')


ANIMATIONS = ("two_scopes", "calls_times_context", "prefix_stability", "giant_folder", "safe_path")


def anim(name: str) -> str:
    """ANIMATION HOOK: the HTML of pages.animations.<name>() in its slot, or nothing without the module."""
    if name not in ANIMATIONS or importlib.util.find_spec("pages.animations") is None:
        return ""
    fn = getattr(importlib.import_module("pages.animations"), name, None)
    html = fn() if callable(fn) else ""
    return f'<div class="anim-slot" data-anim="{name}">{html}</div>' if html else ""


def rust_dependency() -> str:
    body = data.section(read("docs/MODALITIES.md"), "Rust library")
    return re.search(r"```toml\n(.*?)```", body, re.S).group(1).rstrip("\n")


def modalities() -> str:
    items = "".join(f'<li><h3><a href="{m["link"]}">{e(m["name"])}</a></h3><p>{md_inline(m["line"][:1].upper() + m["line"][1:])}</p>'
                    f'<p class="status">{e(m["status"])}</p></li>' for m in factsrc.load()["ways"])
    return f'<ul class="mods">{items}</ul>'


def body(scene: dict) -> str:
    f = factsrc.load()
    facts_html, _ = facts()
    privacy = [
        ("Ignore rules first.", "<code>.gitignore</code>, <code>.dctxignore</code> and a global ignore exclude files before they are opened.", "Excluded from normal selection"),
        ("A built-in deny with no override.", "Env files, private keys, credential and secret files are listed by name but never read.", "Sensitive files"),
        ("<code>.git</code> is closed.", "Never listed or read by the map verbs, never read by <code>pack</code>.", "at any depth is never listed"),
        ("No symlink escapes.", "Files open with <code>O_NOFOLLOW</code>; symlinked directories are never entered.", "Symlinks"),
        ("Local only.", "No telemetry, no model calls, no network for pack and map.", "have **no telemetry**"),
    ]
    priv = '<ul class="checks">' + "".join(
        f'<li><strong>{a}</strong> {b} {src("README.md", "README", line_of("README.md", n))}</li>' for a, b, n in privacy) + "</ul>"
    why = ' <a href="/why/">Why dctx exists, in plain words</a>.' if has_why() else ""
    release = f"{REPO}/releases/tag/v{VERSION}"
    html = f"""
<section class="hero" aria-labelledby="hero-title">
 <div class="wrap hero-grid">
  <div class="hero-copy">
   <ul class="chips" aria-label="Project facts">
    <li><a href="/changelog/#v{VERSION.replace('.', '-')}">dctx <b>{VERSION}</b></a></li><li>Apache-2.0</li><li>C/Rust</li><li><a href="/cli/#mcp"><b>{len(data.mcp_tools())}</b> MCP tools</a></li><li>no telemetry</li><li class="chip-status"><a href="#status">not yet stable &middot; pre-2.0</a></li>
   </ul>
   <h1 id="hero-title">Context, <span class="accent">engineered.</span></h1>
   <p class="lede">dctx gives a model the fewest, most relevant bytes of your code, in an order that stays the same between turns. Deterministic, bounded and offline.</p>
   <div class="actions">
    <a class="btn btn-primary" href="#paths">Choose your path</a><a class="btn" href="/docs/">Read the docs</a><a class="btn" href="/playground/">Playground</a><a class="btn" href="{REPO}">GitHub</a>
   </div>
  </div>
  <div class="hero-side">
   {term(hero_terminal(), "~/kiosk  ·  dctx " + VERSION, play="scene", cls="term-hero")}
  </div>
 </div>
 <div class="wrap">{flow(scene)}</div>
</section>

{status(f)}

<section class="sec sec-paths" id="paths" aria-labelledby="notice-h">
 <div class="wrap">
  {notice("notice-h")}
  <p class="path-lead">Choose your path. Each one has its own setup, first command and pitfall:</p>
  {selector(3)}
  {anim("safe_path")}
  <p><a href="/start/">The safe path, the two scopes and every path in full &rarr;</a></p>
 </div>
</section>

<section class="sec" aria-labelledby="what-the-output-looks-like">
 <div class="wrap">{output_examples()}</div>
</section>

<section class="sec" id="proof" aria-labelledby="proof-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="proof-h">What has been measured</h2><p>Each number links to the document it is parsed from.</p></div>
  {facts_html}
  {bench_table()}
  <p class="caveat">Returned bytes do not measure whole-session cost or establish an advantage over grep plus narrow line-range reads. <a href="/docs/modalities/">Choose a mode</a> and count startup context, all model turns and cached input. <a href="/benchmarks/">Evidence and secondary historical tables</a></p>
  {anim("calls_times_context")}
 </div>
</section>

<section class="sec" id="modalities" aria-labelledby="mod-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="mod-h">Ways to use it</h2><p>One binary, several modalities. Each has its own setup, cost and limits.</p></div>
  {modalities()}
  {anim("giant_folder")}
 </div>
</section>

<section class="sec" id="responsibilities" aria-labelledby="resp-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="resp-h">Who is responsible for what</h2><p>Each level of use has its own responsibilities. <a href="/start/#responsibilities">In full</a></p></div>
  {responsibilities()}
 </div>
</section>

<section class="sec" id="philosophy" aria-labelledby="phil-h">
 <div class="wrap cols cols-2">
  <div class="measure">
   <h2 id="phil-h">Philosophy</h2>
   <p>{md_inline(f["philosophy"])}{why}</p>
   {anim("prefix_stability")}
  </div>
  <div>
   <h3 id="privacy">Ignored means never read</h3>
   {priv}
   <p class="note">dctx decides by file name: a secret pasted into an ordinary source file is not detected. {src("README.md", "README", line_of("README.md", "dctx decides by file name only"))}</p>
  </div>
 </div>
</section>

<section class="sec" id="install" aria-labelledby="install-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="install-h">Install</h2><p>Two scopes of the same binary: yours on PATH, and one pinned inside a project.</p></div>
  <div class="cols cols-2">
   <div>
    <h3>System binary <span class="state">available now</span></h3>
    <p>{md_inline(f["install"]["system"])} <a href="{release}">GitHub Releases (v{VERSION})</a> {src("README.md", "README", line_of("README.md", "## Installation"))}</p>
    {code("Quick start (README)", data.readme_block("Quick Start"), "shell")}
    {code("From source (Rust 1.91 or newer)", data.readme_block("From source"), "shell")}
   </div>
   <div>
    <h3>Project libraries <span class="state">registry publication pending</span></h3>
    <p>{md_inline(f["install"]["project"])}</p>
    {code("Cargo.toml (Rust facade, from Git today)", rust_dependency())}
    <h4>Two scopes, one binary</h4>
    {scopes()}
    <p class="note"><a href="/docs/modalities/">Compare modes, installs and status</a></p>
   </div>
  </div>
  {anim("two_scopes")}
 </div>
</section>
"""
    return html
