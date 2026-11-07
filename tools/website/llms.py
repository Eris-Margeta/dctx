"""Machine-readable files: llms.txt, llms-small.txt, llms-full.txt, feeds, sitemap, robots, security.txt,
humans.txt, manifest, OpenSearch, the search index and build.json. All derived from the route table.

The three llms files are the entry points of AI evaluators: the facts block (data/facts.json), the route
table, then the dctx binary's own output over the documents (data/llms.json, from `capture.py --llms`)."""
from __future__ import annotations

import hashlib
import html
import json
import re

import md
from common import BLOB, CONTACT, ORIGIN, ROOT, TREE, VERSION, date_of, demo, e, read, rec
from components import command
from pages.news_content import NEWS

SECURITY_EXPIRES = "2027-11-01T00:00:00.000Z"  # check.py fails when this is in the past or more than a year ahead
AI_CRAWLERS = ["GPTBot", "ClaudeBot", "Google-Extended", "PerplexityBot", "CCBot"]


def abs_url(path: str) -> str:
    return ORIGIN + path


def strapline() -> str:
    text = md.prepare("README.md")
    m = re.search(r"\*\*(.+?)\*\* (.+)", text)
    return m.group(1) + " " + m.group(2)


LLMS_SECTIONS = ["Docs", "CLI", "Examples", "Evidence", "Project", "Optional"]
HISTORICAL = ("CHANGELOG.md", "docs/research/")  # records of what was true then; removed commands may appear
TIER_LINE = {"llms-small.txt": "the facts and the route table, with an outline of the start documents",
             "llms.txt": "the facts, the route table and the start documents",
             "llms-full.txt": "the facts, the route table, every document the budget allows, the captured CLI help and examples"}

_facts: dict | None = None
_capture: dict | None = None


def facts() -> dict:
    """data/facts.json: the one facts source of the llms files and /for-ai-evaluators/ (schema in README.md)."""
    global _facts
    if _facts is None:
        _facts = json.loads(read("tools/website/data/facts.json"))
        if _facts["version"] != VERSION:
            raise SystemExit(f"facts.json is for dctx {_facts['version']}, VERSION is {VERSION}")
    return _facts


def llms_capture() -> dict:
    """data/llms.json: the dctx output behind the llms files, recorded by `capture.py --llms`."""
    global _capture
    if _capture is None:
        _capture = json.loads(read("tools/website/data/llms.json"))
        if _capture["version"] != VERSION:
            raise SystemExit(f"llms.json was captured from dctx {_capture['version']}, VERSION is {VERSION}; run capture.py --llms")
    return _capture


def stale_sources() -> list[str]:
    """Documents whose bytes differ from the ones the llms capture was made from."""
    cap = llms_capture()
    stale = []
    for rel, digest in cap["sources"].items():
        path = ROOT / rel
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            stale.append(rel)
    return stale


def tier(name: str) -> dict:
    return next(t for t in llms_capture()["tiers"] if t["file"] == name)


def link(url: str) -> str:
    return abs_url(url) if url.startswith("/") else url


def strapline() -> str:
    text = md.prepare("README.md")
    m = re.search(r"\*\*(.+?)\*\* (.+)", text)
    return m.group(1) + " " + m.group(2)


def facts_markdown() -> str:
    f = facts()
    v, b, r = f["verify_download"], f["benchmarks"], f["removed"]
    out = ["## Facts", "", "### What dctx is", "", f["is"], "", "What it is not:", ""]
    out += [f"- {x}" for x in f["is_not"]]
    out += ["", "### Version and download", "",
            f"- Current version: {f['version']}, released {f['released']} (tag `{f['release_tag']}`): {v['link']}",
            f"- {v['available']}", f"- Verify a download: `{v['command']}`", f"- {v['planned']}",
            "", "### Modalities", ""]
    out += [f"- {m['name']} ({m['status']}): {m['line']} {link(m['link'])}" for m in f["modalities"]]
    out += ["", "Two scopes, one binary:", ""] + [f"- {x}" for x in f["scopes"]]
    out += ["", "### Before you start", "", f"> {f['notice']}", "", "### Benchmarks", "", f"Baseline: {b['baseline']}.", ""]
    out += [f"- {c['status'].capitalize()}: {c['text']} {link(c['link'])}" for c in b["claims"]]
    out += ["", f"### Removed in {r['since']}", "", r["intro"], ""]
    out += [f"- {x['was']}: use {x['use']}." for x in r["items"]]
    out += ["", "### How to verify a claim", ""]
    out += [f"- {x['what']}: " + (f"`{x['command']}`" if "command" in x else x["text"]) + f" {link(x['link'])}" for x in f["verify"]]
    return "\n".join(out) + "\n"


def route_table() -> str:
    from routes import build_routes
    routes = [r for r in build_routes(NEWS) if r.indexable and r.llms]
    out = ["## Pages", "", "Documents link to their Markdown twin (index.md), the repository file with only link targets rewritten.", ""]
    for section in LLMS_SECTIONS:
        rows = [r for r in routes if r.llms == section]
        if not rows:
            continue
        out += [f"### {section}", ""]
        out += [f"- [{r.title}]({abs_url(r.path + ('index.md' if r.kind == 'doc' else ''))}): {r.desc}" for r in rows]
        out.append("")
    return "\n".join(out)


def about(name: str) -> str:
    cap, t = llms_capture(), tier(name)
    others = ", ".join(f"{ORIGIN}/{n}" for n in TIER_LINE if n != name)
    cmds = "; ".join("`" + " ".join(c) + "`" for c in t["commands"])
    out = ["## About this file", "",
           f"- This is {name}: {TIER_LINE[name]}. The other entry points: {others}.",
           f"- Engine: dctx {cap['version']}. The documents section is the unedited output of the dctx binary on a copy of "
           f"the documents, recorded by {BLOB}tools/website/capture.py; the site check re-runs it and fails on any byte of drift.",
           f"- Docs commit: {cap['docs_commit']} ({cap['docs_date']}), the last commit that changed one of the documents.",
           f"- Command: {cmds}",
           f"- Budget: {t['est_tokens']:,} of {t['budget']:,} estimated tokens (ceil(2 * UTF-8 bytes / 7)), {len(t['files'])} documents."]
    if t.get("altered"):
        out.append("- Known engine issue: dctx " + cap["version"] + " pack substitutes template variables written as `{{name}}` inside file "
                   "content, so these documents differ from the repository here: " + ", ".join(t["altered"]) + f". Read them at {TREE}.")
    if t["dropped"]:
        out.append("- Left out to stay within the budget: " + ", ".join(t["dropped"]) + f". Everything is in {ORIGIN}/llms-full.txt or at {TREE}.")
    return "\n".join(out) + "\n"


def documents(name: str) -> str:
    t = tier(name)
    if t["mode"] == "outline":
        return ("## Outline of the start documents\n\n`dctx map map FILE --depth 2`: one heading per line with its dctx id and "
                "line range in the file at the docs commit.\n\n```text\n" + t["output"] + "```\n")
    return "## Documents\n\n" + t["output"]


def llms_file(name: str, tail: str = "") -> str:
    parts = ["# dctx", "", f"> {strapline()}", "", about(name), facts_markdown(), route_table(), documents(name)]
    return "\n".join(parts) + tail


def llms_txt() -> str:
    return llms_file("llms.txt")


def llms_small() -> str:
    return llms_file("llms-small.txt")


def full_header(url: str, title: str, source: str) -> str:
    return f"---\nurl: {url}\ntitle: {title}\nsource: {source}\n---\n\n"


def llms_full() -> str:
    help_ids = ["help", "map-help"] + [f"help-{v}" for v in ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]] + ["languages-help"]
    cli = "".join(f"$ {command(rec(i))}\n{rec(i)['stdout']}\n" for i in help_ids)
    parts = ["\n## CLI reference and examples (captured from the binary)\n\n",
             full_header(abs_url("/cli/"), "CLI reference (captured help)", BLOB + "tools/website/data/demo.json") + "```text\n" + cli + "```\n\n"]
    ex = []
    for r in demo()["records"]:
        if r["group"] == "reference":
            continue
        ex.append(f"### {command(r)}\n\nexit {r['exit']}, {r['bytes']} bytes, ~{r['est_tokens']} est. tokens\n\n```text\n{r['stdout']}```\n"
                  + (f"\nstderr:\n\n```text\n{r['stderr']}```\n" if r["stderr"] else ""))
    parts.append(full_header(abs_url("/examples/"), "Examples (captured on the kiosk sample)", BLOB + "tools/website/data/demo.json") + "\n".join(ex) + "\n")
    return llms_file("llms-full.txt", "".join(parts))


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
