"""The home page: hero scene and terminal, facts, how it works, verbs, install, benchmark, privacy."""
from __future__ import annotations

import json
import re

import data
from common import BLOB, REPO, TREE, VERSION, demo, e, line_of, read, rec, src
from components import code, output_examples, table, term, transcript
from routes import DOCS, has_why

HERO_RECORDS = ["pack-for", "find", "neighbors-in", "read-json"]


def help_line(record_id: str, needle: str) -> str:
    """The first line of a captured help page that contains needle, verbatim."""
    for line in rec(record_id)["stdout"].split("\n"):
        if needle in line:
            return line.strip()
    raise SystemExit(f"home: {needle!r} not in {record_id}")


def inline_code(text: str) -> str:
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text, quote=False))


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


def verbs() -> str:
    """One row per command, each meaning taken verbatim from the captured help."""
    rows = []
    def row(name: str, anchor: str, meaning: str, example: str | None):
        page = {"languages-md": "/languages/", "template-default": "/templates/"}.get(example or "")
        ex = f'<a href="{page}">{"table" if example == "languages-md" else "templates"}</a>' if page else (
            f'<a href="/examples/#{example}">example</a>' if example else "")
        rows.append([f'<a href="/cli/#{anchor}"><code>{e(name)}</code></a>', inline_code(meaning), ex])
    row("dctx pack", "help", help_line("help", "Explicit `dctx pack`"), "pack")
    row("dctx pack --for", "help", help_line("help", "`dctx pack --for <TEXT>`"), "pack-for")
    examples = {"map": "map-map", "find": "find", "neighbors": "neighbors-in", "explain": "explain", "read": "read-json",
                "skeleton": "skeleton", "grep": "grep", "hubs": "hubs", "path": "path", "export": "export-mermaid"}
    for verb in ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]:
        lines = rec(f"help-{verb}")["stdout"].split("\n")
        row(f"dctx map {verb}", verb, lines[1].strip(), examples[verb])
    row("dctx mcp", "mcp", help_line("help", "`dctx mcp` serves"), "mcp")
    row("dctx languages", "languages", help_line("help", "`dctx languages` lists"), "languages-md")
    row("dctx template", "help", help_line("help", "template build <PATH>  Compile").split("  ", 1)[1].strip() + "; "
        + help_line("help", "template list          List").split("  ", 1)[1].strip(), "template-default")
    row("dctx cache", "help", help_line("help", "`dctx cache config` shows"), None)
    return table(["Command", "What it does (from <code>--help</code>)", ""], rows, raw=True, cls="verbs")


def ppe() -> str:
    """The five PPE constraints, parsed from SPEC §2; status cut at its first clause."""
    line, head, rows = data.table_after("docs/SPEC.md", "2. The core belief", "MUST satisfy the five")
    out = []
    for ident, constraint, status in rows:
        name = re.sub(r"\*\*PPE-\d+ ([^*]+)\*\*", r"\1", ident)
        short = re.split(r"[;(]", status, 1)[0].strip()
        out.append([f"<b>{e(name)}</b>", e(constraint, quote=False), e(short, quote=False)])
    return table(["Constraint", "Definition", "Status"], out, raw=True,
                 caption=f'{src("docs/SPEC.md", "SPEC §2", line)}')


def bench_table() -> str:
    return ('<p>The baseline is grep plus reading files. A controlled whole-session benchmark against it is being '
            'prepared; see <a href="/benchmarks/">benchmarks</a> for what has been measured so far.</p>')


def body(scene: dict) -> str:
    d = demo()["summary"]
    facts_html, _ = facts()
    c = scene["counts"]
    readme_build = data.readme_block("From source")
    steps = [
        ("repository", f'{c["disk"]} files on disk'),
        ("policy", f'{c["listed"]} listed, {c["read"]} read'),
        ("graph", f'{c["nodes"]} nodes, {c["edges"]} call/import edges'),
        ("bounded page", f'{c["body_tokens"]} of {c["budget"]} tokens'),
    ]
    steps_html = '<ol class="steps" aria-label="What the animation shows">' + "".join(
        f"<li><b>{a}</b><span>{b}</span></li>" for a, b in steps) + "</ol>"
    why_link = ""
    if has_why():
        why_link = ('<section class="sec why-cta" aria-labelledby="why-h"><div class="wrap"><h2 id="why-h"><a href="/why/">Why dctx exists</a></h2>'
                    f'<p>{e(DOCS[0][4])} <a href="/why/">Read it in plain words &rarr;</a></p></div></section>\n')
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
    html = f"""
<section class="hero" aria-labelledby="hero-title">
 <div class="wrap hero-grid">
  <div class="hero-copy">
   <ul class="chips" aria-label="Project facts">
    <li><a href="/changelog/#v{VERSION.replace('.', '-')}">dctx <b>{VERSION}</b></a></li><li>Apache-2.0</li><li>Rust</li><li>Linux x86_64 tested</li><li><a href="/cli/#mcp"><b>{len(data.mcp_tools())}</b> MCP tools</a></li><li>no telemetry</li>
   </ul>
   <h1 id="hero-title">Context, <span class="accent">engineered.</span></h1>
   <p class="lede">dctx gives a model the fewest, most relevant bytes of your code, in an order that stays the same between turns. Deterministic, bounded and offline.</p>
   <div class="install"><span class="p" aria-hidden="true">$</span><code>git clone {REPO} &amp;&amp; cd dctx &amp;&amp; cargo build --locked --release -p dctx-cli</code></div>
   <div class="actions" style-free>
    <a class="btn btn-primary" href="/docs/readme/">Read the docs</a><a class="btn" href="/cli/">CLI reference</a><a class="btn" href="/playground/">Playground</a><a class="btn" href="{REPO}">GitHub</a>
   </div>
   <h2 class="eyebrow" id="scene-h">On the sample project <a href="/examples/">kiosk</a></h2>
   {steps_html}
   {code("Quick start (README)", data.readme_block("Quick Start"), "shell")}
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
 <div class="wrap facts-row">{facts_html}</div>
 <script type="application/json" id="scene-data">{scene_json}</script>
</section>

<section class="sec" aria-labelledby="what-the-output-looks-like">
 <div class="wrap">{output_examples()}</div>
</section>

{why_link}<section class="sec" aria-labelledby="how-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="how-h">How it works</h2><p>Four steps, every one checkable. The scene above runs them on the {c['disk']}-file sample.</p></div>
  <div class="cols cols-2">
   <ol class="how">
    <li><div><b>Select before reading.</b><p>Ignore rules and the built-in deny decide which files exist for dctx before a byte is read, hashed, parsed or cached. In the sample, <code>target/</code> and the binary <code>assets/logo.png</code> are left out, and <code>.env</code> is listed by name only. {src("README.md", "README", line_of("README.md", "Selection happens before any byte is read"))}</p></div></li>
    <li><div><b>Index syntax, not chunks.</b><p>Files become syntax-owned symbols, modules and headings with lexical call and import edges. No embeddings and no vector store; edges carry a confidence label. {src("README.md", "README", line_of("README.md", "dctx does not use embeddings"))}</p></div></li>
    <li><div><b>Answer within a budget.</b><p>Every answer fits an estimated token budget (up to 2,000; explain up to 1,000) and never splits a record. <code>pack --for</code> on the sample used {d['for_body_tokens']} of {d['for_budget']} tokens and dropped {d['for_dropped']}. {src("README.md", "README", line_of("README.md", "**Bounded**"))}</p></div></li>
    <li><div><b>Same bytes every time.</b><p>The same tree and question give the same bytes, so prompt caches hit. Default answers carry no timestamps, digests or ages; <code>--meta</code> restores them. {src("README.md", "README", line_of("README.md", "**Deterministic**"))}</p></div></li>
   </ol>
   <div>
    <p>A conforming implementation must satisfy five constraints. Status as the specification states it:</p>
    {ppe()}
   </div>
  </div>
 </div>
</section>

<section class="sec" aria-labelledby="verbs-h">
 <div class="wrap">
  <div class="sec-head"><h2 id="verbs-h">Commands</h2><p>The meaning of each command is its own help text, captured from dctx {VERSION}.</p></div>
  {verbs()}
 </div>
</section>

<section class="sec" id="install" aria-labelledby="install-h">
 <div class="wrap cols cols-2">
  <div>
   <div class="sec-head"><h2 id="install-h">Install</h2></div>
   <p>Download a prebuilt archive from <a href="https://github.com/Eris-Margeta/dctx/releases/tag/v1.2.0">GitHub Releases</a> and verify it against SHA256SUMS. CI covers Linux, macOS and Windows. npm and crates.io installation are coming soon. Source builds need Rust 1.91 or newer. {src("README.md", "README", line_of("README.md", "## Installation"))}</p>
   {code("From source", readme_build, "shell")}
   <p>Use the CLI for chat, shell agents and CI, or its bundled stdio MCP server for Claude Code, Cursor and Codex.
   Embed the Rust <code>dctx</code> facade through a tagged Git dependency today; crates.io and <code>npx dctx</code>
   are publishing soon. The desktop app is a Tauri preview with a rebuild planned; Python and Node bindings are planned.
   <a href="/docs/modalities/">Compare modes, installs and status</a> &middot;
   <a href="/docs/readme/#use-as-a-library">Use as a library</a>.</p>
  </div>
  <div>
   <div class="sec-head"><h2 id="project-h">Project</h2></div>
   <dl class="kv">
    <dt>licence</dt><dd>Apache-2.0, including commercial use. {src("LICENSE", "LICENSE")}</dd>
    <dt>maintenance</dt><dd>Optional agreements fund security response and long-term support. <a href="/maintenance/">Terms</a></dd>
    <dt>security</dt><dd>Report a vulnerability privately. <a href="/security/">Policy</a> &middot; <a href="/.well-known/security.txt">security.txt</a></dd>
    <dt>trademark</dt><dd>&ldquo;dctx&rdquo; and its logo are trademarks of Eris Margeta. <a href="/trademark/">Policy</a></dd>
    <dt>history</dt><dd>Started in 2025 as <code>dircontxt</code> in C; rewritten in Rust in 2026. {src("README.md", "README", line_of("README.md", "## Where it comes from"))}</dd>
    <dt>source</dt><dd><a href="{REPO}">{REPO.removeprefix("https://")}</a> &middot; <a href="{TREE}crates">crates</a> &middot; <a href="{BLOB}CHANGELOG.md">CHANGELOG.md</a></dd>
   </dl>
  </div>
 </div>
</section>

<section class="sec" id="privacy" aria-labelledby="privacy-h">
 <div class="wrap cols cols-2">
  <div>
   <div class="sec-head"><h2 id="privacy-h">Ignored means never read</h2></div>
   {priv}
   <p class="note">dctx decides by file name: a secret pasted into an ordinary source file is not detected. On the sample, <a href="/examples/#grep"><code>map grep example</code></a> does not find the <code>.env</code> value, which contains that word. {src("README.md", "README", line_of("README.md", "dctx decides by file name only"))}</p>
  </div>
  <div>
   <div class="sec-head"><h2 id="bench-h">Benchmark</h2></div>
   {bench_table()}
   <p class="caveat">Returned bytes do not measure whole-session cost or establish an advantage over grep plus narrow line-range reads. <a href="/docs/modalities/">Choose a mode</a> and count startup context, all model turns and cached input. <a href="/benchmarks/">Evidence and secondary historical tables</a></p>
  </div>
 </div>
</section>
"""
    return html.replace(" style-free", "")
