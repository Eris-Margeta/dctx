<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 Eris Margeta -->
# The dctx website generator

`build.py` writes the static site into `website/`; `check.py` rebuilds it in isolation and fails on any
difference or broken rule. No framework, no Node build, no network during generation, no clock. Social cards require headless Chromium, FFmpeg and gifsicle.

```sh
python3 -m venv tools/website/.venv
tools/website/.venv/bin/pip install -r tools/website/requirements.txt
tools/website/.venv/bin/playwright install chromium
# Install ffmpeg and gifsicle with the system package manager.
# DCTX_CHROME=/path/to/chrome selects an already installed Chrome instead.
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
| `/docs/<slug>/`, `/security/`, `/trademark/`, `/maintenance/`, `/why/` (once `docs/WHY-DCTX.md` exists) | the repository Markdown, rendered at build time (`md.py`); `index.md` next to each page is the file itself with link targets rewritten and decorative symbols spelled in ASCII, and `check.py` fails if it drifts |
| `/cli/`, `/languages/`, `/examples/`, the home showcase | `data/demo.json`, recorded from the real binary by `capture.py` on `sample/`; `check.py` re-records and fails on drift |
| `/templates/` | `crates/dctx-template/templates/*.dctx_template`, ADR-003 and the template captures |
| `/benchmarks/`, the home facts | tables parsed from `docs/BENCHMARK-20261030.md`, the run 7 `aggregate.json` and the stability report (`data.py`) |
| `/how-it-works/` and the explainer animations | `pages/animations.py` (figures, captions, prose fallbacks) and `src/anim.js` (five deterministic canvas scenes, frame-counted, in the `home.js` and `how.js` bundles) |
| `/changelog/` and its feed | `CHANGELOG.md` |
| the home notice and path selector, `/start/` and its four persona pages, `/for-ai-evaluators/` | `data/facts.json` through `facts.py` (see below); persona runs are records from `data/demo.json` |
| the home showcase (five ways to use dctx: chat, task, agent, MCP, library) | `pages/tour.py`: a plain-language step beside the real captured run of each (`data/demo.json`, long output cut with an exact `… N lines` marker); `src/hero.js` turns it into tabs that advance with motion on |
| Social images, icons | `og.py` (Pillow + headless Chrome/CDP + FFmpeg + gifsicle), from the same captured scene geometry |
| `llms-small.txt`, `llms.txt`, `llms-full.txt` | `llms.py`: facts from `data/facts.json`, every route with its HTML meta description, the shared search/page text extraction, and document captures from `data/llms.json` (see below) |
| sitemap, feeds, robots, `security.txt`, `humans.txt`, manifest, OpenSearch, search index, `build.json` | `llms.py`, from the route table in `routes.py` |
| syntax colours in every code block, terminal transcript, help text and template | `highlight.py`, an in-house highlighter run at build time (no library, no runtime script); `.t-*` classes coloured by the `--hl-*` tokens in `src/site.css` |
| CSS and JS | hand-written in `src/`, published content-hashed (`assets.py`); `theme.js` runs inline and `tools/playground/nginx.conf` allows it by hash |
| fonts | `fonts/` (Geist and JetBrains Mono, SIL OFL 1.1), made once by `fonts.py` |

Every number on a hand-written page carries a small source link to the file it is parsed from.

## Facts and the llms files (AI evaluators)

People ask AI tools to evaluate dctx, and those tools read `/llms-small.txt`, `/llms.txt` and `/llms-full.txt`
first. The three files are built so they cannot fall behind the documentation: dctx produces them itself.

`data/facts.json` is the one facts source. `llms.py` renders it first in all three files (`facts_markdown()`);
the site pages read the same file through `facts.py` (`facts.load()` checks the required keys and the four personas and adds
the measured numbers parsed by `data.py`; numbers are never typed into facts.json). Inline code uses backticks, rendered by
`pages.paths.md_inline`. `check.py` rejects sales wording on these pages (the word "enterprise" is on that list; only the disclaimer "not enterprise-ready" passes). Schema 1, fields in render order:

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
| `notice_status` | one line under the notice on the site: what is planned and what the interim wording uses |
| `safe_path[]` | `[step, line]` pairs for /start/ |
| `ways[]` | the home modalities grid (usage patterns: CLI, agents short and long session, MCP and ACP, libraries, CI): `name`, `line`, `status`, `link` |
| `responsibilities` | `head` and four `rows` of level / dctx provides / you are responsible for, planned items marked |
| `personas[]` | the four choose-your-path pages: `id` (the path /start/ID/), `name`, `who`, `gets` (what you get, one plain line), `modality`, `setup[]`, `first_command`, `expect`, `record` (a `data/demo.json` id shown as the real run), `pitfall`, `links[]` |
| `install` | `system` and `project` text for the home install block |
| `philosophy` | the home philosophy paragraph |

Links starting with `/` are site paths. Everything marked planned stays labelled planned (see the honesty rule
in the canonical wording); change a status only when the release has it.

Each llms file starts with an H1 and blockquote summary, capture provenance, the facts block and
sections linking every public HTML route with its meta description. `llms.txt` is the page index;
`llms-small.txt` also adds `dctx map map FILE --depth 2` of the start documents.
`llms-full.txt` retains the document pack (documents, policies, changelog and built-in templates),
then includes every HTML page under its absolute URL and title, in route-table order. `extract_page()`
is shared with the search index. For packed documents, a link identifies the document and only rendered
lines absent verbatim from the pack are added, so formatting changes and page details remain readable
without copying the same prose twice. Search, offline and error pages are included too.

`routes.llms_tiers()` defines the document capture budgets. Documents are taken in order while the
capture stays within its estimated-token budget; the rest are named as left out. If the engine changes
a document's bytes inside the pack, the file says so and names the documents. Those capture budgets
do not limit HTML page coverage. The full file must fit `LLMS_LIMIT` (1 MiB): if necessary, the largest
supplemental CLI/example listings become one-paragraph summaries linking their pages. The build reports
the byte counts and never drops page prose or the document pack; it fails if those alone exceed the limit.

```sh
DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py --llms   # rewrite data/llms.json
```

Run it after committing a document change (the docs commit is part of the output, as with `dates.json`).
`build.py` warns when a document differs from the capture. `check.py` fails when a document's sha256 differs
from `data/llms.json`, when the docs commit differs from git (skipped in a shallow clone), when a re-capture
with `DCTX_BIN` differs by one byte, when `facts.json` disagrees with `VERSION`, `CHANGELOG.md` or the tag,
when facts in any llms file differ from `facts.json`, when any built page's extracted lines are absent
from `llms-full.txt`, when a URL or meta description is absent from `llms.txt`, when a page is absent
from the route table, when a link does not resolve, and when a removed command (`removed.items[].patterns`)
appears anywhere except the removed list, a historical source inside a pack (`CHANGELOG.md`, `docs/research/`)
or a line that itself calls it legacy or removed. An HTML page may show the removed list only inside one
element with `data-facts="removed"`.

## Home page order and the animation hook

`pages/home.py` follows the design review: plain-language claim and the showcase, the project status, the
choose-your-path selector (who it is for and what you get first, setup, first command and pitfall folded) with the notice, captured output, what has been measured, modalities, responsibilities,
philosophy, install per scope; the footer links the llms files. Five slots call
`pages/animations.py` when that module exists: `two_scopes()`, `calls_times_context()`,
`prefix_stability()`, `giant_folder()` and `safe_path()`, each returning an HTML string placed in a
`<div class="anim-slot" data-anim="NAME">`. Without the module, or when a function returns an empty
string, the slot renders nothing and the build passes.

The selector remembers the last chosen persona in `localStorage` (`dctx-path`, wrapped in try/catch)
only to mark that card; every page works without storage.

## Readability

Base text 18 px (17 px under 600 px), line height 1.6 (headings 1.3), heading scale 1.25 (h1 2rem,
h2 1.56rem, h3 1.25rem, h4 1rem), prose measure 68ch, code 15 px with horizontal scroll, light code
blocks in the light theme, no text under 14 px, tables zebra-striped at a 3 % tint.

## Inputs that are not rebuilt

- `data/dates.json`: last commit date of each rendered document. Refresh with `build.py --refresh-dates` after
  committing a document change; `check.py` compares it with git (skipped in a shallow clone).
- `data/removed-paths.json`: hashed names of files the public repository does not contain, so links to them
  become plain text. Rewrite with `build.py --removed-from <list>`.
- `website/assets/tejl-logo-off-black.svg` and `website/.gitignore`.

## Loading

First visit to `/`: 7 requests (HTML, one stylesheet, one script, four preloaded fonts).
Fonts are preloaded and use `font-display: optional` (a font that misses the first paint is not swapped in,
so a slow font never moves text), with metric-matched fallback faces (`size-adjust` and ascent, descent and
line-gap overrides) for Arial/Helvetica/Liberation Sans and DejaVu Sans, computed from letter-frequency
weighted advance widths of Geist against each fallback. Every showcase panel is complete HTML from the first paint;
with JS the panels share one grid cell, so the tallest sets the height and switching steps never moves the page. With motion
on, the steps advance every 7 s (paused on hover, focus, a hidden tab or off screen) and the lines of the shown panel brighten
in order (never blank); with motion off or reduced motion nothing moves and every step stays one click away. Every page also carries the id of its build
(`<meta name="dctx-build">`, a digest of every output file); `site.js` compares it with `/build.json` (fetched with
`cache: 'no-store'` on load, when the tab becomes visible and every 10 minutes) and offers a reload when they differ.
The CSS is minified at build time (`assets.minify_css`). `check.py` enforces the first-load
budget (requests, bytes, largest asset) of every page.

## Rules check.py enforces

Isolated rebuild equals HEAD; capture drift; `--version` equals `VERSION`; deny-list (`DCTX_DENY_LIST`)
over `website/`, `tools/website/`, `tools/playground/`; one `h1`, no skipped heading levels, `alt` and
`width`/`height` on images, no inline handlers or styles, one inline script (the theme script, by hash);
internal links and fragments; external links only to the allowlist; JSON-LD types and the binding
Person/Organization/SoftwareApplication values; social cards (GIF then PNG at 1200×630, separate Twitter PNG at 1200×600, cache hashes, 2–4 second infinite loops and exact frame-zero fallback); vector SVG accessibility and forbidden Unicode glyphs (including HTML entities and JS/CSS escapes);
the llms files (size limits, facts first, every facts and route-table URL resolving, capture and document drift, removed commands); Markdown twins against their sources with only link and symbol normalization; sitemap equals the
indexable pages; feeds parse; `security.txt` expiry; hashed and referenced assets; CSS, JS, font and
HTML budgets; WCAG contrast of the colour tokens in both themes (body text and links 7:1, secondary text and the nine syntax colours 4.5:1); nginx.conf headers; no legacy commands
or rule files and no sales wording on hand-written pages.

## Vector glyphs and social cards

`glyphs.py` converts decorative symbols, including HTML entities, into inline SVGs
using currentColor, explicit dimensions, aria-hidden and focusable=false. An adjacent
accessible label preserves each symbol's meaning. Published Markdown and LLM text
use readable ASCII equivalents (`->`, `[yes]`, `[no]`, `[pending]`, etc.); repository
sources and recorded CLI captures stay byte-for-byte unchanged. New unmapped symbols
fail generation rather than falling back to a platform font. Prose punctuation stays text.
`check.py` scans every served text file, including encoded HTML, CSS and JS characters.

For each unique page card, `og.py` extends the existing Pillow artwork with the
captured hero graph. `browser/social.js` draws only when `renderFrame(seconds)` is
called: 24 CDP frames at 8 fps make a seamless three-second loop. Subtle cyan signals
follow the real graph edges; neither randomness nor wall-clock animation is involved.
FFmpeg uses one `palettegen`/`paletteuse` palette for the entire loop, then gifsicle
runs `-O3 --lossy=40`. The static fallback is decoded from the optimized GIF's first
frame so its pixels match exactly. Twitter has its own 1200×600 composition with a
repositioned footer. All image copy comes from page headings and meta descriptions.
Error and offline pages also have their own page copy and image triplets.
Every image reference carries `?v=<sha256 prefix>` calculated from its shipped bytes.

The complete documented build regenerates these images; `--skip-og` is only a local
editing shortcut after a full build. `check.py --working-tree` always performs an
independent full rebuild, including Chrome rendering and both encoders. Temporary
frames and browser profiles are deleted and never copied into the served tree.
