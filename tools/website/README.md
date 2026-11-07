<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# The dctx website generator

`build.py` writes the static site into `website/`; `check.py` rebuilds it in isolation and fails on any
difference or broken rule. No framework, no Node build, no network, no clock.

```sh
python3 -m venv tools/website/.venv
tools/website/.venv/bin/pip install -r tools/website/requirements.txt
cargo build --locked --release -p dctx-cli
tools/website/.venv/bin/python tools/website/build.py
DCTX_BIN=target/release/dctx-cli tools/website/.venv/bin/python tools/website/check.py
```

`just site`, `just site-capture`, `just site-serve` and `just site-fonts` wrap these.

The deployment workflow builds first and uses `check.py --working-tree --no-capture`
to compare that fresh `website/` against an independent rebuild. All content checks
still run; only the reference bytes change from HEAD to the generated working tree.
Normal CI retains the default HEAD comparison and full CLI capture drift check.
For uncommitted documentation changes, run the full check against the generated tree:

```sh
DCTX_BIN=target/release/dctx-cli tools/website/.venv/bin/python tools/website/check.py --working-tree
tools/website/.venv/bin/python tools/website/test_build.py
```

The unit tests check the freshly generated working tree. The default `check.py`
still compares against HEAD for CI. Both checks run `scripts/render_readme.py --check`
to catch drift in the README measurements, output examples and compiled Rust snippet.

To refresh output examples after a CLI or sample change:

```sh
DCTX_BIN=target/release/dctx-cli tools/website/.venv/bin/python tools/website/capture.py
python3 scripts/render_readme.py
python3 scripts/render_readme.py --check
tools/website/.venv/bin/python tools/website/build.py
DCTX_BIN=target/release/dctx-cli tools/website/.venv/bin/python tools/website/check.py --working-tree
```

The README, home page and `/cli/` share `output_examples.py`: the first 40 pack
lines plus an exact `… (N more lines)` count, find/read (including JSON source),
and the full task pack with an 800-token estimated budget. Nothing in those
examples is handwritten. The Rust snippet comes from the workspace doctest in
`examples/library/README.md`; test it with `cargo test --locked -p dctx-library-example --doc`.

## Where everything comes from

| Output | Source |
|---|---|
| `/docs/<slug>/`, `/security/`, `/trademark/`, `/maintenance/`, `/why/` (once `docs/WHY-DCTX.md` exists) | the repository Markdown, rendered at build time (`md.py`); `index.md` next to each page is the file itself with only link targets rewritten, and `check.py` fails if it drifts |
| `/cli/`, `/languages/`, `/examples/`, the hero terminal | `data/demo.json`, recorded from the real binary by `capture.py` on `sample/`; `check.py` re-records and fails on drift |
| `/templates/` | `crates/dctx-template/templates/*.dctx_template`, ADR-003 and the template captures |
| `/benchmarks/`, the home facts | tables parsed from `docs/BENCHMARK-20261030.md`, the run 7 `aggregate.json` and the stability report (`data.py`) |
| `/changelog/` and its feed | `CHANGELOG.md` |
| OG images, the poster, icons | `og.py` (Pillow), from the same scene geometry as the canvas animation |
| `llms-small.txt`, `llms.txt`, `llms-full.txt` | `llms.py`: the facts block from `data/facts.json`, the route table in `routes.py`, then the dctx binary's own output over the documents, recorded in `data/llms.json` by `capture.py --llms` (see below) |
| sitemap, feeds, robots, `security.txt`, `humans.txt`, manifest, OpenSearch, search index, `build.json` | `llms.py`, from the route table in `routes.py` |
| CSS and JS | hand-written in `src/`, published content-hashed (`assets.py`); `theme.js` runs inline and `tools/playground/nginx.conf` allows it by hash |
| fonts | `fonts/` (Geist and JetBrains Mono, SIL OFL 1.1), made once by `fonts.py` |

Every number on a hand-written page carries a small source link to the file it is parsed from.

## Facts and the llms files (AI evaluators)

People ask AI tools to evaluate dctx, and those tools read `/llms-small.txt`, `/llms.txt` and `/llms-full.txt`
first. The three files are built so they cannot fall behind the documentation: dctx produces them itself.

`data/facts.json` is the one facts source. `llms.py` renders it first in all three files (`facts_markdown()`);
any page that shows facts (such as `/for-ai-evaluators/`) reads the same file. Schema 1, fields in render order:

| Field | Content |
|---|---|
| `version`, `released`, `release_tag` | current version (must equal `VERSION`), release date (must equal the `CHANGELOG.md` heading and the commit date of the tag; never the clock), tag name |
| `is`, `is_not[]` | one sentence; what dctx is not (no RAG, no network, no telemetry, no agent loop) |
| `verify_download` | `available`, `command`, `planned`, `link`: how to check a download today (SHA256SUMS) and what is planned (signatures) |
| `modalities[]` | `name`, `status` (`available`, `available from Git`, `preview`, `publishing soon`, `planned`), `line`, `link` |
| `scopes[]` | the system binary and the project binary, one line each |
| `notice` | the advanced-user notice, verbatim (interim wording until the three-tool profile and receipts ship) |
| `benchmarks` | `baseline`, `claims[]` with `status` (`being prepared`, `measured`, `historical`, `hypothesis`), `text`, `link` |
| `removed` | `since`, `intro`, `items[]` with `was`, `use` and `patterns` (Python regexes that find the old command) |
| `verify[]` | `what`, then `command` (shown as code) or `text`, and `link` |

Links starting with `/` are site paths. Everything marked planned stays labelled planned (see the honesty rule
in the canonical wording); change a status only when the release has it.

Each llms file is: title and strapline; "About this file" (engine version, docs commit and its date from git,
the exact dctx commands, the budget); the facts block; the route table (`## Pages`); then the dctx output.
`routes.llms_tiers()` defines the tiers: `llms-small.txt` adds `dctx map map FILE --depth 2` of the start
documents, `llms.txt` adds `dctx pack` of the start documents, and `llms-full.txt` adds `dctx pack` of every
document, policy, the changelog and the built-in templates, then the captured CLI help and examples.
Documents are taken in order while the output stays within the tier's budget in estimated tokens; the rest
are named as left out. If the engine changes a document's bytes inside the pack (dctx 1.2.0 substitutes
`{{name}}` template variables in file content), the file says so and names the documents.

```sh
DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py --llms   # rewrite data/llms.json
```

Run it after committing a document change (the docs commit is part of the output, as with `dates.json`).
`build.py` warns when a document differs from the capture. `check.py` fails when a document's sha256 differs
from `data/llms.json`, when the docs commit differs from git (skipped in a shallow clone), when a re-capture
with `DCTX_BIN` differs by one byte, when `facts.json` disagrees with `VERSION`, `CHANGELOG.md` or the tag, when
a link in the facts or the route table does not resolve, and when a removed command (`removed.items[].patterns`)
appears anywhere except the removed list, a historical source inside a pack (`CHANGELOG.md`, `docs/research/`)
or a line that itself calls it legacy or removed. An HTML page may show the removed list only inside one
element with `data-facts="removed"`.

## Inputs that are not rebuilt

- `data/dates.json`: last commit date of each rendered document. Refresh with `build.py --refresh-dates` after
  committing a document change; `check.py` compares it with git (skipped in a shallow clone).
- `data/removed-paths.json`: hashed names of files the public repository does not contain, so links to them
  become plain text. Rewrite with `build.py --removed-from <list>`.
- `website/assets/tejl-logo-off-black.svg` and `website/.gitignore`.

## Loading

First visit to `/`: 8 requests (HTML, one stylesheet, one script, four preloaded fonts, the poster image).
Fonts are preloaded and use `font-display: optional` (a font that misses the first paint is not swapped in,
so a slow font never moves text), with metric-matched fallback faces (`size-adjust` and ascent, descent and
line-gap overrides) for Arial/Helvetica/Liberation Sans and DejaVu Sans, computed from letter-frequency
weighted advance widths of Geist against each fallback. The hero animation starts after the load event,
when the main thread is idle; the poster is on screen before that. `check.py` enforces the first-load
budget (requests, bytes, largest asset) of every page.

## Rules check.py enforces

Isolated rebuild equals HEAD; capture drift; `--version` equals `VERSION`; deny-list (`DCTX_DENY_LIST`)
over `website/`, `tools/website/`, `tools/playground/`; one `h1`, no skipped heading levels, `alt` and
`width`/`height` on images, no inline handlers or styles, one inline script (the theme script, by hash);
internal links and fragments; external links only to the allowlist; JSON-LD types and the binding
Person/Organization/SoftwareApplication values; OG images (1200×630, dark and light, the 24-frame GIF);
the llms files (size limits, facts first, every facts and route-table URL resolving, capture and document drift, removed commands); Markdown twins against their sources; sitemap equals the
indexable pages; feeds parse; `security.txt` expiry; hashed and referenced assets; CSS, JS, font and
HTML budgets; WCAG contrast of the colour tokens in both themes; nginx.conf headers; no legacy commands
or rule files and no sales wording on hand-written pages.
