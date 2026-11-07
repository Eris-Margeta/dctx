"""The single route table: sitemap, llms.txt, navigation, search index and OG images read it."""
from __future__ import annotations

from dataclasses import dataclass, field

import json

from common import ROOT, read


@dataclass
class Route:
    path: str
    title: str
    desc: str
    kind: str                      # home, doc, page, news, post, changelog, search, error
    section: str | None = None     # nav item that is current
    source: str | None = None      # primary repository file
    group: str | None = None       # docs group, llms.txt section
    og: str = ""                   # OG image slug
    crumb: str | None = None       # breadcrumb label (defaults to title)
    indexable: bool = True
    llms: str | None = None        # llms.txt section
    extra: dict = field(default_factory=dict)

    @property
    def slug(self) -> str:
        return self.og or (self.path.strip("/").replace("/", "-") or "home")


# (slug, file, title, group, one line). Order = sidebar order = llms-full order = pager order.
# Groups follow the design review (R3, section 2): Start, Modalities, Scopes, Engine, Reference, Trust.
DOCS = [
    ("readme", "README.md", "README", "Start", "Installation, quick start, map mode, language tiers and the CLI reference."),
    ("modalities", "docs/MODALITIES.md", "Choosing how to use dctx", "Modalities", "Clipboard, grep plus reads, long sessions, huge trees, MCP startup cost and CI."),
    ("map-mode", "docs/guides/MAP-MODE.md", "Map mode guide", "Modalities", "Every map verb with examples, budgets, ids and precision labels."),
    ("agents", "docs/guides/AGENTS-SNIPPET.md", "Agent snippet", "Modalities", "The AGENTS.md section that tells coding agents to ask dctx first."),
    ("plugins", "docs/guides/PLUGINS.md", "Plugins guide", "Modalities", "The plugin boundary, capabilities, consent and current limits."),
    ("spec", "docs/SPEC.md", "Specification", "Engine", "The core belief, the five constraints, the ten RAG defect classes, policy and roadmap."),
    ("privacy", "docs/ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md", "Privacy guarantees", "Engine", "Ignore semantics and the full sensitive-file deny list."),
    ("language-coverage", "docs/ADR-018-Language-Coverage.md", "Language coverage (ADR-018)", "Engine", "The three tiers and how a language enters one."),
    ("templates-contract", "docs/design/contracts/DTS-v2.md", "Template contract (DTS v2)", "Engine", "DTS source v2 and the compiled plan IR v1."),
    ("templating", "docs/ADR-003-Templating-Engine.md", "Templating engine (ADR-003)", "Engine", "Markers, ahead-of-time compilation, the two rendering passes and the data variables."),
    ("crate-map", "docs/CRATE-MAP.md", "Crate map", "Engine", "How the Rust workspace is organised."),
    ("exit-codes", "docs/CLI-EXIT-CODES.md", "CLI exit codes", "Reference", "The exit-code contract for map, pack, mcp and languages."),
    ("benchmark", "docs/BENCHMARK-20261030.md", "Measurement evidence", "Trust", "Grep plus file reads first; whole-session scope and secondary historical protocols."),
    ("stability", "docs/research/stability-20261105/REPORT.md", "Byte-stability report", "Trust", "Protocol, raw answers and limits of the 33/33 measurement."),
    ("benchmark-oct", "docs/research/trail-code-2026-10/REPORT.md", "Earlier two-way study", "Trust", "October 2026, before call edges: the benchmark that set the roadmap."),
    ("releasing", "RELEASING.md", "Releasing", "Trust", "How a release is cut and checked."),
    ("contributing", "CONTRIBUTING.md", "Contributing", "Trust", "How to propose changes, with a DCO sign-off."),
]
LLMS_SECTION = {"benchmark": "Evidence", "stability": "Evidence", "benchmark-oct": "Evidence",
                "crate-map": "Project", "contributing": "Project", "releasing": "Project"}
# docs/START.md (the safe path in ten minutes) and docs/WHY-DCTX.md are written on their own cards; each
# page appears (START at /docs/start/, WHY first in the docs at /why/) as soon as its file is in the tree,
# and nothing fails while it is not.
START = ("start", "docs/START.md", "Safe path in ten minutes", "Start",
         "One project, the system binary and the lean MCP profile: install, first command, what to measure.")
if (ROOT / START[1]).is_file():
    DOCS.insert(1, START)
WHY = ("why", "docs/WHY-DCTX.md", "Why dctx", "Start",
       "Why dctx exists, its philosophy, and why byte-identical output keeps the prompt cache hitting.")
if (ROOT / WHY[1]).is_file():
    DOCS.insert(0, WHY)
DOC_GROUPS = ["Start", "Modalities", "Engine", "Reference", "Trust"]
PERSONAS = [(p["id"], p["name"], p["who"]) for p in json.loads(read("tools/website/data/facts.json"))["personas"]]

# Project policies rendered with the docs renderer at top-level paths.
POLICIES = [
    ("/security/", "SECURITY.md", "Security policy", "How to report a vulnerability privately, and what happens next."),
    ("/trademark/", "TRADEMARK.md", "Trademark policy", "What the dctx name and logo may be used for, and what forks may call themselves."),
    ("/maintenance/", "MAINTENANCE.md", "Maintenance", "dctx is free under Apache-2.0. Maintenance agreements fund security response, long-term support and priority work."),
]

NAV = [("/start/", "Start", "start"), ("/docs/", "Docs", "docs"), ("/cli/", "CLI", "cli"), ("/examples/", "Examples", "examples"),
       ("/templates/", "Templates", "templates"), ("/playground/", "Playground", "playground"),
       ("/benchmarks/", "Benchmarks", "benchmarks"), ("/changelog/", "Changelog", "changelog")]


def doc_path(slug: str) -> str:
    return "/why/" if slug == "why" else f"/docs/{slug}/"


def has_why() -> bool:
    return DOCS[0][0] == "why"


def doc_nav() -> list[tuple[str, list[tuple[str, str]]]]:
    """The docs sidebar: the R3 groups, mixing rendered documents and generated pages.

    Only pages that exist are linked: a document missing from DOCS (START, WHY) drops out."""
    docs = {s: (t, doc_path(s)) for s, _, t, *_ in DOCS}

    def d(slug: str, label: str | None = None, frag: str = ""):
        return (label or docs[slug][0], docs[slug][1] + frag) if slug in docs else None
    groups = [
        ("Start", [d("why", "What dctx is"), ("Choose your path", "/start/"), d("start"), d("readme", "README and quick start")]),
        ("Modalities", [d("modalities"), d("map-mode", "CLI and agents: map mode"), d("agents"),
                        d("modalities", "MCP", "#mcp-server-convenient-tools-with-startup-context"),
                        d("modalities", "Libraries", "#rust-library-embed-the-facade"),
                        d("modalities", "CI and caching", "#ci--prompt-cache-prefix-stability"), d("plugins")]),
        ("Scopes", [("Two scopes, one binary", "/start/#two-scopes"), ("Who is responsible for what", "/start/#responsibilities")]),
        ("Engine", [d("spec"), d("privacy"), d("modalities", "Cache and limits", "#huge-trees-scope-before-indexing"),
                    d("language-coverage"), d("templates-contract"), d("templating"), d("crate-map")]),
        ("Reference", [("CLI reference", "/cli/"), d("exit-codes"), ("Languages", "/languages/"), ("Templates", "/templates/"),
                       ("Examples", "/examples/")]),
        ("Trust", [("Benchmarks", "/benchmarks/"), d("benchmark"), d("stability"), d("benchmark-oct"),
                   d("why", "Philosophy", "#the-philosophy"), ("Security policy", "/security/"), d("releasing"), d("contributing"),
                   ("Trademark", "/trademark/"), ("Maintenance", "/maintenance/")]),
        ("For AI evaluators", [("Facts and verification", "/for-ai-evaluators/"), ("llms.txt", "/llms.txt")]),
    ]
    return [(g, [i for i in items if i]) for g, items in groups]


def build_routes(news: list[dict]) -> list[Route]:
    routes = [
        Route("/", "dctx", "dctx is the open-source context engine for AI-assisted development: deterministic, bounded code context "
              "for agents and chat, offline, with no telemetry. Apache-2.0.", "home", og="home", llms=None),
        Route("/docs/", "Documentation", "Specification, guides, benchmark reports and project policies for dctx, rendered from the repository.",
              "page", "docs", llms="Docs"),
    ]
    routes.append(Route("/start/", "Choose your path", "dctx is an advanced tool with several ways to use it. Pick the path that "
                        "matches your work: one giant folder, many small projects, a library in your own tool, or a team rollout.",
                        "page", "start", og="start", llms="Docs"))
    for pid, name, who in PERSONAS:
        routes.append(Route(f"/start/{pid}/", name, f"{who} Setup, first command, what to expect and the pitfall to avoid.", "page", "start", og=f"start-{pid}", llms="Docs",
                            extra={"persona": pid}))
    routes.append(Route("/for-ai-evaluators/", "For AI evaluators", "What dctx is and is not, its version, modalities, "
                        "benchmark status, removed commands and how to verify every claim.", "page", None, og="for-ai-evaluators",
                        llms="Docs"))
    for slug, file, title, group, line in DOCS:
        routes.append(Route(doc_path(slug), title, line, "doc", "docs", source=file, group=group, og=f"docs-{slug}",
                            llms=LLMS_SECTION.get(slug, "Docs")))
    routes += [
        Route("/cli/", "CLI reference", "Every dctx command, flag, environment variable, exit code and MCP tool, captured from the dctx binary.",
              "page", "cli", og="cli", llms="CLI"),
        Route("/languages/", "Languages", "The 50 language families dctx indexes, by tier: full, outline and docs.",
              "page", "cli", og="languages", llms="CLI"),
        Route("/examples/", "Examples", "Every dctx command on one sample project, with its exact output, exit code, bytes and estimated tokens.",
              "page", "examples", og="examples", llms="Examples"),
        Route("/templates/", "Templates", "The two built-in dctx templates, their source, their output on the same project and what changes between them.",
              "page", "templates", og="templates", llms="Examples"),
        Route("/playground/", "Playground", "Run the real dctx binary on a sample project or on your own text files, in the browser.",
              "page", "playground", og="playground", llms="Examples"),
        Route("/benchmarks/", "Benchmarks", "Grep plus file reads, whole-session accounting, byte stability and secondary historical measurements.",
              "page", "benchmarks", og="benchmarks", llms="Evidence"),
        Route("/changelog/", "Changelog", "Every dctx release, from dircontxt 1.0.0 in C to dctx in Rust, with known issues.",
              "changelog", "changelog", source="CHANGELOG.md", og="changelog", llms="Optional"),
        Route("/news/", "News", "Releases, measurements and product decisions from the dctx project.", "page", "news", og="news", llms="Optional"),
    ]
    for post in news:
        routes.append(Route(f"/news/{post['slug']}/", post["title"], post["summary"], "post", "news", og=f"news-{post['slug']}",
                            crumb=post["title"], llms="Optional", extra={"post": post}))
    for path, file, title, line in POLICIES:
        routes.append(Route(path, title, line, "doc", None, source=file, group="Policy", og=path.strip("/"), llms="Project"))
    routes += [
        Route("/search/", "Search", "Search the dctx website.", "search", og="search", indexable=False),
        Route("/playground/offline/", "Playground offline", "The playground service is not reachable right now.", "page", "playground",
              og="playground", indexable=False),
        Route("/404.html", "Page not found", "This page does not exist on the dctx website.", "error", og="home", indexable=False),
    ]
    paths = [r.path for r in routes]
    if len(paths) != len(set(paths)):
        raise SystemExit("route table: duplicate paths")
    return routes
