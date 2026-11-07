"""The single route table: sitemap, llms.txt, navigation, search index and OG images read it."""
from __future__ import annotations

from dataclasses import dataclass, field

from common import ROOT


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


# (slug, file, title, group, one line). Order = sidebar order = llms-full order.
DOCS = [
    ("readme", "README.md", "README", "Start here", "Installation, quick start, map mode, language tiers and the CLI reference."),
    ("spec", "docs/SPEC.md", "Specification", "Start here", "The core belief, the five constraints, the ten RAG defect classes, policy and roadmap."),
    ("modalities", "docs/MODALITIES.md", "Choosing how to use dctx", "Start here", "Clipboard, grep plus reads, long sessions, huge trees, MCP startup cost and CI."),
    ("map-mode", "docs/guides/MAP-MODE.md", "Map mode guide", "Start here", "Every map verb with examples, budgets, ids and precision labels."),
    ("agents", "docs/guides/AGENTS-SNIPPET.md", "Agent snippet", "Using dctx", "The AGENTS.md section that tells coding agents to ask dctx first."),
    ("plugins", "docs/guides/PLUGINS.md", "Plugins guide", "Using dctx", "The plugin boundary, capabilities, consent and current limits."),
    ("exit-codes", "docs/CLI-EXIT-CODES.md", "CLI exit codes", "Using dctx", "The exit-code contract for map, pack, mcp and languages."),
    ("privacy", "docs/ADR-014-Privacy-Guarantees-and-Ignore-Semantics.md", "Privacy guarantees", "Using dctx", "Ignore semantics and the full sensitive-file deny list."),
    ("templates-contract", "docs/design/contracts/DTS-v2.md", "Template contract (DTS v2)", "Templates", "DTS source v2 and the compiled plan IR v1."),
    ("templating", "docs/ADR-003-Templating-Engine.md", "Templating engine (ADR-003)", "Templates", "Markers, ahead-of-time compilation, the two rendering passes and the data variables."),
    ("language-coverage", "docs/ADR-018-Language-Coverage.md", "Language coverage (ADR-018)", "Using dctx", "The three tiers and how a language enters one."),
    ("benchmark", "docs/BENCHMARK-20261030.md", "Measurement evidence", "Evidence", "Grep plus file reads first; whole-session scope and secondary historical protocols."),
    ("stability", "docs/research/stability-20261105/REPORT.md", "Byte-stability report", "Evidence", "Protocol, raw answers and limits of the 33/33 measurement."),
    ("benchmark-oct", "docs/research/trail-code-2026-10/REPORT.md", "Earlier two-way study", "Evidence", "October 2026, before call edges: the benchmark that set the roadmap."),
    ("crate-map", "docs/CRATE-MAP.md", "Crate map", "Project", "How the Rust workspace is organised."),
    ("contributing", "CONTRIBUTING.md", "Contributing", "Project", "How to propose changes, with a DCO sign-off."),
    ("releasing", "RELEASING.md", "Releasing", "Project", "How a release is cut and checked."),
]
# docs/WHY-DCTX.md is written on its own card; the page appears (first in the docs, at /why/) as soon
# as the file is in the tree, and nothing fails while it is not.
WHY = ("why", "docs/WHY-DCTX.md", "Why dctx", "Start here",
       "Why dctx exists, its philosophy, and why byte-identical output keeps the prompt cache hitting.")
if (ROOT / WHY[1]).is_file():
    DOCS.insert(0, WHY)
DOC_GROUPS = ["Start here", "Using dctx", "Templates", "Evidence", "Project"]

# Project policies rendered with the docs renderer at top-level paths.
POLICIES = [
    ("/security/", "SECURITY.md", "Security policy", "How to report a vulnerability privately, and what happens next."),
    ("/trademark/", "TRADEMARK.md", "Trademark policy", "What the dctx name and logo may be used for, and what forks may call themselves."),
    ("/maintenance/", "MAINTENANCE.md", "Maintenance", "dctx is free under Apache-2.0. Maintenance agreements fund security response, long-term support and priority work."),
]

NAV = [("/docs/", "Docs", "docs"), ("/cli/", "CLI", "cli"), ("/examples/", "Examples", "examples"),
       ("/templates/", "Templates", "templates"), ("/playground/", "Playground", "playground"),
       ("/benchmarks/", "Benchmarks", "benchmarks"), ("/changelog/", "Changelog", "changelog")]


def doc_path(slug: str) -> str:
    return "/why/" if slug == "why" else f"/docs/{slug}/"


def has_why() -> bool:
    return DOCS[0][0] == "why"


def build_routes(news: list[dict]) -> list[Route]:
    routes = [
        Route("/", "dctx", "dctx is the open-source context engine for AI-assisted development: deterministic, bounded code context "
              "for agents and chat, offline, with no telemetry. Apache-2.0.", "home", og="home", llms=None),
        Route("/docs/", "Documentation", "Specification, guides, benchmark reports and project policies for dctx, rendered from the repository.",
              "page", "docs", llms="Docs"),
    ]
    for slug, file, title, group, line in DOCS:
        routes.append(Route(doc_path(slug), title, line, "doc", "docs", source=file, group=group, og=f"docs-{slug}",
                            llms="Evidence" if group == "Evidence" else ("Project" if group == "Project" else "Docs")))
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
    # After the policies so that adding it shifts no earlier OG image seed.
    routes.append(Route("/how-it-works/", "How dctx works", "Five short animations: two scopes of one binary, calls times context, "
                        "prefix stability, the giant folder and the safe path for new users.", "page", "docs", og="how-it-works", llms="Docs"))
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
