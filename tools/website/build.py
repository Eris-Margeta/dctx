#!/usr/bin/env python3
"""Generate the dctx website into website/ (or --out DIR).

    tools/website/.venv/bin/python tools/website/build.py            # build
    tools/website/.venv/bin/python tools/website/build.py --refresh-dates
    python3 tools/website/build.py --removed-from FILE                 # rewrite data/removed-paths.json

Inputs are repository files only (documents, CHANGELOG.md, the capture in data/demo.json, the
templates, the hand-written CSS/JS in src/ and the fonts in fonts/); see README.md. The build reads
no clock and no network: dates come from data/dates.json (refreshed from git with --refresh-dates),
so a clean `git archive` of a commit builds the same bytes as the committed website/.
Needs Markdown and Pillow (tools/website/requirements.txt).
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

if os.environ.get("PYTHONHASHSEED") != "0":
    os.environ["PYTHONHASHSEED"] = "0"
    os.execv(sys.executable, [sys.executable, *sys.argv])

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
STATIC = {".gitignore", "assets/tejl-logo-off-black.svg"}  # committed inputs inside website/


def refresh_dates(root: Path) -> int:
    from routes import DOCS, POLICIES
    files = sorted({f for _, f, *_ in DOCS} | {f for _, f, *_ in POLICIES} | {"CHANGELOG.md", "README.md"})
    out = {}
    for f in files:
        iso = subprocess.run(["git", "-C", str(root), "log", "-1", "--format=%cI", "--", f], check=True,
                             capture_output=True, text=True).stdout.strip()
        if iso:
            out[f] = iso
    (HERE / "data" / "dates.json").write_text(json.dumps(out, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    print(f"dates.json: {len(out)} files")
    return 0


def removed_from(path: str) -> int:
    """Hash the publish export's removal list (git filter-repo --paths-from-file format)."""
    paths, basenames = set(), set()
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("glob:**/"):
            basenames.add(hashlib.sha256(line[len("glob:**/"):].encode()).hexdigest())
        elif line.startswith(("glob:", "regex:")):
            continue
        else:
            paths.add(hashlib.sha256(line.encode()).hexdigest())
    (HERE / "data" / "removed-paths.json").write_text(json.dumps(
        {"note": "sha256 of repository paths (directories end in /) and base names the public tree does not contain",
         "paths": sorted(paths), "basenames": sorted(basenames)}, indent=1) + "\n", encoding="utf-8")
    print(f"removed-paths.json: {len(paths)} paths, {len(basenames)} base names")
    return 0


def build(out: Path) -> int:
    import llms
    import glyphs
    import md
    import og
    import shell
    from assets import Assets
    from common import ICON, READS, ROOT, VERSION, date_of, read_bytes
    from pages import animations, benchmarks, docs, examples, home, misc, paths, playground, reference
    from pages.news_content import NEWS
    from routes import DOCS, POLICIES, build_routes, doc_path
    from social_manifest import SocialStale, load_cards

    routes = build_routes(NEWS)
    scene = og.scene_geometry()
    # Preflight all committed triplets before rendering pages or touching the output.
    try:
        manifest, social = load_cards(routes, scene)
    except SocialStale as error:
        print(f"build: {error}", file=sys.stderr)
        return 1
    assets = Assets()
    assets.load()
    assets.social = manifest
    for entry in manifest.values():
        for kind in ("gif", "png", "twitter"):
            record = entry[kind]
            url = "/assets/og/" + record["file"]
            assets.urls[url] = url + "?v=" + record["sha256"][:12]
    site_images = og.site_images(scene)
    for url, content in site_images.items():
        assets.urls[url] = url + "?v=" + hashlib.sha256(content).hexdigest()[:12]
    social.update(site_images)
    route_of = {f: doc_path(s) for s, f, *_ in DOCS} | {f: p for p, f, *_ in POLICIES} | {"CHANGELOG.md": "/changelog/"}
    linker = md.Linker(route_of)
    rendered = {}
    for r in routes:
        if r.kind == "doc":
            rendered[r.path] = md.render(r.source, linker)
    files: dict[str, bytes] = {}
    pages: dict[str, str] = {}
    releases = misc.releases(linker)
    index_url = "/assets/search-index.json"
    doc_routes = [r for r in routes if r.kind == "doc" and r.group != "Policy"]

    def emit(route, body, graph=None, parents=None, script="page.js", head="", og_type="website", times=(None, None)):
        merged, extra = [], {}
        for node in graph or []:
            if node.get("@merge"):
                extra.update({k: v for k, v in node.items() if k != "@merge"})
            else:
                merged.append(node)
        pages[route.path] = shell.page(route, body, assets, graph=merged, parents=parents, script=script, head_extra=head,
                                       og_type=og_type, article_times=times, webpage_extra=extra)

    for r in routes:
        if r.kind == "home":
            emit(r, home.body(), script="home.js")
        elif r.path == "/start/":
            emit(r, paths.start())
        elif r.extra.get("persona"):
            emit(r, paths.persona(r.extra["persona"]), parents=[("/start/", "Choose your path")])
        elif r.path == "/for-ai-evaluators/":
            body, graph = paths.evaluators()
            emit(r, body, graph)
        elif r.path == "/docs/":
            emit(r, docs.index_page(rendered))
        elif r.kind == "doc":
            i = doc_routes.index(r) if r in doc_routes else -1
            prev = doc_routes[i - 1] if i > 0 else None
            nxt = doc_routes[i + 1] if 0 <= i < len(doc_routes) - 1 else None
            body, graph, (head, modified) = docs.doc_page(r, rendered[r.path], prev, nxt)
            parents = [("/docs/", "Documentation")] if r.group != "Policy" else []
            emit(r, body, graph, parents, head=head, og_type="article", times=(None, modified))
        elif r.path == "/cli/":
            body, graph = reference.cli()
            emit(r, body, graph)
        elif r.path == "/languages/":
            emit(r, reference.languages())
        elif r.path == "/examples/":
            body, graph = examples.examples()
            emit(r, body, graph, script="examples.js")
        elif r.path == "/templates/":
            body, graph = examples.templates()
            emit(r, body, graph)
        elif r.path == "/playground/":
            body, graph = playground.playground()
            emit(r, body, graph, script="playground.js")
        elif r.path == "/playground/offline/":
            emit(r, playground.offline(), parents=[("/playground/", "Playground")])
        elif r.path == "/benchmarks/":
            body, graph = benchmarks.benchmarks()
            emit(r, body, graph)
        elif r.path == "/how-it-works/":
            emit(r, animations.page(), script="how.js")
        elif r.kind == "changelog":
            emit(r, misc.changelog(linker))
        elif r.path == "/news/":
            emit(r, misc.news_index())
        elif r.kind == "post":
            body, graph = misc.news_post(r.extra["post"])
            emit(r, body, graph, parents=[("/news/", "News")], og_type="article", times=(r.extra["post"]["date"], None))
        elif r.kind == "search":
            emit(r, misc.search(index_url, routes), script="search.js")
        elif r.kind == "error":
            emit(r, misc.not_found(index_url), script="search.js")
        else:
            raise SystemExit(f"build: no page for {r.path}")

    for path, html in pages.items():
        pages[path] = glyphs.vectorize(html)
        files[path.lstrip("/") + ("index.html" if path.endswith("/") else "")] = pages[path].encode("utf-8")
    for path, doc in rendered.items():
        files[path.lstrip("/") + "index.md"] = glyphs.plain(doc.twin).encode("utf-8")
    for url, data in assets.files.items():
        files[url.lstrip("/")] = data
    files.update({url.lstrip("/"): content for url, content in social.items()})
    files["favicon.svg"] = (ICON.replace(' aria-hidden="true" focusable="false"', ' xmlns="http://www.w3.org/2000/svg"') + "\n").encode()
    files["llms.txt"] = llms.llms_txt(pages, routes).encode()
    files["llms-small.txt"] = llms.llms_small(pages, routes).encode()
    files["llms-full.txt"] = llms.llms_full(pages, routes).encode()
    for rel in llms.stale_sources():
        print(f"warning: {rel} changed since data/llms.json was captured; run capture.py --llms (check.py fails)", file=sys.stderr)
    files["news/feed.xml"] = llms.news_feed().encode()
    files["changelog/feed.xml"] = llms.changelog_feed(releases).encode()
    files["sitemap.xml"] = llms.sitemap(routes).encode()
    files["robots.txt"] = llms.robots().encode()
    files[".well-known/security.txt"] = llms.security_txt().encode()
    updated = max(d for d in (date_of(r.source) for r in routes if r.source) if d)
    files["humans.txt"] = llms.humans_txt(updated).encode()
    files["manifest.webmanifest"] = llms.manifest().encode()
    files["opensearch.xml"] = llms.opensearch().encode()
    files["assets/search-index.json"] = llms.search_index(pages, routes).encode()
    files["assets/openapi.json"] = read_bytes("tools/playground/openapi.json")

    for rel in files:
        if rel.endswith((".txt", ".xml")):
            files[rel] = glyphs.plain(files[rel].decode("utf-8")).encode("utf-8")

    # build.json: what was built from what. No clock: the source digest identifies the inputs.
    digest = hashlib.sha256("".join(f"{k}\0{v}\n" for k, v in sorted(READS.items())).encode()).hexdigest()
    # build_id: every output file (pages still carrying the mark), so a change of code, style or content
    # gives a new id; each page carries it in <meta name="dctx-build"> and site.js compares it with build.json.
    build_id = hashlib.sha256("".join(f"{k}\0{hashlib.sha256(v).hexdigest()}\n" for k, v in sorted(files.items())).encode()).hexdigest()
    mark = shell.BUILD_MARK.encode()
    for rel in [r for r in files if r.endswith(".html")]:
        files[rel] = files[rel].replace(mark, build_id.encode())
    total = sum(len(v) for v in files.values())
    files["build.json"] = (json.dumps({"version": VERSION, "build_id": build_id, "source_sha256": digest, "sources": len(READS),
                                        "sources_updated": updated, "pages": len(pages), "files": len(files) + 1 + len(STATIC),
                                        "bytes": total}, indent=1) + "\n").encode()

    # Write: keep the static inputs, replace everything else.
    out.mkdir(parents=True, exist_ok=True)
    for child in sorted(out.rglob("*"), reverse=True):
        rel = child.relative_to(out).as_posix()
        if rel in STATIC or any(s.startswith(rel + "/") for s in STATIC):
            continue
        if child.is_dir():
            if not any(child.iterdir()):
                child.rmdir()
        else:
            child.unlink()
    for rel in sorted(files):
        target = out / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(files[rel])
    if out.resolve() != (ROOT / "website").resolve():
        for rel in STATIC:
            if (ROOT / "website" / rel).exists() and not (out / rel).exists():
                (out / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / "website" / rel, out / rel)
    for w in sorted(set(linker.warnings)):
        print(f"warning: {w}", file=sys.stderr)
    print(f"build: {len(pages)} pages, {len(files) + len(STATIC)} files, {total:,} bytes; "
          f"llms-full.txt {len(files['llms-full.txt']):,} bytes; source {digest[:12]}")
    return 0


def main(argv: list[str]) -> int:
    from common import ROOT
    if "--refresh-dates" in argv:
        return refresh_dates(ROOT)
    if "--removed-from" in argv:
        return removed_from(argv[argv.index("--removed-from") + 1])
    out = Path(argv[argv.index("--out") + 1]).resolve() if "--out" in argv else ROOT / "website"
    try:
        import markdown  # noqa: F401
        import PIL  # noqa: F401
    except ImportError as error:
        print(f"build: {error}; install tools/website/requirements.txt (see tools/website/README.md)", file=sys.stderr)
        return 2
    return build(out)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
