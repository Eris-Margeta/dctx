"""Changelog (from CHANGELOG.md), news, search and the error pages."""
from __future__ import annotations

import re

from common import BLOB, ORG_ID, ORIGIN, PERSON_ID, VERSION, e, human_date, read
from pages.news_content import NEWS


def inline(s: str, linker) -> str:
    s = e(s, quote=False)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)

    def link(m):
        target = linker.resolve("CHANGELOG.md", m.group(2))
        return f'<a href="{e(target)}">{m.group(1)}</a>' if target else m.group(1)
    return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", link, s)


def releases(linker) -> list[dict]:
    text = read("CHANGELOG.md")
    refs = dict(re.findall(r"^\[([^\]]+)\]:\s*(\S+)$", text, re.M))
    out = []
    for blk in re.split(r"^## ", text, flags=re.M)[1:]:
        head, _, rest = blk.partition("\n")
        m = re.match(r"(?:(dircontxt) )?\[([^\]]+)\](?:\s*[—-]\s*(\S+))?", head)
        if not m:
            continue
        prefix, ver, date = m.group(1), m.group(2), m.group(3)
        name = f"{prefix} {ver}" if prefix else ver
        slug = "v" + re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")
        html, para, items = [], [], []

        def flush():
            if para:
                html.append("<p>" + inline(" ".join(para), linker) + "</p>")
                para.clear()
            if items:
                html.append("<ul>" + "".join(f"<li>{inline(i, linker)}</li>" for i in items) + "</ul>")
                items.clear()
        for line in rest.split("\n"):
            if re.match(r"^\[[^\]]+\]:", line):
                continue
            if line.startswith("### "):
                flush()
                html.append(f"<h3>{inline(line[4:], linker)}</h3>")
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
        out.append({"name": name, "slug": slug, "date": date, "html": "".join(html), "ver": ver, "prefix": prefix,
                    "compare": refs.get(ver) if not prefix else None, "text": rest})
    return out


def changelog(linker) -> str:
    parts, toc = [], []
    for r in releases(linker):
        badge = '<span class="tag">In development</span>' if r["ver"] == "Unreleased" else ""
        when = f'<small><time datetime="{r["date"]}">{r["date"]}</time></small>' if r["date"] else ""
        compare = f'<p class="note"><a href="{r["compare"]}">Compare in the repository</a></p>' if r["compare"] else ""
        parts.append(f'<section class="release" id="{r["slug"]}" aria-labelledby="{r["slug"]}-h"><h2 id="{r["slug"]}-h">{e(r["name"])} {when} {badge}</h2>'
                     f'{r["html"]}{compare}</section>')
        toc.append(f'<li><a href="#{r["slug"]}">{e(r["name"])}</a></li>')
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Changelog</span><h1>Changelog</h1>
<p class="lede">Every release, including known issues we have measured and not yet fixed. Generated from <a href="{BLOB}CHANGELOG.md">CHANGELOG.md</a>; also an <a href="/changelog/feed.xml">Atom feed</a>.</p>
<nav aria-label="Releases"><ul class="toc-row">{"".join(toc)}</ul></nav></div>
<div class="prose">{"".join(parts)}</div></div>"""


def news_index() -> str:
    items = "".join(
        f'<li><p class="meta"><time datetime="{n["date"]}">{human_date(n["date"])}</time><span class="tag">{n["kind"]}</span></p>'
        f'<h2><a href="/news/{n["slug"]}/">{e(n["title"])}</a></h2><p>{e(n["summary"])}</p></li>' for n in NEWS)
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">News</span><h1>News</h1>
<p class="lede">Releases, measurements and product decisions. Also an <a href="/news/feed.xml">Atom feed</a>.</p></div>
<ul class="post-list">{items}</ul></div>"""


def news_post(n: dict) -> tuple[str, list]:
    path = f"/news/{n['slug']}/"
    art = {"@type": "BlogPosting", "@id": ORIGIN + path + "#article", "headline": n["title"], "description": n["summary"],
           "datePublished": n["date"], "inLanguage": "en", "mainEntityOfPage": {"@id": ORIGIN + path + "#webpage"},
           "author": {"@id": PERSON_ID}, "publisher": {"@id": ORG_ID}, "image": ORIGIN + f"/assets/og/news-{n['slug']}.dark.png"}
    body = f"""<div class="wrap"><article class="prose">{{{{crumbs}}}}<span class="eyebrow">{n['kind']}</span>
<h1>{e(n['title'])}</h1><p class="lede">{e(n['summary'])}</p>
<p class="meta"><time datetime="{n['date']}">{human_date(n['date'])}</time><span>The dctx project</span></p>
{n['body']}<p><a href="/news/">&larr; All news</a></p></article></div>"""
    return body, [art]


def search(index_url: str, routes) -> str:
    pages = "".join(f'<li><a href="{r.path}">{e(r.title)}</a><div class="u">{r.path}</div><p>{e(r.desc)}</p></li>'
                    for r in routes if r.indexable)
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Search</span><h1>Search</h1>
<p class="lede">Titles, headings and text of every page, searched in your browser. Nothing is sent anywhere.</p></div>
<form id="search-form" role="search" action="/search/" method="get" data-index="{index_url}">
<div class="field"><label for="q">Search the site</label><input type="search" id="q" name="q" autocomplete="off" spellcheck="false"></div></form>
<p class="status" id="search-note" role="status" aria-live="polite"></p>
<ul class="results" id="search-results"></ul>
<div class="no-js-only"><p>Search needs JavaScript. Every page of the site:</p><ul class="results">{pages}</ul></div></div>"""


def not_found(index_url: str) -> str:
    return f"""<div class="wrap notfound"><span class="eyebrow">404</span><h1>Page not found.</h1>
<p class="lede">There is no page at this address. It may have moved.</p>
<form id="search-form" role="search" action="/search/" method="get" data-index="{index_url}">
<div class="field"><label for="q">Search the site</label><input type="search" id="q" name="q" autocomplete="off"></div></form>
<p class="status" id="search-note" role="status" aria-live="polite"></p><ul class="results" id="search-results"></ul>
<div class="actions"><a class="btn btn-primary" href="/">Home</a><a class="btn" href="/docs/">Documentation</a><a class="btn" href="/cli/">CLI reference</a><a class="btn" href="/examples/">Examples</a></div></div>"""
