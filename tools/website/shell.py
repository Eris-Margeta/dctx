"""The shared page shell: head (meta, OG, links, JSON-LD), header, breadcrumbs and footer."""
from __future__ import annotations

import json

from common import (APP_ID, BLOB, CONTACT, ICON, LICENSE_URL, ORG_ID, ORIGIN, PERSON_ID, REPO, SITE_ID, VERSION, e)
from routes import NAV, Route


def ld(graph: list) -> str:
    return json.dumps({"@context": "https://schema.org", "@graph": graph}, ensure_ascii=False, indent=1).replace("<", "\\u003c")


def base_graph() -> list:
    """The binding entities, identical on every page."""
    return [
        {"@type": "Organization", "@id": ORG_ID, "name": "TEJL", "url": "https://tejl.com/",
         "logo": ORIGIN + "/assets/tejl-logo-off-black.svg", "email": CONTACT, "sameAs": ["https://tejl.hr/"],
         "founder": {"@id": PERSON_ID}},
        {"@type": "Person", "@id": PERSON_ID, "name": "Eris Margeta", "url": "https://tejl.com/",
         "sameAs": ["https://github.com/Eris-Margeta"], "jobTitle": "Founder", "worksFor": {"@id": ORG_ID}},
        {"@type": "WebSite", "@id": SITE_ID, "url": ORIGIN + "/", "name": "dctx", "inLanguage": "en",
         "publisher": {"@id": ORG_ID},
         "potentialAction": {"@type": "SearchAction", "target": ORIGIN + "/search/?q={search_term_string}",
                             "query-input": "required name=search_term_string"}},
        {"@type": "SoftwareApplication", "@id": APP_ID, "name": "dctx", "softwareVersion": VERSION,
         "applicationCategory": "DeveloperApplication", "operatingSystem": "Linux (x86_64 tested)",
         "description": "The context engine for AI-assisted software development: deterministic, bounded, offline code context.",
         "url": ORIGIN + "/", "license": LICENSE_URL, "isAccessibleForFree": True, "codeRepository": REPO,
         "programmingLanguage": "Rust", "releaseNotes": ORIGIN + "/changelog/#v" + VERSION.replace(".", "-"),
         "softwareHelp": {"@type": "CreativeWork", "url": ORIGIN + "/docs/"},
         "author": {"@id": PERSON_ID}, "publisher": {"@id": ORG_ID}},
    ]


def crumbs_of(route: Route, parents: list[tuple[str, str]]) -> list[tuple[str, str]]:
    return [("/", "dctx")] + parents + [(route.path, route.crumb or route.title)]


def nav_links(section: str | None) -> str:
    items = [f'<li><a href="{h}"{" aria-current=\"page\"" if s == section else ""}>{t}</a></li>' for h, t, s in NAV]
    items.append(f'<li><a href="{REPO}">GitHub</a></li>')
    return "".join(items)


def footer() -> str:
    col = lambda hid, title, links: (f'<nav aria-labelledby="{hid}"><h2 id="{hid}">{title}</h2><ul>'  # noqa: E731
                                     + "".join(f'<li><a href="{h}">{t}</a></li>' for h, t in links) + "</ul></nav>")
    return f"""<footer class="ftr">
 <div class="wrap">
  <div class="ftr-grid">
   <div class="ftr-brand">
    <a class="brand" href="/">{ICON}<span>dctx</span></a>
    <p>The context engine for AI-assisted software development. Fewer tokens, stable prefixes, exact source.</p>
    <p class="mono small">dctx {VERSION} &middot; Apache-2.0 &middot; Linux x86_64 tested</p>
   </div>
   {col("f-product", "Product", [("/docs/", "Documentation"), ("/cli/", "CLI reference"), ("/examples/", "Examples"), ("/templates/", "Templates"), ("/playground/", "Playground"), ("/languages/", "Languages"), ("/benchmarks/", "Benchmarks")])}
   {col("f-res", "Resources", [("/docs/spec/", "Specification"), ("/changelog/", "Changelog"), ("/news/", "News"), ("/news/feed.xml", "News feed (Atom)"), ("/changelog/feed.xml", "Release feed (Atom)"), ("/llms.txt", "llms.txt"), ("/llms-full.txt", "llms-full.txt"), ("/search/", "Search")])}
   {col("f-proj", "Project", [(REPO, "Source code"), (REPO + "/issues", "Issues"), ("/security/", "Security policy"), ("/.well-known/security.txt", "security.txt"), ("/maintenance/", "Maintenance"), ("/trademark/", "Trademark"), (BLOB + "LICENSE", "License (Apache-2.0)"), (f"mailto:{CONTACT}?subject=dctx", "Contact")])}
  </div>
  <div class="ftr-bottom">
   <div>
    <p>&copy; 2025&ndash;2026 <a href="https://tejl.com/">Eris Margeta</a>. dctx is open source under Apache-2.0.</p>
    <p>&ldquo;dctx&rdquo; and its logo are trademarks of Eris Margeta (<a href="/trademark/">trademark policy</a>). This site uses no cookies, analytics or third-party requests.</p>
   </div>
   <div class="credit">
    <p>Published by TEJL<br><a href="https://tejl.com/">tejl.com</a> &middot; <a href="https://tejl.hr/">tejl.hr</a></p>
    <a class="credit-logo" href="https://tejl.com/" aria-label="TEJL, publisher of dctx"><img src="/assets/tejl-logo-off-black.svg" width="818" height="422" alt="" loading="lazy" decoding="async"></a>
   </div>
  </div>
 </div>
</footer>"""


def og_tags(route: Route, assets) -> tuple[str, str]:
    """OG image meta (GIF first on the home page, then dark, then light) and the twitter image."""
    slug = route.slug if route.kind != "error" else "home"
    alt = f"dctx: {route.title}" if route.kind != "home" else "dctx: a repository flows through ignore rules into a graph, then into one bounded, stable page."
    images = []
    if route.kind == "home":
        images.append(("/assets/og/home.gif", "image/gif"))
    images += [(f"/assets/og/{slug}.dark.png", "image/png"), (f"/assets/og/{slug}.light.png", "image/png")]
    tags = "".join(
        f'<meta property="og:image" content="{ORIGIN}{u}">\n<meta property="og:image:type" content="{t}">\n'
        f'<meta property="og:image:width" content="1200">\n<meta property="og:image:height" content="630">\n'
        f'<meta property="og:image:alt" content="{e(alt)}">\n' for u, t in images)
    twitter = (f'<meta name="twitter:image" content="{ORIGIN}/assets/og/{slug}.dark.png">\n'
               f'<meta name="twitter:image:alt" content="{e(alt)}">\n')
    for url, _ in images:
        assets.require(url)
    return tags, twitter


def page(route: Route, body: str, assets, *, graph: list | None = None, parents=None, script: str = "page.js", head_extra: str = "",
         og_type: str = "website", main_class: str = "", article_times: tuple[str | None, str | None] = (None, None),
         webpage_extra: dict | None = None) -> str:
    url = ORIGIN + route.path.replace("index.html", "")
    g = base_graph()
    webpage = {"@type": "WebPage", "@id": url + "#webpage", "url": url, "name": route.title, "description": route.desc,
               "inLanguage": "en", "isPartOf": {"@id": SITE_ID}}
    if route.kind == "home":
        webpage["about"] = {"@id": APP_ID}
    crumbs = None
    if route.kind != "home":
        crumbs = crumbs_of(route, parents or [])
        webpage["breadcrumb"] = {"@id": url + "#breadcrumb"}
        g.append({"@type": "BreadcrumbList", "@id": url + "#breadcrumb", "itemListElement": [
            {"@type": "ListItem", "position": i + 1, "name": n, "item": ORIGIN + p} for i, (p, n) in enumerate(crumbs)]})
    webpage.update(webpage_extra or {})
    g.append(webpage)
    g += graph or []
    title = "dctx: the context engine for AI-assisted development" if route.kind == "home" else f"{route.title} · dctx"
    robots = "noindex, follow" if not route.indexable else "index, follow, max-image-preview:large"
    og, twitter = og_tags(route, assets)
    published, modified = article_times
    times = ""
    if published:
        times += f'<meta property="article:published_time" content="{published}">\n'
    if modified:
        times += f'<meta property="article:modified_time" content="{modified}">\n'
    crumb_html = ""
    if crumbs:
        crumb_html = ('<nav class="crumbs" aria-label="Breadcrumb"><ol>' + "".join(
            f'<li><a href="{p}"{" aria-current=\"page\"" if i == len(crumbs) - 1 else ""}>{e(n)}</a></li>'
            for i, (p, n) in enumerate(crumbs)) + "</ol></nav>")
    body = body.replace("{{crumbs}}", crumb_html)
    me = '<link rel="me" href="https://github.com/Eris-Margeta">\n' if route.kind == "home" else ""
    script_tags = f'<script src="{assets.url(script)}" defer></script>\n'
    nav = nav_links(route.section)
    return f"""<!doctype html>
<html lang="en" class="no-js">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{e(title)}</title>
<script>{assets.theme_inline}</script>
<link rel="stylesheet" href="{assets.url("site.css")}">
<link rel="preload" href="{assets.url("geist-400.woff2")}" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="{assets.url("geist-600.woff2")}" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="{assets.url("jetbrains-mono-400.woff2")}" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="{assets.url("jetbrains-mono-500.woff2")}" as="font" type="font/woff2" crossorigin>
<meta name="description" content="{e(route.desc)}">
<meta name="robots" content="{robots}">
<meta name="generator" content="dctx website builder">
<meta name="color-scheme" content="light dark">
<meta name="theme-color" content="#fbfaff" media="(prefers-color-scheme: light)">
<meta name="theme-color" content="#07060f" media="(prefers-color-scheme: dark)">
<link rel="canonical" href="{url}">
<meta property="og:type" content="{og_type}">
<meta property="og:site_name" content="dctx">
<meta property="og:locale" content="en_US">
<meta property="og:title" content="{e(title)}">
<meta property="og:description" content="{e(route.desc)}">
<meta property="og:url" content="{url}">
{og}{times}<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{e(title)}">
<meta name="twitter:description" content="{e(route.desc)}">
{twitter}<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="alternate icon" href="/favicon-32.png" type="image/png" sizes="32x32">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="manifest" href="/manifest.webmanifest">
<link rel="license" href="{LICENSE_URL}">
<link rel="author" href="https://tejl.com/">
<link rel="help" href="/docs/">
<link rel="search" type="application/opensearchdescription+xml" title="dctx" href="/opensearch.xml">
<link rel="alternate" type="application/atom+xml" title="dctx news" href="/news/feed.xml">
<link rel="alternate" type="application/atom+xml" title="dctx releases" href="/changelog/feed.xml">
{me}{head_extra}{script_tags}<script type="application/ld+json">
{ld(g)}
</script>
</head>
<body>
<a class="skip" href="#main">Skip to content</a>
<header class="hdr">
 <div class="wrap hdr-in">
  <a class="brand" href="/">{ICON}<span>dctx</span><span class="ver">{VERSION}</span></a>
  <nav class="nav" aria-label="Primary"><ul>{nav}</ul></nav>
  <div class="hdr-end">
   <a class="tbtn search-link" href="/search/" aria-label="Search"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="11" cy="11" r="6.5"/><path d="m16 16 4.5 4.5"/></svg></a>
   <button class="tbtn" id="motion-toggle" type="button" aria-pressed="true" hidden>Motion: on</button>
   <button class="tbtn" id="theme-toggle" type="button" aria-label="Switch colour theme" hidden><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><circle cx="12" cy="12" r="8"/><path d="M12 4a8 8 0 0 0 0 16z" fill="currentColor"/></svg></button>
   <details class="menu"><summary class="tbtn" aria-label="Menu"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true"><path d="M4 7h16M4 12h16M4 17h16"/></svg></summary>
    <nav class="menu-panel" aria-label="Mobile"><ul>{nav}</ul></nav></details>
  </div>
 </div>
</header>
<main id="main"{f' class="{main_class}"' if main_class else ""}>
{body}
</main>
{footer()}
</body>
</html>
"""
