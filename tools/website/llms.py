"""Machine-readable files: llms.txt, llms-small.txt, llms-full.txt, feeds, sitemap, robots, security.txt,
humans.txt, manifest, OpenSearch, the search index and build.json. All derived from the route table.

The three llms files are the entry points of AI evaluators: the facts block (data/facts.json), the route
table, then the dctx binary's own output over the documents (data/llms.json, from `capture.py --llms`)."""
from __future__ import annotations

import hashlib
import html
import json
import re
import sys

import md
from common import BLOB, CONTACT, ORIGIN, PUBLISHER, ROOT, TREE, VERSION, date_of, demo, e, read, rec
from components import command
from pages.news_content import NEWS

SECURITY_EXPIRES = "2027-11-01T00:00:00.000Z"  # check.py fails when this is in the past or more than a year ahead
AI_CRAWLERS = ["GPTBot", "ClaudeBot", "Google-Extended", "PerplexityBot", "CCBot"]


def abs_url(path: str) -> str:
    return ORIGIN + path


LLMS_LIMIT = {"llms-small.txt": 20_480, "llms.txt": 102_400, "llms-full.txt": 1_048_576}
LLMS_SECTIONS = ["Overview", "Docs", "CLI", "Examples", "Evidence", "Project", "Optional"]
HISTORICAL = ("CHANGELOG.md", "docs/research/")  # records of what was true then; removed commands may appear
TIER_LINE = {"llms-small.txt": "the facts and the route table, with an outline of the start documents",
             "llms.txt": "the facts and an index of every public HTML page",
             "llms-full.txt": "the facts, every public HTML page, the document pack and captured CLI help and examples"}

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


def facts_for_file(name: str) -> str:
    text = facts_markdown()
    # llms.txt reserves headings for its H1 and H2 URL lists (llmstxt.org).
    return intro(text) if name == "llms.txt" else text


def intro(text: str) -> str:
    return re.sub(r"^#{2,} (.+)$", r"**\1**", text, flags=re.M)


def route_table(pages: dict[str, str], routes, *, index: bool = False) -> str:
    """The llms.txt index uses the HTML URL and the page's actual meta description."""
    out = [] if index else ["## Pages", ""]
    for section in LLMS_SECTIONS:
        rows = [r for r in routes if (r.llms or "Overview") == section]
        if not rows:
            continue
        out += [f"{'##' if index else '###'} {section}", ""]
        out += [f"- [{r.title}]({abs_url(r.path)}): {extract_page(pages[r.path])['description']}" for r in rows]
        out.append("")
    return "\n".join(out)


def about(name: str) -> str:
    cap, t = llms_capture(), tier(name)
    others = ", ".join(f"{ORIGIN}/{n}" for n in TIER_LINE if n != name)
    cmds = "; ".join("`" + " ".join(c) + "`" for c in t["commands"])
    out = ["## About this file", "",
           f"- Publisher: {PUBLISHER}. TEJL.com: https://tejl.com/; TEJL.hr: https://tejl.hr/.",
           f"- This is {name}: {TIER_LINE[name]}. The other entry points: {others}.",
           f"- Engine: dctx {cap['version']}. Document captures are the unedited output of the dctx binary on a copy of "
           f"the documents, recorded by {BLOB}tools/website/capture.py; the site check re-runs them and fails on any byte of drift. "
           f"Read the pack at {ORIGIN}/llms-full.txt.",
           f"- Docs commit: {cap['docs_commit']} ({cap['docs_date']}), the last commit that changed one of the documents.",
           f"- Command: {cmds}",
           f"- Document capture budget: {t['est_tokens']:,} of {t['budget']:,} estimated tokens "
           f"(ceil(2 * UTF-8 bytes / 7)), {len(t['files'])} documents."]
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


def llms_file(name: str, pages: dict[str, str], routes) -> str:
    provenance = intro(about(name)) if name == "llms.txt" else about(name)
    parts = ["# dctx", "", f"> {strapline()}", "", provenance, facts_for_file(name),
             route_table(pages, routes, index=name == "llms.txt")]
    if name != "llms.txt":
        parts.append(documents(name))
    return "\n".join(parts)


def llms_txt(pages: dict[str, str], routes) -> str:
    return llms_file("llms.txt", pages, routes)


def llms_small(pages: dict[str, str], routes) -> str:
    return llms_file("llms-small.txt", pages, routes)


def full_header(url: str, title: str, source: str) -> str:
    return f"---\nurl: {url}\ntitle: {title}\nsource: {source}\n---\n\n"


def captured_listings() -> list[str]:
    help_ids = ["help", "map-help"] + [f"help-{v}" for v in ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]] + ["languages-help"]
    cli = "".join(f"$ {command(rec(i))}\n{rec(i)['stdout']}\n" for i in help_ids)
    parts = ["\n## CLI reference and examples (captured from the binary)\n\n" +
             full_header(abs_url("/cli/"), "CLI reference (captured help)", BLOB + "tools/website/data/demo.json") + "```text\n" + cli + "```\n\n"]
    ex = []
    for r in demo()["records"]:
        if r["group"] == "reference":
            continue
        ex.append(f"### {command(r)}\n\nexit {r['exit']}, {r['bytes']} bytes, ~{r['est_tokens']} est. tokens\n\n```text\n{r['stdout']}```\n"
                  + (f"\nstderr:\n\n```text\n{r['stderr']}```\n" if r["stderr"] else ""))
    parts.append(full_header(abs_url("/examples/"), "Examples (captured on the kiosk sample)", BLOB + "tools/website/data/demo.json") + "\n".join(ex) + "\n")
    return parts


def page_texts(pages: dict[str, str], routes, packed: str) -> str:
    """Keep route order. Link packed documents and add only text absent from the pack.

    Markdown markup and generated page furniture can change the rendered text, so those
    lines still need to be included. Prose already present verbatim is not copied again.
    """
    packed_text = normalize(packed)
    parts = ["\n## Public HTML pages\n\n"]
    for r in routes:
        extracted = extract_page(pages[r.path])
        parts.append(full_header(abs_url(r.path), r.title, BLOB + r.source if r.source else abs_url(r.path)))
        lines = extracted["lines"]
        if r.source and r.source in tier("llms-full.txt")["files"]:
            parts.append(f"Document: `{r.source}` in [Documents](#documents). [Read the page]({abs_url(r.path)}).\n\n")
            lines = [line for line in lines if normalize(line) not in packed_text]
            if lines:
                parts.append("Rendered text and page details not already verbatim in the document pack:\n\n")
        parts.append("\n".join(lines) + "\n\n")
    return "".join(parts)


def llms_full(pages: dict[str, str], routes) -> str:
    base = llms_file("llms-full.txt", pages, routes)
    base += page_texts(pages, routes, tier("llms-full.txt")["output"])
    listings = captured_listings()
    full = base + "".join(listings)
    limit = LLMS_LIMIT["llms-full.txt"]
    # Only these supplemental generated listings can be shortened; page prose and
    # the document pack are mandatory even when they exceed the limit on their own.
    for i in sorted(range(len(listings)), key=lambda i: len(listings[i].encode()), reverse=True):
        if len(full.encode()) <= limit:
            break
        url, title = (("/cli/", "CLI help"), ("/examples/", "Command examples"))[i]
        before = len(listings[i].encode())
        listings[i] = (f"\n{title}: the generated listing is available on [the full page]({abs_url(url)}); "
                       "its page text is included above. Commands, outputs and exit codes come from the captured binary runs.\n")
        full = base + "".join(listings)
        print(f"llms-full.txt: {len(base.encode()):,} mandatory bytes; summarized {title} "
              f"({before:,} -> {len(listings[i].encode()):,} bytes); total {len(full.encode()):,}, limit {limit:,}", file=sys.stderr)
    if len(full.encode()) > limit:
        raise ValueError(f"llms-full.txt: {len(full.encode()):,} bytes > {limit:,}; "
                         f"mandatory page text and document pack: {len(base.encode()):,} bytes")
    return full


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
        url = f"{ORIGIN}/changelog/#{r['slug']}"
        summary = f"Release notes for {r['name']}. Read the changelog for details."
        entries.append((r["name"], url, r["date"], summary, f'<p><a href="{url}">Read the release notes</a>.</p>'))
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
Organisation: {PUBLISHER}
Site: https://tejl.com/ and https://tejl.hr/

/* SITE */
Last update: {updated}
Language: English
Standards: HTML, CSS, schema.org JSON-LD, Atom, Open Graph, llms.txt, OpenAPI 3.1
Components: a Python standard-library generator with Markdown, an in-house build-time syntax highlighter and Pillow for images; Geist and JetBrains Mono (SIL OFL 1.1); no frameworks, no third-party requests
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


def normalize(text: str) -> str:
    from glyphs import plain
    return re.sub(r"\s+", " ", plain(text)).strip()


def extract_page(page: str) -> dict:
    """Shared search/llms extraction: visible main text, headings and meta description.

    Preserve source line boundaries for verbatim pack deduplication and the freshness
    gate; inline tags and whitespace are handled exactly as in the search index.
    """
    main = page[page.index('<main id="main"'):page.index("</main>")]
    # Historical commands belong on the changelog, not in current discovery text.
    main = re.sub(r"<!-- release-history:start -->.*?<!-- release-history:end -->", "", main, flags=re.S)
    main = re.sub(r'<span class="glyph-wrap" role="img" aria-label="([^"]*)">', lambda m: html.unescape(m[1]), main)
    main = re.sub(r"<(script|style|svg|nav|footer|header)\b.*?</\1>", "", main, flags=re.S)
    main = re.sub(r"<!--.*?-->", "", main, flags=re.S)
    heads = [html.unescape(re.sub(r"<[^>]+>", "", h)).replace("#", "").strip()
             for h in re.findall(r"<h[23][^>]*>(.*?)</h[23]>", main, re.S)]
    plain = html.unescape(re.sub(r"<[^>]+>", " ", main))
    lines = [normalize(line) for line in plain.splitlines() if normalize(line)]
    description = re.search(r'<meta name="description" content="([^"]*)"', page)
    return {"text": normalize(plain), "lines": lines, "headings": heads,
            "description": normalize(html.unescape(description.group(1))) if description else ""}


def search_index(pages: dict[str, str], routes) -> str:
    out = []
    for r in routes:
        if not r.indexable:
            continue
        extracted = extract_page(pages[r.path])
        out.append({"url": r.path, "title": r.title, "h": extracted["headings"][:60],
                    "text": (extracted["description"] or extracted["text"])[:160]})
    return json.dumps(out, ensure_ascii=False, separators=(",", ":")) + "\n"
