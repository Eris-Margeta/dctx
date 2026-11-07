"""Documentation: the index and every rendered repository document (and the three policies)."""
from __future__ import annotations

from common import BLOB, REPO, ORG_ID, ORIGIN, PERSON_ID, APP_ID, SITE_ID, e, date_of, human_date, iso_of
from components import table
from routes import DOC_GROUPS, DOCS, doc_path


def sidebar(current: str) -> str:
    groups = []
    for group in DOC_GROUPS:
        items = "".join(f'<li><a href="{doc_path(s)}"{" aria-current=\"page\"" if doc_path(s) == current else ""}>{e(t)}</a></li>'
                        for s, _, t, g, _ in DOCS if g == group)
        groups.append(f'<p class="side-h">{e(group)}</p><ul>{items}</ul>')
    policies = ('<p class="side-h">Policies</p><ul>' + "".join(
        f'<li><a href="{p}"{" aria-current=\"page\"" if p == current else ""}>{t}</a></li>'
        for p, t in (("/security/", "Security"), ("/trademark/", "Trademark"), ("/maintenance/", "Maintenance"))) + "</ul>")
    return f'<aside class="sidebar" aria-label="Documentation"><p class="side-h"><a href="/docs/">All documents</a></p>{"".join(groups)}{policies}</aside>'


def toc_html(toc: list[tuple[int, str, str]]) -> tuple[str, str]:
    if not toc:
        return "", ""
    items = "".join(f'<li class="l{lvl}"><a href="#{i}">{e(t)}</a></li>' for lvl, i, t in toc)
    side = f'<nav class="toc" aria-label="On this page"><p class="side-h">On this page</p><ul>{items}</ul></nav>'
    mobile = f'<details class="toc-m"><summary>On this page</summary><ul>{"".join(f"<li><a href=\"#{i}\">{e(t)}</a></li>" for lvl, i, t in toc if lvl == 2)}</ul></details>'
    return side, mobile


def doc_page(route, doc, prev, nxt) -> tuple[str, list, tuple]:
    updated = date_of(doc.rel)
    md_url = route.path + "index.md"
    meta = [f'<a href="{BLOB}{doc.rel}">View source</a>', f'<a href="{REPO}/edit/main/{doc.rel}">Edit on GitHub</a>',
            f'<a href="{md_url}" type="text/markdown">Markdown</a>', f"<code>{e(doc.rel)}</code>",
            f"{doc.words:,} words"]
    if updated:
        meta.append(f'updated <time datetime="{updated}">{human_date(updated)}</time>')
    notice = ""
    html = doc.html
    end_h1 = html.find("</h1>")
    meta_html = f'<p class="doc-meta">{" &middot; ".join(meta)}</p>'
    html = html[:end_h1 + 5] + meta_html + notice + "{{tocm}}" + html[end_h1 + 5:] if end_h1 >= 0 else meta_html + html
    side_toc, mobile_toc = toc_html(doc.toc)
    html = html.replace("{{tocm}}", mobile_toc)
    pager = ""
    if prev or nxt:
        pager = '<nav class="pager" aria-label="Previous and next">' + (
            f'<a class="prev" href="{prev.path}" rel="prev"><span>Previous</span>{e(prev.title)}</a>' if prev else "") + (
            f'<a class="next" href="{nxt.path}" rel="next"><span>Next</span>{e(nxt.title)}</a>' if nxt else "") + "</nav>"
    body = (f'<div class="wrap docs-grid">{sidebar(route.path)}<article class="prose">{{{{crumbs}}}}{html}{pager}</article>'
            f"{side_toc}</div>")
    url = ORIGIN + route.path
    article = {"@type": "TechArticle", "@id": url + "#article", "headline": doc.title or route.title, "description": route.desc,
               "url": url, "inLanguage": "en", "about": {"@id": APP_ID}, "author": {"@id": PERSON_ID}, "publisher": {"@id": ORG_ID},
               "isPartOf": {"@id": SITE_ID}, "mainEntityOfPage": {"@id": url + "#webpage"},
               "wordCount": doc.words, "encoding": {"@type": "MediaObject", "contentUrl": url + "index.md", "encodingFormat": "text/markdown"}}
    if updated:
        article["dateModified"] = iso_of(doc.rel)
    head = f'<link rel="alternate" type="text/markdown" href="{md_url}" title="{e(route.title)} as Markdown">\n'
    if prev:
        head += f'<link rel="prev" href="{prev.path}">\n'
    if nxt:
        head += f'<link rel="next" href="{nxt.path}">\n'
    return body, [article], (head, iso_of(doc.rel))


def index_page(docs_by_path: dict) -> str:
    parts = []
    for group in DOC_GROUPS:
        rows = []
        for slug, file, title, g, line in DOCS:
            if g != group:
                continue
            doc = docs_by_path[doc_path(slug)]
            updated = date_of(file) or ""
            rows.append([f'<a href="{doc_path(slug)}">{e(title)}</a>', e(line), f"{doc.words:,}",
                         f'<time datetime="{updated}">{updated}</time>' if updated else "",
                         f'<a class="src" href="{BLOB}{file}">{e(file)}</a>'])
        parts.append(f'<h2 id="{group.lower().replace(" ", "-")}">{e(group)}</h2>'
                     + table(["Document", "What it covers", "Words", "Updated", "Source"], rows, raw=True, numeric={2}, cls="doc-index"))
    rows = [[f'<a href="{p}">{t}</a>', e(d), "", "", f'<a class="src" href="{BLOB}{f}">{f}</a>'] for p, f, t, d in (
        ("/security/", "SECURITY.md", "Security policy", "How to report a vulnerability privately."),
        ("/trademark/", "TRADEMARK.md", "Trademark policy", "Use of the dctx name and logo."),
        ("/maintenance/", "MAINTENANCE.md", "Maintenance", "What a maintenance agreement covers."))]
    parts.append('<h2 id="policies">Policies</h2>' + table(["Document", "What it covers", "", "", "Source"], rows, raw=True))
    return f"""<div class="wrap"><div class="page-head">{{{{crumbs}}}}<span class="eyebrow">Documentation</span><h1>Documentation</h1>
<p class="lede">Every document is rendered from the repository at build time, with a Markdown twin next to it (<code>index.md</code>), and all of them are in <a href="/llms-full.txt">llms-full.txt</a>. The CLI reference is generated from the binary: <a href="/cli/">/cli/</a>.</p></div>
{"".join(parts)}
<p class="note">Crawlers, including AI crawlers, may read this site: see <a href="/robots.txt">robots.txt</a> and <a href="/llms.txt">llms.txt</a>.</p></div>"""
