# DCX-WEB-V2: the dctx website, version 2

Branch `card/DCX-WEB-V2` on tag `v1.1.0`. Not deployed: the deploy runbook is
[`tools/playground/DEPLOY.md`](../../tools/playground/DEPLOY.md). Everything below was run on the build host
against `127.0.0.1` only.

## Gates

| Gate | Result |
|---|---|
| `tools/website/check.py` with `DCTX_BIN` and the publish deny-list | `PASS: 0 errors`; 156 files, 35 pages, 19 Markdown twins compared with their sources; `deny-list hits: 0` |
| Clean `git archive HEAD` build, diffed with the committed `website/` | byte-identical (`diff -r`, 156 files) |
| `capture.py` twice, and with the static musl binary | `data/demo.json` byte-identical each time (40 records) |
| Capture drift negative test (one record edited) | `check.py` fails: `capture drift: … differs from data/demo.json in find` |
| `python -m unittest tools/website/test_build.py` | 14 tests OK (route table, slugs, link rewriter, table parser, parsed benchmark cells, capture records, full check) |
| `cargo test` in `tools/playground` | 43 tests OK (24 unit, 19 HTTP); `cargo clippy --all-targets -D warnings` clean |
| `tools/playground/smoke.sh` on the real binary, `SMOKE_ALL=1` | 17 of 17; all 20 recorded examples return the same bytes live |
| `tools/website/browser/e2e.py` (headless Chromium 1140) | 27 of 27, plus the offline check with the service stopped |

## Owner acceptance checklist

1. **check.py PASS on a clean archive; CI with the capture-drift check.** Met for check.py (above). CI: the
   steps `Website check` and `Playground tests` are added to `.github/workflows/ci.yml` but were not run, since
   the CI runner was not contacted; the runner's Python needs `tools/website/requirements.txt` (set
   `DCTX_SITE_PYTHON` to that interpreter). **Partly met: CI not run.**
2. **Hero animation, real data, terminal, replay, motion toggle, reduced motion, no JS.** Met. The canvas scene
   draws the sample's 13 files, the policy step (13 on disk, 11 listed, 10 read; `.env` name only, `target/`
   ignored, the PNG binary), the 41 nodes and 13 call/import edges of `map export`, `find greet` lighting
   `s:src/util.rs#greet` and its callers, then the 25 lines of `pack --for` flying into a frame with
   `body_tokens=223 budget=600 dropped=0`. Then the terminal types four captured commands (51 lines). e2e:
   scene advances, terminal 0 → 51 of 51 lines, replay restarts, the toggle turns motion off and is remembered,
   reduced motion gives the final frame and the full transcript, JS off shows the poster and the transcript.
   Frame time with `?perf`: median 0.7–1.2 ms, p95 1.0–1.7 ms (headless, software rendering).
3. **Every command and output produced by dctx 1.1.0 at build time.** Met: `data/demo.json` from
   `capture.py`; `--version` must start with `dctx 1.1.0`; drift fails. Three values are scrubbed and labelled
   where shown: the commit hash in `--version`, `MOD:` times, `age_s`.
4. **Templates page: both sources, real output, tree diff.** Met: `/templates/`. `pack` has no option that
   selects a compiled template in 1.1.0 (`crates/dctx-cli/src/pack.rs`; the built-ins are chosen in
   `crates/dctx-template/src/legacy_output.rs`), so no custom template is shown, and the page says so.
5. **Playground: health, same bytes, pasted files, no-JS form, limits, container, site usable without it.**
   Met on the binary: health OK; all 20 recorded examples identical live; pasted files; `/api/v1/run-form` with
   JS off (e2e); 400 for 25 of 25 bad commands, 413 for a 600 KB body, 429 after the burst, 503 when the queue is
   full (20 of 30 parallel runs with a queue of 1); a 3 s run is killed; idle RSS 820 kB. Backend down: the page
   shows the offline notice and the recorded runs, and a form post gets the 503 offline page. The container
   (`cap_drop: ALL`, read-only, uid 65532, 192 MiB) is configured in `compose.yaml` and `Dockerfile.runtime` but
   was not built here (no Docker on the build host). **Partly met: image not built or measured.**
6. **Fonts self-hosted with licences; no external requests.** Met: Geist 400/600 and JetBrains Mono 400/500
   (10–23 KB each), `assets/fonts/OFL-*.txt`; the browser loaded only same-origin resources; check.py rejects any
   external resource.
7. **Density; no sales copy.** Met: 60 data-bearing items above the fold at 1440×900 (6 chips, 4 steps,
   5 facts, 3 quick-start lines, the install line, 17 terminal lines, 8 nav items, 16 scene labels;
   `tools/website/browser/density.py`); body 15 px; tables, not cards. check.py finds no `free forever`,
   `enterprise`, `pricing`, `trusted by` on any hand-written page (rendered repository documents are exempt;
   README and the specification use "enterprise" about readiness).
8. **llms files, twins, machine files.** Met: `llms.txt` 1,770 bytes, every URL resolves; `llms-small.txt`;
   `llms-full.txt` 345,067 bytes with every document twin; `index.md` and `rel=alternate` on every document;
   sitemap, robots (AI crawlers allowed), two Atom feeds, `security.txt`, `humans.txt`, `build.json`, manifest,
   icons, OpenSearch, `/search/`. nginx serves `.txt` and `.md` as text with UTF-8.
9. **OG images.** Met: every page has a dark and a light PNG (33 cards, 66 files, 1200×630, largest 113,506 bytes; the offline page shares the playground card, 404 the home card), home GIF first
   (24 frames, 859,844 bytes), `twitter:image` static, alt text everywhere.
10. **JSON-LD and footer.** Met: check.py validates every page's graph: exactly one Organization (TEJL,
    tejl.com, sameAs tejl.hr), Person (Eris Margeta, tejl.com, sameAs GitHub, Founder, worksFor), WebSite and
    SoftwareApplication (`softwareVersion` 1.1.0, author the Person, no `downloadUrl`); TechArticle, Dataset,
    WebAPI, SoftwareSourceCode, BlogPosting, BreadcrumbList. Footer text unchanged, TEJL logo, tejl.com and tejl.hr.
11. **Budgets, CSP, headers, contrast, a11y.** Met: first visit to `/` is 8 requests and 187,779 bytes; CSS
    30,193 bytes; JS sources 29,346 bytes (largest page bundle 20,074; `hero.js` 12,700, `term.js` 3,137); one
    inline script, the theme script, allowed by its hash in the CSP; HSTS, nosniff, Referrer-Policy,
    Permissions-Policy in `nginx.conf`; contrast 6.17–18.31:1 in both themes; one h1, no skipped levels, sized
    images, no inline styles or handlers.
12. **Deny-list 0; no legacy commands taught; allowlisted links only.** Met. The repository documents were fixed
    instead of edited on the site: README, SPEC and ADR-014 no longer teach the legacy snapshot commands or the
    legacy omit-content rule file, and the two evidence reports use neutral host names. `/cli/` prints
    `dctx --help` verbatim, which still lists the legacy snapshot options.
13. **Deploy runbook executed by the operator.** Not run here by design: `tools/playground/DEPLOY.md`.
    **Not met yet.**

## Loading, against the reference sites

Measured with `tools/website/browser/measure.py` (cache disabled, 1440×900). The reference sites were measured
over the internet, this site from a local server without compression, so the times are not comparable; requests
and bytes are.

| Page | Requests | Bytes | FCP | CLS |
|---|---:|---:|---:|---:|
| www.internet.technology | 40 | 396,189 | 774 ms | 0 |
| www.rural.internet.technology | 8 | 371,670 | 518 ms | 0 |
| dctx `/` (local) | 9 (8 + favicon) | 192,110 | 206 ms | 0 |
| dctx `/docs/spec/` (local) | 7 | 220,150 | 298 ms | 0 |

With the web fonts held back 0.8 s each (worst case), CLS is 0 on all 35 sitemap pages at 1440 and 390 px
(`tools/website/browser/slowfont.py` and a sweep of every page; before the fix: 0.36 on `/cli/` at 1440 px, up to
0.22 on document pages at 390 px). Two changes: the fonts use `font-display: optional` (all four are preloaded,
so at normal load they are on the first paint; a font that misses it is not swapped in, so text never moves), and
the monospace fallback's `size-adjust` is 100.1 % instead of 99.66 %, so Linux Chromium rounds the fallback to
the same whole-pixel ppem and advance as JetBrains Mono at 12.5 px. At normal load CLS is 0 on every page
measured and all four web fonts report `loaded`.

## Decisions taken while building

- Cold index on the home page is the run 7 figure, 0.13–1.20 s (run 7 `aggregate.json`, as in the README and
  the 1.1 news post), not the run 1 figure 0.16–1.16 s, so that it matches the 140/140 of the same run.
  `/benchmarks/` shows both, labelled.
- The README caveat calls the independent benchmark pending, but BENCHMARK-20261030.md has its result; the
  site shows that section verbatim and cites its numbers.
- Four font files instead of seven (fewer requests); 500 maps to 400, 700 to 600.
- `build.json` carries a digest of every input file, not a commit hash (a committed file cannot hold its own
  commit's hash, and the public mirror rewrites hashes). DEPLOY.md compares the digest.
- The hero poster is a dedicated scene image (`assets/og/scene.*.png`), not the OG card, so the page does not
  repeat its headline inside the picture.
- The playground's static musl `dctx-cli` is built with `zig cc` as the musl C compiler (sqlite); the Dockerfile
  uses `musl-tools`.
- `/why/` (docs/WHY-DCTX.md, written on its own card) is wired: route, home link, first docs item, all three
  llms files. It appears on the first build after that file lands; checked with a placeholder in a scratch copy
  (check.py PASS there).
- The canonical host is `https://www.dctx.internet.technology`.
- `/install.sh` is not published (default off).

## Sources of the numbers

| Number on the site | Source |
|---|---|
| 140/140, 122, 96, 29,340 / 88,923 / 197,203 tokens | `docs/BENCHMARK-20261030.md`, Run 7 table (parsed by `data.run7`) |
| 0.13–1.20 s on 6 corpora (Trail-Code 1.04–52.83 s, graphify 4.59–59.56 s) | `docs/research/bench-3way-20261030/results/run7-53f8b6b/aggregate.json` |
| 22/28 on the independent protocol | `docs/BENCHMARK-20261030.md`, section "Independent benchmark" |
| 33/33 text, 33/33 JSON | `docs/research/stability-20261105/REPORT.md`, first table |
| 0 socket attempts | `docs/BENCHMARK-20261030.md`, headline table, Network row |
| 50 languages, 36/8/6 | `dctx languages --markdown` capture (equals the README block; tested) |
| 10 MCP tools | `tools/list` capture |
| 13 files, 11 listed, 10 read, 41 nodes, 13 edges, 223 of 600 tokens | `tools/website/data/demo.json` |
| Every byte and token count | the capture; tokens are `ceil(2 × bytes / 7)`, as `dctx --help` prints |
