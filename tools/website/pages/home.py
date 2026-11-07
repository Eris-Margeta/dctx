"""The home page, in the order of the design review (R3, section 2): claim and hero animation; the
advanced-user notice with the choose-your-path selector; real captured output; what has been measured;
the modalities; responsibilities; philosophy; install per scope. The footer (shell.py) links llms.txt.

Animation hook: pages/animations.py is written on its own card. When it exists, its functions
two_scopes(), calls_times_context(), prefix_stability(), giant_folder() and safe_path() each return an
HTML string that is placed in the matching slot below (see anim()); when it does not, the slots are empty."""
from __future__ import annotations

import importlib
import importlib.util
import json
import re

import data
import facts as factsrc
from common import REPO, VERSION, e, line_of, read, rec, src
from components import code, output_examples, term, transcript
from pages.paths import md_inline, notice, responsibilities, selector
from routes import has_why

HERO_RECORDS = ["pack-for", "find", "neighbors-in", "read-json"]


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
    items = "".join(f'<li><h3><a href="{m["href"]}">{e(m["name"])}</a></h3><p>{md_inline(m["line"][:1].upper() + m["line"][1:])}</p>'
                    f'<p class="status">{e(m["status"])}</p></li>' for m in factsrc.load()["modalities"])
    return f'<ul class="mods">{items}</ul>'


def body(scene: dict) -> str:
    f = factsrc.load()
    facts_html, _ = facts()
    c = scene["counts"]
    steps = [
        ("repository", f'{c["disk"]} files on disk'),
        ("policy", f'{c["listed"]} listed, {c["read"]} read'),
        ("graph", f'{c["nodes"]} nodes, {c["edges"]} call/import edges'),
        ("bounded page", f'{c["body_tokens"]} of {c["budget"]} tokens'),
    ]
    steps_html = '<ol class="steps" aria-label="What the animation shows">' + "".join(
        f"<li><b>{a}</b><span>{b}</span></li>" for a, b in steps) + "</ol>"
    scene_json = json.dumps(scene, ensure_ascii=False, separators=(",", ":")).replace("<", "\\u003c")
    poster_alt = (f"The kiosk sample as dctx sees it: {c['nodes']} nodes and {c['edges']} call and import edges, the query find greet "
                  f"lighting s:src/util.rs#greet, and its {c['body_tokens']}-token page inside a {c['budget']}-token budget.")
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
    <li><a href="/changelog/#v{VERSION.replace('.', '-')}">dctx <b>{VERSION}</b></a></li><li>Apache-2.0</li><li>Rust</li><li>Linux x86_64 tested</li><li><a href="/cli/#mcp"><b>{len(data.mcp_tools())}</b> MCP tools</a></li><li>no telemetry</li>
   </ul>
   <h1 id="hero-title">Context, <span class="accent">engineered.</span></h1>
   <p class="lede">dctx gives a model the fewest, most relevant bytes of your code, in an order that stays the same between turns. Deterministic, bounded and offline.</p>
   <div class="actions">
    <a class="btn btn-primary" href="#paths">Choose your path</a><a class="btn" href="/docs/">Read the docs</a><a class="btn" href="/playground/">Playground</a><a class="btn" href="{REPO}">GitHub</a>
   </div>
   <h2 class="eyebrow" id="scene-h">On the sample project <a href="/examples/">kiosk</a></h2>
   {steps_html}
  </div>
  <div class="hero-side">
   <div class="stage" data-scene aria-labelledby="scene-h">
    <picture><source srcset="/assets/og/scene.dark.png" media="(prefers-color-scheme: dark)"><img src="/assets/og/scene.light.png" width="1200" height="630" alt="{e(poster_alt)}" fetchpriority="high"></picture>
    <canvas aria-hidden="true"></canvas>
    <div class="stage-cap"><span data-cap>4 bounded page &middot; {c['body_tokens']} of {c['budget']} tokens</span><button class="tbtn" type="button" data-replay hidden>Replay</button></div>
   </div>
   {term(hero_terminal(), "~/kiosk  ·  dctx " + VERSION, play="scene", cls="term-hero")}
  </div>
 </div>
 <script type="application/json" id="scene-data">{scene_json}</script>
</section>

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
    <h3>Project libraries <span class="state">publishing soon</span></h3>
    <p>{md_inline(f["install"]["project"])}</p>
    {code("Cargo.toml (Rust facade, from Git today)", rust_dependency())}
    <p>{md_inline(f["scopes"]["text"])}</p>
    <p class="note">{md_inline(f["scopes"]["status"])} <a href="/docs/modalities/">Compare modes, installs and status</a></p>
   </div>
  </div>
  {anim("two_scopes")}
 </div>
</section>
"""
    return html
