#!/usr/bin/env python3
"""Generate the static dctx website into website/.

One shared shell (head, header, footer, JSON-LD) for every page; the changelog page is
rendered from CHANGELOG.md so it cannot drift. Run from anywhere:

    python3 tools/website/build.py

Output is plain HTML/CSS/JS with no external requests. Facts on the pages come from
README.md and CHANGELOG.md; do not add a claim here that those files do not make.
"""
from __future__ import annotations

import html
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "website"
ORIGIN = "https://dctx.internet.technology"
REPO = "https://github.com/Eris-Margeta/dctx"
BLOB = REPO + "/blob/main/"
TREE = REPO + "/tree/main/"
VERSION = "1.1.0"
CONTACT = "info@tejl.com"
ORG_ID = ORIGIN + "/#organization"
PERSON_ID = ORIGIN + "/#author"
SITE_ID = ORIGIN + "/#website"
APP_ID = ORIGIN + "/#software"
ICON = ('<svg viewBox="0 0 32 32" aria-hidden="true" focusable="false"><rect width="32" height="32" rx="8" fill="#14101f"/>'
        '<path d="M11.5 9H8.5v14h3M20.5 9h3v14h-3" fill="none" stroke="#fff" stroke-width="2.2" stroke-linecap="square"/>'
        '<circle cx="16" cy="16" r="2.6" fill="#8b5cf6"/></svg>')

NAV = [("/#why", "Why dctx", None), ("/#install", "Install", None), ("/#benchmark", "Benchmark", None),
       ("/docs/", "Docs", "docs"), ("/news/", "News", "news"), ("/changelog/", "Changelog", "changelog")]

e = html.escape


def ld(graph: list) -> str:
    return json.dumps({"@context": "https://schema.org", "@graph": graph}, ensure_ascii=False, indent=1).replace("<", "\\u003c")


def base_graph() -> list:
    return [
        {"@type": "Organization", "@id": ORG_ID, "name": "TEJL", "url": "https://tejl.com/",
         "logo": ORIGIN + "/assets/tejl-logo-off-black.svg", "email": CONTACT, "sameAs": ["https://tejl.hr/"]},
        {"@type": "Person", "@id": PERSON_ID, "name": "Eris Margeta"},
        {"@type": "WebSite", "@id": SITE_ID, "url": ORIGIN + "/", "name": "dctx", "inLanguage": "en",
         "publisher": {"@id": ORG_ID}},
    ]


def software() -> dict:
    return {"@type": "SoftwareApplication", "@id": APP_ID, "name": "dctx", "softwareVersion": VERSION,
            "applicationCategory": "DeveloperApplication", "operatingSystem": "Linux (x86_64 tested)",
            "description": "The context engine for AI-assisted software development: deterministic, bounded, offline code context.",
            "url": ORIGIN + "/", "license": "https://www.apache.org/licenses/LICENSE-2.0",
            "isAccessibleForFree": True, "codeRepository": REPO, "programmingLanguage": "Rust",
            "author": {"@id": PERSON_ID}, "publisher": {"@id": ORG_ID}}


def page(path: str, title: str, desc: str, body: str, *, section: str | None = None, crumbs=None,
         extra_graph=None, og_type="website", full_title: str | None = None, noindex=False) -> None:
    url = ORIGIN + path
    graph = base_graph()
    webpage = {"@type": "WebPage", "@id": url + "#webpage", "url": url, "name": title, "description": desc,
               "inLanguage": "en", "isPartOf": {"@id": SITE_ID}}
    if path == "/":
        webpage["about"] = {"@id": APP_ID}
        graph.append(software())
    if crumbs:
        webpage["breadcrumb"] = {"@id": url + "#breadcrumb"}
        items = [("/", "dctx")] + crumbs
        graph.append({"@type": "BreadcrumbList", "@id": url + "#breadcrumb", "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": n, "item": ORIGIN + p} for i, (p, n) in enumerate(items)]})
    graph.append(webpage)
    graph += extra_graph or []
    doc_title = full_title or f"{title} · dctx"
    links = "".join(
        f'<li><a href="{h}"{" aria-current=\"page\"" if s and s == section else ""}>{t}</a></li>' for h, t, s in NAV)
    crumb_html = ""
    if crumbs:
        parts = ['<li><a href="/">dctx</a></li>'] + [
            f'<li><a href="{p}"{" aria-current=\"page\"" if i == len(crumbs) - 1 else ""}>{e(n)}</a></li>'
            for i, (p, n) in enumerate(crumbs)]
        crumb_html = f'<nav class="crumbs" aria-label="Breadcrumb"><ol>{"".join(parts)}</ol></nav>'
    body = body.replace("{{crumbs}}", crumb_html)
    robots = "noindex, follow" if noindex else "index, follow, max-image-preview:large"
    canonical = f'<link rel="canonical" href="{url}">\n<meta property="og:url" content="{url}">\n'
    out = f"""<!doctype html>
<html lang="en" class="js-off">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{e(doc_title)}</title>
<meta name="description" content="{e(desc)}">
<meta name="robots" content="{robots}">
{canonical}<meta name="color-scheme" content="light dark">
<meta name="theme-color" content="#fafaf9" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#0b0a12" media="(prefers-color-scheme: dark)">
<meta property="og:type" content="{og_type}">
<meta property="og:site_name" content="dctx">
<meta property="og:locale" content="en_US">
<meta property="og:title" content="{e(doc_title)}">
<meta property="og:description" content="{e(desc)}">
<meta property="og:image" content="{ORIGIN}/assets/og.png">
<meta property="og:image:width" content="1200">
<meta property="og:image:height" content="630">
<meta property="og:image:alt" content="dctx: context, engineered. A code tree flows into a compact, stable block of tokens.">
<meta name="twitter:card" content="summary_large_image">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="alternate" type="application/atom+xml" title="dctx news" href="/news/feed.xml">
<script>try{{var t=localStorage.getItem('dctx-theme');if(t==='light'||t==='dark')document.documentElement.setAttribute('data-theme',t)}}catch(e){{}}</script>
<link rel="stylesheet" href="/assets/site.css">
<script src="/assets/site.js" defer></script>
<script type="application/ld+json">
{ld(graph)}
</script>
</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="site-header">
 <div class="wrap nav">
  <a class="brand" href="/">{ICON}<span>dctx</span></a>
  <nav aria-label="Primary"><ul class="nav-links">{links}</ul></nav>
  <div class="nav-end">
   <a class="btn btn-ghost btn-sm btn-gh" href="{REPO}">Source code</a>
   <button class="icon-btn" id="theme-toggle" type="button" aria-label="Switch colour theme" hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 0 0 16z" fill="currentColor"/></svg></button>
   <details class="menu"><summary class="icon-btn" aria-label="Menu"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg></summary>
    <nav class="menu-panel" aria-label="Mobile"><ul>{links}<li><a href="{REPO}">Source code</a></li></ul></nav></details>
  </div>
 </div>
</header>
<main id="main">
{body}
</main>
{FOOTER}
</body>
</html>
"""
    target = OUT / (path.lstrip("/") + "index.html" if path.endswith("/") else path.lstrip("/"))
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(out, encoding="utf-8")


FOOTER = f"""<footer class="site-footer">
 <div class="wrap">
  <div class="foot-grid">
   <div>
    <a class="brand" href="/">{ICON}<span>dctx</span></a>
    <p>The context engine for AI-assisted software development. Fewer tokens, stable prefixes, exact source.</p>
   </div>
   <nav aria-labelledby="f-product"><h2 id="f-product">Product</h2><ul>
    <li><a href="/#why">Why dctx</a></li><li><a href="/#install">Install</a></li><li><a href="/#benchmark">Benchmark</a></li>
    <li><a href="/#privacy">Security and privacy</a></li><li><a href="/maintenance/">Maintenance</a></li></ul></nav>
   <nav aria-labelledby="f-res"><h2 id="f-res">Resources</h2><ul>
    <li><a href="/docs/">Documentation</a></li><li><a href="{BLOB}docs/SPEC.md">Specification</a></li>
    <li><a href="/news/">News</a></li><li><a href="/changelog/">Changelog</a></li><li><a href="/news/feed.xml">Atom feed</a></li></ul></nav>
   <nav aria-labelledby="f-proj"><h2 id="f-proj">Project</h2><ul>
    <li><a href="{REPO}">Source code</a></li><li><a href="{REPO}/issues">Issues</a></li>
    <li><a href="{BLOB}SECURITY.md">Report a vulnerability</a></li><li><a href="{BLOB}LICENSE">License (Apache-2.0)</a></li>
    <li><a href="mailto:{CONTACT}?subject=dctx">Contact</a></li></ul></nav>
  </div>
  <div class="foot-bottom">
   <div>
    <p>&copy; 2025&ndash;2026 Eris Margeta. dctx is open source under Apache-2.0.</p>
    <p>&ldquo;dctx&rdquo; and its logo are trademarks of Eris Margeta (<a href="{BLOB}TRADEMARK.md">trademark policy</a>). This site uses no cookies, analytics or third-party requests.</p>
   </div>
   <div class="credit">
    <p>Published by TEJL<br><a href="https://tejl.com/">tejl.com</a> &middot; <a href="https://tejl.hr/">tejl.hr</a></p>
    <a class="credit-logo" href="https://tejl.com/" aria-label="TEJL, publisher of dctx"><img src="/assets/tejl-logo-off-black.svg" width="818" height="422" alt="" loading="lazy" decoding="async"></a>
   </div>
  </div>
 </div>
</footer>"""


def code(label: str, text: str) -> str:
    lines = []
    for raw in text.strip("\n").split("\n"):
        m = re.match(r"^(.*?)(\s+#\s.*)?$", raw)
        cmd, comment = m.group(1), m.group(2) or ""
        if raw.lstrip().startswith("#"):
            lines.append(f'<span class="c">{e(raw)}</span>')
        else:
            lines.append(f'<span class="p">$ </span>{e(cmd)}' + (f'<span class="c">{e(comment)}</span>' if comment else ""))
    return (f'<div class="code"><div class="code-head"><span>{e(label)}</span>'
            f'<button class="copy" type="button" aria-live="polite" hidden>Copy</button></div>'
            f'<pre><code>{chr(10).join(lines)}</code></pre></div>')


# ---------------------------------------------------------------- home
HOME = f"""
<section class="hero" aria-labelledby="hero-title">
 <div class="wrap">
  <ul class="badge-row" aria-label="Project facts">
   <li><a class="badge" href="/changelog/#v1-1-0">Version <b>{VERSION}</b></a></li>
   <li><span class="badge">Apache-2.0</span></li>
   <li><span class="badge">No telemetry</span></li>
   <li><span class="badge">Written in Rust</span></li>
  </ul>
  <h1 id="hero-title">Context, <span class="accent">engineered.</span></h1>
  <p class="lede">dctx is the context engine for AI-assisted software development. It gives a model the fewest, most relevant bytes of your source code, in an order that stays the same between turns: smaller prompts, prompt caches that hit, and answers built on exact source instead of guesses.</p>
  <div class="actions">
   <a class="btn btn-primary" href="#install">Install from source
    <svg viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><path d="M4 10h12m-5-5 5 5-5 5"/></svg></a>
   <a class="btn btn-ghost" href="{BLOB}docs/SPEC.md">Read the specification</a>
  </div>
  <figure class="hero-art"><img src="/assets/dctx-header.svg" width="1280" height="400" fetchpriority="high"
   alt="Diagram: many file nodes of a code tree flow into one compact, stable block of tokens, labelled deterministic bytes and stable prefix."></figure>
 </div>
</section>

<section class="section-alt" aria-labelledby="facts-h" style="padding-block:clamp(40px,6vw,64px)">
 <div class="wrap">
  <h2 id="facts-h" class="visually-hidden">dctx at a glance</h2>
  <dl class="facts">
   <div class="fact"><dt>140/140</dt><dd>points on our 70-question protocol, against Trail-Code 122 and graphify 96.</dd></div>
   <div class="fact"><dt>0.13&ndash;1.20&nbsp;s</dt><dd>cold index of six benchmark corpora (median of three runs).</dd></div>
   <div class="fact"><dt>50</dt><dd>language families: 36 full, 8 outline, 6 documentation formats.</dd></div>
   <div class="fact"><dt>33/33</dt><dd>text and 33/33 JSON answers byte-identical after unrelated edits.</dd></div>
   <div class="fact"><dt>0</dt><dd>network calls. No telemetry, and dctx never calls a model.</dd></div>
  </dl>
  <p class="note">Measured on one Linux x86_64 machine; details and caveats in <a href="#benchmark">the benchmark</a> and the <a href="{BLOB}docs/research/stability-20261105/REPORT.md">stability report</a>.</p>
 </div>
</section>

<section id="why" aria-labelledby="why-h">
 <div class="wrap">
  <div class="section-head">
   <span class="eyebrow">Why dctx</span>
   <h2 id="why-h">Every token in front of a model is paid for three times.</h2>
   <p>dctx exists to cut all three costs. The specification turns that belief into five testable constraints: every answer is bounded, deterministic, prefix-stable, non-redundant and honest about its precision.</p>
  </div>
  <div class="grid grid-3">
   <article class="card"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><circle cx="12" cy="13" r="8"/><path d="M12 9v4l2.5 2.5M9 2h6"/></svg></div>
    <h3>Prefill</h3><p>A model processes every input token before it answers; you pay for each one and wait for it. dctx answers with exact spans inside an estimated token budget, never whole files by default.</p></article>
   <article class="card"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M4 12a8 8 0 0 1 13.7-5.6L20 8.7M20 4v4.7h-4.7M20 12a8 8 0 0 1-13.7 5.6L4 15.3M4 20v-4.7h4.7"/></svg></div>
    <h3>Cache misses</h3><p>Providers serve a repeated prompt prefix for a fraction of the price, but one changed byte breaks it. The same tree and question give dctx the same bytes, and an unrelated edit leaves map answers unchanged.</p></article>
   <article class="card"><div class="icon"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="4"/><circle cx="12" cy="12" r="1" fill="currentColor"/></svg></div>
    <h3>Attention</h3><p>Answers get worse as irrelevant tokens pile up. dctx ranks syntax-owned spans, never cuts a record in half, and labels every answer with its precision and freshness.</p></article>
  </div>
 </div>
</section>

<section class="section-alt" id="features" aria-labelledby="features-h">
 <div class="wrap">
  <div class="section-head">
   <span class="eyebrow">What you get</span>
   <h2 id="features-h">One engine for every way software is built.</h2>
   <p>The command line, the MCP server, the library and the desktop app share one core, one privacy boundary and the same output.</p>
  </div>
  <div class="grid grid-3">
   <article class="card"><span class="k">dctx map</span><h3>Map mode for agents</h3><p>Ten verbs (map, find, neighbors, explain, read, skeleton, grep, hubs, path, export) with stable ids, cursor paging and lexical call and import edges. Up to 2,000 estimated tokens per answer.</p></article>
   <article class="card"><span class="k">dctx mcp</span><h3>MCP server</h3><p>A stdio MCP server exposes the map verbs as tools, so any MCP-capable agent can query the same bounded, deterministic answers.</p></article>
   <article class="card"><span class="k">dctx pack</span><h3>Whole-project context</h3><p>The whole project, or only what changed, as one structured block on stdout or the clipboard. Tree lines carry no timestamps, so a bare <code>touch</code> leaves the page byte-identical.</p></article>
   <article class="card"><span class="k">No embeddings</span><h3>Zero RAG defects</h3><p>No chunking, no vector store. dctx answers from a lexical graph of syntax-owned spans and verifies source bytes before it emits them.</p></article>
   <article class="card"><span class="k">Ed25519</span><h3>Verified plugins and index</h3><p>Plugin distribution and index verification use Ed25519 signatures. Plugins run out of process with explicit capabilities and consent.</p></article>
   <article class="card"><span class="k">Preview &middot; 0.11</span><h3>dctx Desktop</h3><p>The same engine in a window, for people who prefer one. Free for everyone, including companies. The desktop app is a preview on its own version line.</p></article>
  </div>
  <details class="disclose" id="languages">
   <summary>Supported languages: 36 full, 8 outline, 6 documentation formats</summary>
   <div>
    <p><strong>Full</strong> (symbols and lexical call edges): Rust, JavaScript, TypeScript/TSX, Python, Go, Java, C/C++, C#, Kotlin, Ruby, PHP, Swift, Vue/Svelte/Astro/HTML script blocks, Scala, Dart, Zig, Solidity, Gleam, Fortran, Perl, Groovy, PowerShell, Elixir, Lua, OCaml, R, Julia, Haskell, Erlang, Verilog/SystemVerilog, Terraform/HCL, SQL, Clojure, Common Lisp, Scheme, Emacs Lisp.</p>
    <p><strong>Outline</strong> (symbols only): JSON/JSONC/TOML/YAML, Shell, Makefile, Dockerfile, Protobuf, Pascal, GraphQL, Nix.</p>
    <p><strong>Docs</strong> (headed sections): Markdown, plain text, reStructuredText, AsciiDoc, Org, LaTeX.</p>
    <p class="note">Call edges come from names, scopes and imports, never from a type checker; heuristic families are labelled <code>guess</code>. Run <code>dctx languages</code> for the tier and confidence of every format.</p>
   </div>
  </details>
 </div>
</section>

<section id="install" aria-labelledby="install-h">
 <div class="wrap">
  <div class="section-head">
   <span class="eyebrow">Install</span>
   <h2 id="install-h">Build it from source in a minute.</h2>
   <p>dctx needs a recent stable Rust toolchain with Cargo. Linux x86_64 is the only tested platform; macOS and Windows builds are untested. There is no crates.io package or prebuilt binary yet.</p>
  </div>
  <ol class="steps">
   <li><div><h3>Build the CLI</h3>
{code("zsh", f'''git clone {REPO}
cd dctx
cargo build --locked --release -p dctx-cli
alias dctx="$PWD/target/release/dctx-cli"''')}
    <p class="note">The built executable is named <code>dctx-cli</code>; the alias lets you type <code>dctx</code>. <code>cargo install --locked --path crates/dctx-cli</code> installs it into Cargo's bin directory instead.</p></div></li>
   <li><div><h3>Pack a project for a chat assistant</h3>
{code("zsh", '''dctx pack .              # whole project as one context block
dctx pack . --clipboard  # straight to the clipboard
dctx . -d -c             # only what changed since the last snapshot''')}</div></li>
   <li><div><h3>Give an agent a map</h3>
{code("zsh", """dctx map find "SnapshotConfig"
dctx map read 's:crates/dctx-cli/src/lib.rs#SnapshotConfig' --json
dctx mcp                 # stdio MCP server with the map verbs as tools""")}
    <p class="note">Ids come from <code>map</code> and <code>find</code>. See the <a href="{BLOB}docs/guides/MAP-MODE.md">map mode guide</a> for every verb.</p></div></li>
  </ol>
 </div>
</section>

<section class="section-alt" id="benchmark" aria-labelledby="bench-h">
 <div class="wrap">
  <div class="section-head">
   <span class="eyebrow">Measured in the open</span>
   <h2 id="bench-h">More correct answers, with fewer tokens.</h2>
   <p>Run 7 on 31 October 2026, against Trail-Code 0.28.0 and graphify 0.9.97 in code-only mode. Tokens are estimated as stdout bytes / 3.5 for every tool.</p>
  </div>
  <div class="table-wrap">
   <table>
    <caption>70-question protocol, six pinned corpora, one Linux x86_64 runner</caption>
    <thead><tr><th scope="col">Tool</th><th scope="col" class="num">Points /140</th><th scope="col" class="num">Gold facts /354</th><th scope="col" class="num">Tokens</th><th scope="col" class="num">Tool calls</th></tr></thead>
    <tbody>
     <tr class="lead"><th scope="row">dctx <code>53f8b6b</code></th><td class="num">140</td><td class="num">353</td><td class="num">29,340</td><td class="num">105</td></tr>
     <tr><th scope="row">Trail-Code 0.28.0</th><td class="num">122</td><td class="num">312</td><td class="num">88,923</td><td class="num">70</td></tr>
     <tr><th scope="row">graphify 0.9.97, code-only</th><td class="num">96</td><td class="num">238</td><td class="num">197,203</td><td class="num">91</td></tr>
    </tbody>
   </table>
  </div>
  <div class="caveat">
   <p><strong>Read this before quoting the table.</strong> The protocol was designed by the dctx team, so it can favour dctx. An independently selected protocol (repositories picked by seed, gold answers from language servers, questions written without seeing dctx output) is pending, and its result will be published whatever it shows.</p>
  </div>
  <div class="grid grid-2">
   <div class="card"><h3>Cold index</h3><p>dctx took 0.13&ndash;1.20 s on the six corpora (median of three runs), against 1.04&ndash;52.83 s for Trail-Code and 4.59&ndash;59.56 s for graphify, on an Intel Xeon E5-2680 v2 runner.</p></div>
   <div class="card"><h3>Byte stability</h3><p>On 6 November 2026, 33/33 text and 33/33 JSON answers stayed identical after unrelated comment edits across three pinned corpora, and over five runs.</p></div>
  </div>
  <p class="note">Questions, gold facts, scorer, raw results and scripts: <a href="{BLOB}docs/BENCHMARK-20261030.md">benchmark write-up</a> &middot; <a href="{TREE}docs/research/bench-3way-20261030">raw data</a> &middot; <a href="{BLOB}docs/research/stability-20261105/REPORT.md">stability report</a>.</p>
 </div>
</section>

<section id="privacy" aria-labelledby="privacy-h">
 <div class="wrap grid grid-2" style="align-items:start;gap:var(--s12)">
  <div>
   <span class="eyebrow">Security and privacy</span>
   <h2 id="privacy-h">Ignored means never read.</h2>
   <p class="lede">Selection happens before any byte is read, hashed, parsed or cached, and is checked again before output leaves the engine.</p>
   <p>The release check counts network connect and send calls under <code>strace -f</code> for every map verb, <code>pack</code>, <code>cache status</code> and <code>mcp</code>, and fails on any. dctx decides by file name: a secret pasted into an ordinary source file is not detected.</p>
  </div>
  <ul class="checks">
   <li><strong>Ignore rules first.</strong> <code>.gitignore</code>, <code>.dctxignore</code> and a global ignore exclude files before they are opened; <code>.dctxomitcontent</code> lists a file without reading it.</li>
   <li><strong>A built-in deny with no override.</strong> Env files, private keys, credential and secrets files are listed by name but never read.</li>
   <li><strong><code>.git</code> is closed.</strong> Never listed or read by the map verbs, never read by <code>pack</code>.</li>
   <li><strong>No symlink escapes.</strong> Files open with <code>O_NOFOLLOW</code>; symlinked directories are never entered.</li>
   <li><strong>Local only.</strong> No telemetry, no model calls, no network for pack and map.</li>
  </ul>
 </div>
</section>

<section class="section-alt" id="open-source" aria-labelledby="oss-h">
 <div class="wrap grid grid-2" style="align-items:center;gap:var(--s12)">
  <div>
   <span class="eyebrow">Open source</span>
   <h2 id="oss-h">Free for any use. Paid maintenance keeps it healthy.</h2>
   <p>dctx is Apache-2.0, including for commercial use. No feature is held back behind a paid edition. Companies that depend on it can fund security response times and long-term support through a maintenance agreement.</p>
   <div class="actions" style="justify-content:flex-start">
    <a class="btn btn-primary" href="/maintenance/">Maintenance agreements</a>
    <a class="btn btn-ghost" href="/docs/">Documentation</a>
   </div>
  </div>
  <div class="card">
   <h3>Where it comes from</h3>
   <p>dctx started in 2025 as <code>dircontxt</code>, written in pure C, to give a model the whole project in one structured, versioned block. It was rewritten in Rust in 2026 and grew a privacy-first core, a plugin host, a desktop app and map mode for agents.</p>
   <p><a href="/changelog/">Read the changelog</a></p>
  </div>
 </div>
</section>
"""

# ---------------------------------------------------------------- docs
DOCS = [
    ("Start here", [
        ("docs/SPEC.md", "Specification", "The core belief, the five constraints, the ten RAG defect classes, policy and roadmap."),
        ("README.md", "README", "Installation, quick start, map mode, language tiers and the CLI reference."),
        ("docs/guides/MAP-MODE.md", "Map mode guide", "Every map verb with examples, budgets, ids and precision labels."),
    ]),
    ("Using dctx", [
        ("docs/guides/AGENTS-SNIPPET.md", "Agent snippet", "The AGENTS.md section that tells coding agents to ask dctx first."),
        ("docs/guides/PLUGINS.md", "Plugins guide", "The plugin boundary, capabilities, consent and current limits."),
        ("docs/CLI-EXIT-CODES.md", "CLI exit codes", "The exit-code contract for map, pack, mcp and languages."),
        ("docs/ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md", "Privacy guarantees", "Ignore semantics and the full sensitive-file deny list."),
    ]),
    ("Evidence", [
        ("docs/BENCHMARK-20261030.md", "Three-way benchmark", "dctx, Trail-Code and graphify on the 70-question protocol, with caveats."),
        ("docs/research/stability-20261105/REPORT.md", "Byte-stability report", "Protocol, raw answers and limits of the 33/33 measurement."),
        ("docs/research/trail-code-2026-10/REPORT.md", "Earlier two-way study", "October 2026, before call edges: the benchmark that set the roadmap."),
    ]),
    ("Project", [
        ("docs/CRATE-MAP.md", "Crate map", "How the Rust workspace is organised."),
        ("CONTRIBUTING.md", "Contributing", "How to propose changes, with a DCO sign-off."),
        ("SECURITY.md", "Security policy", "How to report a vulnerability privately."),
        ("MAINTENANCE.md", "Maintenance terms", "What a maintenance agreement covers."),
        ("CHANGELOG.md", "Changelog source", "The source of the changelog page on this site."),
    ]),
]


def docs_body() -> str:
    parts = []
    for heading, items in DOCS:
        lis = "".join(f'<li><a class="link-card" href="{BLOB}{p}"><strong>{e(t)}</strong><span>{e(d)}</span></a></li>' for p, t, d in items)
        slug = re.sub(r"[^a-z]+", "-", heading.lower()).strip("-")
        parts.append(f'<h2 id="{slug}">{e(heading)}</h2><ul class="link-list">{lis}</ul>')
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}
<span class="eyebrow">Documentation</span><h1>Documentation</h1>
<p class="lede">The documentation lives next to the code, so it is versioned with it. These links open the current main branch of the repository.</p></div>
<div class="prose">{''.join(parts)}</div></div>"""


# ---------------------------------------------------------------- news
NEWS = [
    {
        "slug": "dctx-1-1", "date": "2026-11-04", "kind": "Release",
        "title": "dctx 1.1: three more languages, a prefix-stable pack, and a measured lead.",
        "summary": "Perl, Fortran and Verilog join map mode; dctx pack drops timestamps so prompt caches hit; the three-way benchmark and the byte-stability measurement are public.",
        "body": f"""
<p>dctx {VERSION} was released on 4 November 2026. It folds in the 1.0.1 line and follows the first v1 release of 31 October.</p>
<h2>What is new</h2>
<ul>
<li><strong>Perl, Fortran and Verilog/SystemVerilog</strong> join the full tier, with symbols and lexical call edges labelled <code>guess</code>. dctx now covers 50 language families: 36 full, 8 outline and 6 documentation formats.</li>
<li><strong>A prefix-stable <code>dctx pack</code>.</strong> Tree lines carry the file id only, with no modification time and no size, so a bare <code>touch</code> leaves the page byte-identical and an edit keeps everything before the edited file as a shared prefix. <code>--tree-stamps=on</code> brings back the 1.0.0 lines.</li>
<li><strong>Faster id lookup</strong>, record and entry caps checked during the scan, and a closed stdout pipe that exits cleanly.</li>
</ul>
<h2>What we measured</h2>
<p>On our own 70-question protocol, run on 31 October against Trail-Code 0.28.0 and graphify 0.9.97 in code-only mode, dctx scored 140 of 140 points with 29,340 estimated tokens; Trail-Code scored 122 with 88,923 and graphify 96 with 197,203. Cold indexing of the six corpora took dctx 0.13&ndash;1.20 s.</p>
<p>That protocol was designed by the dctx team and can favour dctx. An independently selected protocol is pending, and we will publish its result whatever it shows.</p>
<p>On 6 November, 33 of 33 text answers and 33 of 33 JSON answers stayed byte-identical after unrelated edits across three pinned corpora. Three weeks earlier, answer headers that changed after unrelated edits were one of the findings that <a href="/news/specification-and-benchmark/">our first benchmark</a> held against us.</p>
<p><a href="{BLOB}docs/BENCHMARK-20261030.md">Benchmark write-up</a> &middot; <a href="{BLOB}docs/research/stability-20261105/REPORT.md">Stability report</a> &middot; <a href="/changelog/#v1-1-0">Full changelog</a></p>
"""},
    {
        "slug": "desktop-free-forever", "date": "2026-10-21", "kind": "Announcement",
        "title": "dctx Desktop is now free. Forever.",
        "summary": "For individuals, teams and companies. No trial, no seat count, no expiry.",
        "body": f"""
<p>Starting today, the dctx desktop app is free for everyone who uses it: a single developer, a ten-person studio or an engineering organisation of thousands. It stays that way.</p>
<h2>Why we are doing this</h2>
<p>dctx exists because of one belief: prompt-processing efficiency. Every token in front of a model is paid for three times, in prefill, in cache misses and in attention. The smaller, more exact and more stable the context, the better and cheaper every answer becomes.</p>
<p>The desktop app is how people who do not live in a terminal reach that engine: designers, analysts, product managers and engineers who simply prefer a window. Charging for it put a toll at the front door of the very habit we want to spread. So we are removing the toll.</p>
<blockquote>Efficient context should be the default for everyone who works with a model, not a premium feature.</blockquote>
<h2>What you get</h2>
<ul>
<li>The same engine as the command line and the library: one core, the same privacy guarantee, the same output.</li>
<li>Drag-and-drop or native folder selection, and snapshots generated without blocking the interface.</li>
<li>Your global dctx settings and ignore rules, applied exactly as on the command line.</li>
<li>Everything runs on your machine. No account, no telemetry, no network.</li>
</ul>
<p>The source is in the open repository under Apache-2.0, next to the CLI and the core. The desktop app is a preview on its own version line (0.11).</p>
<h2>What does not change</h2>
<p>dctx remains open source. The repository is ongoing work, and we publish what we measure, including where we still have work to do, as in <a href="/news/specification-and-benchmark/">our first public benchmark</a>.</p>
<p>Organisations that depend on dctx can sign a <a href="/maintenance/">maintenance agreement</a> for security response times, long-term support and priority work. Paying never changes the license, and it funds the work that keeps the desktop app free.</p>
<p><em>Already bought a license? Thank you. You helped build what everyone now gets for free.</em></p>
"""},
    {
        "slug": "specification-and-benchmark", "date": "2026-10-21", "kind": "Engineering",
        "title": "Our specification, and a benchmark that does not flatter us.",
        "summary": "The dctx Context Management Specification, and an isolated comparison with Trail-Code: wins, losses and what comes next.",
        "body": f"""
<div class="caveat"><p><strong>Historical post.</strong> These results describe dctx as of October 2026, before call edges and byte-stable answers. For current measurements, read <a href="/news/dctx-1-1/">dctx 1.1</a>.</p></div>
<p>An engineering organisation should be able to check every claim a tool makes. So we wrote down exactly what dctx is meant to be, and then measured where it stands today.</p>
<h2>The specification</h2>
<p>The dctx Context Management Specification turns our core belief, prompt-processing efficiency, into five testable constraints: every answer is bounded, deterministic, prefix-stable, non-redundant and honest about its precision and freshness. It names ten ways chunk retrieval fails and the mechanism that rules out each one, defines a seven-layer organisation policy in which every layer can only narrow the one above it, and lays out the roadmap with an acceptance gate for every step. Each item is marked as available, under contract or a target.</p>
<h2>The benchmark</h2>
<p>We ran dctx and Trail-Code side by side on a dedicated machine: seven pinned code bases in Rust, Go, Python and TypeScript, no network during the runs, every command pinned to the same CPUs, and an adversarial corpus with planted secrets. The full report, raw data and scripts are public so anyone can rerun it.</p>
<h3>Where dctx led</h3>
<ul>
<li>Cold indexing on every code base: 0.09&ndash;6.05 s against 2.4&ndash;95 s, with 1.6&ndash;27&times; less memory.</li>
<li>Footprint: one 9.2 MB binary against 405.6 MB of Node.js packages.</li>
<li>Bounds: map answers never exceeded their token budget.</li>
<li>Network: dctx opened no sockets and wrote nothing into the repositories.</li>
</ul>
<h3>Where Trail-Code led</h3>
<ul>
<li>Call graphs: dctx's map command did not yet expose call or import edges.</li>
<li>Answers to agent questions: 122 of 140 points against 69 of 140, with fewer tokens.</li>
<li>Stability under edits: Trail-Code kept most answers byte-identical after an unrelated change; dctx did not, which costs prompt-cache hits.</li>
<li>Incremental refresh after an edit.</li>
</ul>
<h2>What happens next</h2>
<p>The findings are now the first items on our roadmap: call and import edges with honest confidence labels, a persistent incremental index, answer headers that stay stable across unrelated edits, a bounded repository overview, a search verb, and safer defaults for secrets when packing a whole project. Each one ships only when it passes the gate the specification sets for it, and we will publish the next measurement the same way.</p>
<p><a href="{BLOB}docs/SPEC.md">Read the specification</a> &middot; <a href="{BLOB}docs/research/trail-code-2026-10/REPORT.md">Read the benchmark report</a></p>
"""},
]

MONTHS = "January February March April May June July August September October November December".split()


def human(date: str) -> str:
    y, m, d = date.split("-")
    return f"{int(d)} {MONTHS[int(m) - 1]} {y}"


def news_pages() -> None:
    items = "".join(
        f'<li class="post"><p class="meta"><time datetime="{n["date"]}">{human(n["date"])}</time><span class="tag">{n["kind"]}</span></p>'
        f'<h2><a href="/news/{n["slug"]}/">{e(n["title"])}</a></h2><p>{e(n["summary"])}</p></li>' for n in NEWS)
    page("/news/", "News", "Releases, measurements and product decisions from the dctx project.",
         f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">News</span><h1>News</h1>
<p class="lede">Releases, measurements and product decisions from the dctx project. Also available as an <a href="/news/feed.xml">Atom feed</a>.</p></div>
<ul class="post-list">{items}</ul></div>""", section="news", crumbs=[("/news/", "News")])
    for n in NEWS:
        path = f"/news/{n['slug']}/"
        art = {"@type": "BlogPosting", "@id": ORIGIN + path + "#article", "headline": n["title"],
               "description": n["summary"], "datePublished": n["date"], "inLanguage": "en",
               "mainEntityOfPage": {"@id": ORIGIN + path + "#webpage"}, "author": {"@id": ORG_ID},
               "publisher": {"@id": ORG_ID}, "image": ORIGIN + "/assets/og.png"}
        body = f"""<div class="wrap"><article><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">{n['kind']}</span>
<h1>{e(n['title'])}</h1><p class="lede">{e(n['summary'])}</p>
<p class="meta"><time datetime="{n['date']}">{human(n['date'])}</time><span>The dctx project</span></p></div>
<div class="prose">{n['body']}<p><a href="/news/">&larr; All news</a></p></div></article></div>"""
        page(path, n["title"], n["summary"], body, section="news", crumbs=[("/news/", "News"), (path, n["title"])],
             extra_graph=[art], og_type="article")
    entries = "".join(
        f"""<entry><title>{e(n['title'])}</title><link href="{ORIGIN}/news/{n['slug']}/"/><id>{ORIGIN}/news/{n['slug']}/</id>
<updated>{n['date']}T00:00:00Z</updated><summary>{e(n['summary'])}</summary></entry>""" for n in NEWS)
    (OUT / "news" / "feed.xml").write_text(f"""<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"><title>dctx news</title><link href="{ORIGIN}/news/"/><link rel="self" href="{ORIGIN}/news/feed.xml"/>
<id>{ORIGIN}/news/</id><updated>{max(n['date'] for n in NEWS)}T00:00:00Z</updated><author><name>The dctx project</name></author>
{entries}
</feed>
""", encoding="utf-8")


# ---------------------------------------------------------------- changelog (from CHANGELOG.md)
def inline(s: str) -> str:
    s = e(s, quote=False)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)

    def link(m):
        href = m.group(2).replace("https://dctx.app/", "/")
        if not re.match(r"^(https?:|/|#|mailto:)", href):
            href = BLOB + href
        return f'<a href="{href}">{m.group(1)}</a>'
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, s)


def changelog() -> None:
    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    refs = dict(re.findall(r"^\[([^\]]+)\]:\s*(\S+)$", text, re.M))
    blocks = re.split(r"^## ", text, flags=re.M)[1:]
    releases, toc = [], []
    for blk in blocks:
        head, _, rest = blk.partition("\n")
        m = re.match(r"(?:(dircontxt) )?\[([^\]]+)\](?:\s*[—-]\s*(\S+))?", head)
        if not m:
            continue
        prefix, ver, date = m.group(1), m.group(2), m.group(3)
        name = f"{prefix} {ver}" if prefix else ver
        slug = "v" + re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
        out, para, items = [], [], []

        def flush():
            if para:
                out.append("<p>" + inline(" ".join(para)) + "</p>"); para.clear()
            if items:
                out.append("<ul>" + "".join(f"<li>{inline(i)}</li>" for i in items) + "</ul>"); items.clear()
        for line in rest.split("\n"):
            if re.match(r"^\[[^\]]+\]:", line):
                continue
            if line.startswith("### "):
                flush(); out.append(f"<h3>{inline(line[4:])}</h3>")
            elif line.startswith("- "):
                if para:
                    flush()
                items.append(line[2:].strip())
            elif line.startswith("  ") and items and line.strip():
                items[-1] += " " + line.strip()
            elif not line.strip():
                if para:
                    flush()
            else:
                if items:
                    flush()
                para.append(line.strip())
        flush()
        badge = '<span class="tag">In development</span>' if ver == "Unreleased" else ""
        when = f'<small><time datetime="{date}">{date}</time></small>' if date else ""
        compare = f'<p class="note"><a href="{refs[ver]}">Compare in the repository</a></p>' if ver in refs and not prefix else ""
        releases.append(f'<section class="release" id="{slug}" aria-labelledby="{slug}-h" style="padding:var(--s8) 0 0">'
                        f'<h2 id="{slug}-h">{e(name)} {when} {badge}</h2>{"".join(out)}{compare}</section>')
        toc.append(f'<li><a href="#{slug}">{e(name)}</a></li>')
    body = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Changelog</span><h1>Every change, in the open.</h1>
<p class="lede">What changed in dctx, release by release, including known issues we have measured and not yet fixed. This page is generated from <a href="{BLOB}CHANGELOG.md">CHANGELOG.md</a>.</p>
<nav aria-label="Releases"><ul class="toc">{''.join(toc)}</ul></nav></div>
<div class="prose">{''.join(releases)}</div></div>"""
    page("/changelog/", "Changelog", "Every dctx release, from dircontxt 1.0.0 in C to dctx " + VERSION + " in Rust, with known issues.",
         body, section="changelog", crumbs=[("/changelog/", "Changelog")])


# ---------------------------------------------------------------- maintenance
MAINT = f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Maintenance</span>
<h1>Free to use. Paid maintenance keeps it healthy.</h1>
<p class="lede">dctx is Apache-2.0 open source, free for any use including commercial use. A maintenance agreement pays for security response, long-term support and priority work. Paying never changes the license and never closes a feature.</p></div>
<h2 id="tiers">Three ways to support dctx</h2>
<div class="grid grid-3" style="margin-bottom:var(--s16)">
 <article class="card"><span class="k">On request</span><h3>Supporter</h3><p>Individuals and teams who want to fund dctx.</p>
  <ul class="checks"><li>Sponsorship through GitHub Sponsors</li><li>Your logo in the README</li><li>No service commitments</li></ul>
  <p style="margin-top:var(--s6)"><a class="btn btn-ghost" href="https://github.com/sponsors/Eris-Margeta">GitHub Sponsors</a></p></article>
 <article class="card"><span class="k">On request</span><h3>Team</h3><p>For one organisation that depends on dctx.</p>
  <ul class="checks"><li>Security response: critical 72 hours, high 7 days, medium next release</li><li>Long-term support: backported fixes for 18 months after 2.0</li><li>Priority on bug reports</li><li>A say in the roadmap through a regular review</li><li>Advance notice of breaking changes</li><li>Invoice and a written maintenance commitment for procurement</li></ul>
  <p style="margin-top:var(--s6)"><a class="btn btn-primary" href="mailto:{CONTACT}?subject=dctx%20maintenance%20(Team)">Ask for pricing</a></p></article>
 <article class="card"><span class="k">On request</span><h3>Partner</h3><p>For organisations that build products on dctx.</p>
  <ul class="checks"><li>Everything in Team</li><li>Integration help</li><li>A named contact</li></ul>
  <p style="margin-top:var(--s6)"><a class="btn btn-ghost" href="mailto:{CONTACT}?subject=dctx%20maintenance%20(Partner)">Ask for pricing</a></p></article>
</div>
<div class="prose">
<h2 id="guarantees">What paying never does</h2>
<ul class="checks">
<li>Paying never changes the license. dctx stays Apache-2.0 for everyone.</li>
<li>Paying never closes or paywalls a feature. Every feature stays in the open source project.</li>
<li>The core (<code>pack</code>) and every modality stay open.</li>
<li>Security reports still go through <a href="{BLOB}SECURITY.md">SECURITY.md</a>, whatever your tier.</li>
</ul>
<p style="margin-top:var(--s8)">The full terms are in <a href="{BLOB}MAINTENANCE.md">MAINTENANCE.md</a>. Trademark use is covered by <a href="{BLOB}TRADEMARK.md">TRADEMARK.md</a>.</p>
</div></div>"""


def main() -> None:
    page("/", "dctx", "dctx is the open-source context engine for AI-assisted development: deterministic, bounded code context for agents and chat, with no telemetry and no network. Apache-2.0.",
         HOME, full_title="dctx: the context engine for AI-assisted development")
    page("/docs/", "Documentation", "Specification, guides, benchmark reports and project policies for dctx.", docs_body(),
         section="docs", crumbs=[("/docs/", "Documentation")])
    news_pages()
    changelog()
    page("/maintenance/", "Maintenance", "dctx is free under Apache-2.0. Maintenance agreements fund security response, long-term support and priority work.",
         MAINT, crumbs=[("/maintenance/", "Maintenance")])
    page("/404.html", "Page not found", "This page does not exist on the dctx website.",
         """<div class="wrap notfound"><span class="eyebrow">404</span><h1>Page not found.</h1>
<p class="lede">The page you asked for is not here. It may have moved.</p>
<div class="actions"><a class="btn btn-primary" href="/">dctx home</a><a class="btn btn-ghost" href="/docs/">Documentation</a></div></div>""",
         noindex=True)
    routes = ["/", "/docs/", "/news/"] + [f"/news/{n['slug']}/" for n in NEWS] + ["/changelog/", "/maintenance/"]
    (OUT / "sitemap.xml").write_text('<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
                                     + "".join(f"<url><loc>{ORIGIN}{r}</loc></url>\n" for r in routes) + "</urlset>\n", encoding="utf-8")
    (OUT / "robots.txt").write_text(f"User-agent: *\nAllow: /\n\nSitemap: {ORIGIN}/sitemap.xml\n", encoding="utf-8")
    (OUT / "favicon.svg").write_text(ICON.replace(' aria-hidden="true" focusable="false"', ' xmlns="http://www.w3.org/2000/svg"') + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
