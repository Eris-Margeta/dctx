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
```

## Where everything comes from

| Output | Source |
|---|---|
| `/docs/<slug>/`, `/security/`, `/trademark/`, `/maintenance/`, `/why/` (once `docs/WHY-DCTX.md` exists) | the repository Markdown, rendered at build time (`md.py`); `index.md` next to each page is the file itself with only link targets rewritten, and `check.py` fails if it drifts |
| `/cli/`, `/languages/`, `/examples/`, the hero terminal | `data/demo.json`, recorded from the real binary by `capture.py` on `sample/`; `check.py` re-records and fails on drift |
| `/templates/` | `crates/dctx-template/templates/*.dctx_template`, ADR-003 and the template captures |
| `/benchmarks/`, the home facts | tables parsed from `docs/BENCHMARK-20261030.md`, the run 7 `aggregate.json` and the stability report (`data.py`) |
| `/changelog/` and its feed | `CHANGELOG.md` |
| OG images, the poster, icons | `og.py` (Pillow), from the same scene geometry as the canvas animation |
| `llms.txt`, `llms-small.txt`, `llms-full.txt`, sitemap, feeds, robots, `security.txt`, `humans.txt`, manifest, OpenSearch, search index, `build.json` | `llms.py`, from the route table in `routes.py` |
| CSS and JS | hand-written in `src/`, published content-hashed (`assets.py`); `theme.js` runs inline and `tools/playground/nginx.conf` allows it by hash |
| fonts | `fonts/` (Geist and JetBrains Mono, SIL OFL 1.1), made once by `fonts.py` |

Every number on a hand-written page carries a small source link to the file it is parsed from.

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
`llms.txt` ≤ 2 KB with every URL resolving; Markdown twins against their sources; sitemap equals the
indexable pages; feeds parse; `security.txt` expiry; hashed and referenced assets; CSS, JS, font and
HTML budgets; WCAG contrast of the colour tokens in both themes; nginx.conf headers; no legacy commands
or rule files and no sales wording on hand-written pages.
