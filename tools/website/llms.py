"""Machine-readable files: llms.txt, llms-small.txt, llms-full.txt, feeds, sitemap, robots, security.txt,
humans.txt, manifest, OpenSearch, the search index and build.json. All derived from the route table."""
from __future__ import annotations

import html
import json
import re

import data
import md
from common import BLOB, CONTACT, ORIGIN, VERSION, date_of, demo, e, rec
from components import command
from pages.news_content import NEWS
from routes import DOCS, POLICIES, doc_path, has_why

SECURITY_EXPIRES = "2027-11-01T00:00:00.000Z"  # check.py fails when this is in the past or more than a year ahead
AI_CRAWLERS = ["GPTBot", "ClaudeBot", "Google-Extended", "PerplexityBot", "CCBot"]


def abs_url(path: str) -> str:
    return ORIGIN + path


def strapline() -> str:
    text = md.prepare("README.md")
    m = re.search(r"\*\*(.+?)\*\* (.+)", text)
    return m.group(1) + " " + m.group(2)


LLMS_LINKS = [
    ("Docs", [("README", doc_path("readme") + "index.md", "install, quick start, map mode"),
              ("Specification", doc_path("spec") + "index.md", "constraints and roadmap"),
              ("Map mode", doc_path("map-mode") + "index.md", "every map verb"),
              ("Which mode?", doc_path("modalities") + "index.md", "scope and startup cost"),
              ("How it works", "/how-it-works/", "five animations"),
              ("All documents", "/docs/", "with Markdown twins")]),
    ("CLI", [("CLI reference", "/cli/", "help, env, exit codes, MCP tools"),
             ("Languages", "/languages/", "50 languages by tier")]),
    ("Examples", [("Examples", "/examples/", "every command, exact output"),
                  ("Templates", "/templates/", "built-in templates"),
                  ("Playground", "/playground/", "API: /api/v1/openapi.json")]),
    ("Evidence", [("Benchmarks", "/benchmarks/", "tables and caveats"),
                  ("Stability report", doc_path("stability") + "index.md", "33/33 protocol")]),
    ("Project", [("Security", "/security/index.md", "private reports"),
                 ("Contributing", doc_path("contributing") + "index.md", "DCO")]),
    ("Optional", [("Changelog", "/changelog/", "releases"), ("llms-full.txt", "/llms-full.txt", "everything")]),
]


def llms_links() -> list:
    links = [(section, list(items)) for section, items in LLMS_LINKS]
    if has_why():
        links[0][1].insert(0, ("Why dctx", "/why/index.md", "why it exists; why byte-identical output matters"))
    return links


def llms_txt() -> str:
    out = [f"# dctx", "", f"> {strapline()}", "",
           f"dctx {VERSION}: a Rust CLI and MCP server; Apache-2.0; offline, no telemetry. Short: {ORIGIN}/llms-small.txt", ""]
    for section, links in llms_links():
        out.append(f"## {section}")
        out += [f"- [{t}]({abs_url(u)}): {d}" for t, u, d in links]
        out.append("")
    return "\n".join(out)


def readme_section(text: str, heading: str) -> str:
    return data.section(text, heading).strip()


def llms_small() -> str:
    readme = md.prepare("README.md")
    quick = readme_section(readme, "Quick Start")
    quick = quick.split("\n\n`dctx pack` writes")[0]
    mapmode = re.search(r"```zsh\n(.*?)```", readme_section(readme, "Map mode: context for agents"), re.S).group(0)
    r7, stab, langs = data.run7(), data.stability(), data.languages()
    lo, hi, n = r7["cold"]["dctx"]
    net, _ = data.network_zero()
    verbs = []
    for verb in ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]:
        lines = rec(f"help-{verb}")["stdout"].split("\n")
        verbs.append(f"- dctx map {verb}: {lines[1].strip()}")
    _, head, rows = data.md_tables(md.read("docs/CLI-EXIT-CODES.md"))[0]
    exits = "\n".join(f"- {r[0]}: {r[1]}" for r in rows)
    facts = [
        f"- Compare with grep plus file reads; count startup and all model turns, not just tool output: {ORIGIN}/docs/modalities/",
        f"- Cold index {lo:.2f}-{hi:.2f} s on {n} corpora, median of 3 (run 7): {BLOB}{data.RUN7}",
        f"- {langs['total']} languages: {langs['tiers']['full']} full, {langs['tiers']['outline']} outline, {langs['tiers']['docs']} docs: {ORIGIN}/languages/",
        f"- {stab['text']} text and {stab['json']} JSON answers byte-identical after unrelated edits: {BLOB}{data.STAB}",
        f"- {net} socket attempts by dctx in every traced benchmark variant: {BLOB}{data.BENCH}",
    ]
    install = data.readme_block("From source")
    why = []
    if has_why():
        text = md.read("docs/WHY-DCTX.md").strip()
        why = ["## Why dctx", "", text if len(text) < 4000 else text[:text.rfind("\n\n", 0, 4000)] + f"\n\n(continued: {ORIGIN}/why/)", ""]
    return "\n".join([llms_txt(), *why, "## Quick start", "", quick, "", "## Map mode", "", mapmode, "", "## Map verbs", "", *verbs, "",
                      "## Facts", "", *facts, "", "## Install", "", "```zsh", install, "```", "", "## Exit codes", "", exits, "",
                      f"Everything, including every document and every captured example: {ORIGIN}/llms-full.txt", ""])


def full_header(url: str, title: str, source: str) -> str:
    return f"---\nurl: {url}\ntitle: {title}\nsource: {source}\n---\n\n"


def llms_full(docs: dict, linker) -> str:
    parts = [llms_txt(), "\n"]
    for slug, file, title, _, _ in DOCS:
        parts.append(full_header(abs_url(doc_path(slug)), title, BLOB + file) + docs[doc_path(slug)].twin.rstrip("\n") + "\n\n")
    for path, file, title, _ in POLICIES:
        parts.append(full_header(abs_url(path), title, BLOB + file) + docs[path].twin.rstrip("\n") + "\n\n")
    help_ids = ["help", "map-help"] + [f"help-{v}" for v in ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]] + ["languages-help"]
    cli = "".join(f"$ {command(rec(i))}\n{rec(i)['stdout']}\n" for i in help_ids)
    parts.append(full_header(abs_url("/cli/"), "CLI reference (captured help)", BLOB + "tools/website/data/demo.json") + "```text\n" + cli + "```\n\n")
    ex = []
    for r in demo()["records"]:
        if r["group"] == "reference":
            continue
        ex.append(f"### {command(r)}\n\nexit {r['exit']}, {r['bytes']} bytes, ~{r['est_tokens']} est. tokens\n\n```text\n{r['stdout']}```\n"
                  + (f"\nstderr:\n\n```text\n{r['stderr']}```\n" if r["stderr"] else ""))
    parts.append(full_header(abs_url("/examples/"), "Examples (captured on the kiosk sample)", BLOB + "tools/website/data/demo.json") + "\n".join(ex) + "\n")
    for name in ("default", "stamped"):
        file = f"crates/dctx-template/templates/{name}.dctx_template"
        parts.append(full_header(abs_url("/templates/"), f"{name}.dctx_template", BLOB + file) + "```text\n" + md.read(file) + "```\n\n")
    changelog = md._md_links(md.read("CHANGELOG.md"), "CHANGELOG.md", linker)
    parts.append(full_header(abs_url("/changelog/"), "Changelog", BLOB + "CHANGELOG.md") + changelog)
    return "".join(parts)


def atom(title: str, self_path: str, page_path: str, entries: list[tuple[str, str, str, str, str]]) -> str:
    """entries: (title, url, updated ISO date, summary text, html content)."""
    updated = max(x[2] for x in entries)
    items = "".join(
        f"<entry><title>{e(t)}</title><link href=\"{u}\"/><id>{u}</id><updated>{d}T00:00:00Z</updated>"
        f"<summary>{e(s)}</summary>" + (f"<content type=\"html\">{e(c)}</content>" if c else "") + "</entry>\n"
        for t, u, d, s, c in entries)
    return (f'<?xml version="1.0" encoding="utf-8"?>\n<feed xmlns="http://www.w3.org/2005/Atom"><title>{e(title)}</title>'
            f'<link href="{ORIGIN}{page_path}"/><link rel="self" href="{ORIGIN}{self_path}"/><id>{ORIGIN}{page_path}</id>'
            f"<updated>{updated}T00:00:00Z</updated><author><name>Eris Margeta</name><uri>https://tejl.com/</uri></author>\n{items}</feed>\n")


def news_feed() -> str:
    return atom("dctx news", "/news/feed.xml", "/news/",
                [(n["title"], f"{ORIGIN}/news/{n['slug']}/", n["date"], n["summary"], "") for n in NEWS])


def changelog_feed(releases: list[dict]) -> str:
    entries = []
    for r in releases:
        if not r["date"] or not re.match(r"^\d{4}-\d{2}-\d{2}$", r["date"]):
            continue
        summary = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", r["html"]))).strip()[:240]
        entries.append((r["name"], f"{ORIGIN}/changelog/#{r['slug']}", r["date"], summary, r["html"]))
    return atom("dctx releases", "/changelog/feed.xml", "/changelog/", entries)


def sitemap(routes) -> str:
    rows = []
    for r in routes:
        if not r.indexable:
            continue
        lastmod = date_of(r.source) if r.source else (r.extra["post"]["date"] if r.kind == "post" else None)
        rows.append(f"<url><loc>{ORIGIN}{r.path}</loc>" + (f"<lastmod>{lastmod}</lastmod>" if lastmod else "") + "</url>\n")
    return '<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n' + "".join(rows) + "</urlset>\n"


def robots() -> str:
    bots = "".join(f"User-agent: {b}\nAllow: /\n\n" for b in AI_CRAWLERS)
    return f"# dctx is a developer tool: people and AI crawlers are welcome to read it.\nUser-agent: *\nAllow: /\n\n{bots}Sitemap: {ORIGIN}/sitemap.xml\n"


def security_txt() -> str:
    return (f"Contact: mailto:{CONTACT}\nContact: {BLOB}SECURITY.md\nExpires: {SECURITY_EXPIRES}\nPreferred-Languages: en, hr\n"
            f"Canonical: {ORIGIN}/.well-known/security.txt\nPolicy: {ORIGIN}/security/\n")


def humans_txt(updated: str) -> str:
    return f"""/* TEAM */
Founder: Eris Margeta
Organisation: TEJL
Site: https://tejl.com/ and https://tejl.hr/

/* SITE */
Last update: {updated}
Language: English
Standards: HTML, CSS, schema.org JSON-LD, Atom, Open Graph, llms.txt, OpenAPI 3.1
Components: a Python standard-library generator with Markdown and Pygments, Pillow for images; Geist and JetBrains Mono (SIL OFL 1.1); no frameworks, no third-party requests
Source: {BLOB}tools/website
"""


def manifest() -> str:
    return json.dumps({"name": "dctx", "short_name": "dctx", "description": "The context engine for AI-assisted software development.",
                       "start_url": "/", "scope": "/", "display": "browser", "background_color": "#07060f", "theme_color": "#6d3df0",
                       "icons": [{"src": "/assets/icon-192.png", "sizes": "192x192", "type": "image/png"},
                                 {"src": "/assets/icon-512.png", "sizes": "512x512", "type": "image/png"},
                                 {"src": "/favicon.svg", "sizes": "any", "type": "image/svg+xml"}]}, indent=1) + "\n"


def opensearch() -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
<ShortName>dctx</ShortName>
<Description>Search the dctx website</Description>
<InputEncoding>UTF-8</InputEncoding>
<Image width="32" height="32" type="image/png">{ORIGIN}/favicon-32.png</Image>
<Url type="text/html" template="{ORIGIN}/search/?q={{searchTerms}}"/>
</OpenSearchDescription>
"""


def search_index(pages: dict[str, str], routes) -> str:
    out = []
    for r in routes:
        if not r.indexable:
            continue
        page = pages[r.path]
        main = page[page.index('<main id="main"'):page.index("</main>")]
        main = re.sub(r"<script.*?</script>", "", main, flags=re.S)
        heads = [html.unescape(re.sub(r"<[^>]+>", "", h)).replace("#", "").strip() for h in re.findall(r"<h[23][^>]*>(.*?)</h[23]>", main, re.S)]
        text = re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", main))).strip()
        out.append({"url": r.path, "title": r.title, "h": heads[:60], "text": (r.desc or text)[:160]})
    return json.dumps(out, ensure_ascii=False, separators=(",", ":")) + "\n"
