# DCX-SITE-02: website perfect

Changes: `tools/website/build.py` (structured data, footer link, desktop wording), `tools/website/check.py` (offline JSON-LD validation: every block parsed, `@id` references resolve inside the page graph, required fields per type), regenerated `website/`.

Structured data: Person has url, sameAs (GitHub only), jobTitle Founder, worksFor org; Organization has founder; SoftwareApplication keeps author = Person; news pages emit BlogPosting with author = Person, publisher = org. `softwareVersion` comes from the single constant `VERSION` in build.py.

**Before publish: tag `v1.1.0` must be cut (only `v1.1.0-rc1` exists).** `VERSION = "1.1.0"` and the "Version 1.1.0" badge are only true after that.

1. [x] Version 1.1.0: DID (one constant `VERSION`, tools/website/build.py); depends on tag v1.1.0 being cut (OWNER/MAIN step E).
2. [x] Desktop preview 0.11: SOFTENED. Card and news text now say "Preview, source only: there is no download yet, so build it from the repository"; no download button. tools/website/build.py. Free-for-everyone wording kept (owner announcement 21 Oct).
3. [x] 36 full / 8 outline / 6 docs: DID, confirmed. README.md `languages:begin` block counts 36/8/6, and `target/release/dctx-cli languages --markdown` prints the same; a README test pins the block to the registry.
4. [x] Ed25519 verified plugins and index: DID, confirmed. Verifier crates/dctx-ed25519 (tests/policy.rs, tests/differential.rs rejection tests), plugin signature check crates/dctx-plugin-distribution/src/domain/signature.rs, bad-signature test `signature_failures_are_refused` in crates/dctx-plugin-distribution/tests/install.rs, index fixtures plugins/index/invalid/ used by tests/discovery.rs.
5. [x] 0 network calls: DID, confirmed. `scripts/release-check.sh --quick target/release/dctx-cli` printed PASS network (0 inet connect/send for all 16 commands incl. pack and mcp) and RELEASE-CHECK PASS on 6 Nov. Caveat: binary built 3 Nov.
6. [x] Benchmark table = Run 7: DID, confirmed. 140/353/29,340; 122/312/88,923; 96/238/197,203 equal docs/BENCHMARK-20261030.md Run 7. Tool-call column (105/70/91) equals 7x per-corpus calls in docs/research/bench-3way-20261030/tasks/results/run5-53f8b6b/tasks_headline.md (run 7 reuses run-5 task results per the doc).
7. [x] 33/33 stability, 6 Nov: DID, confirmed. docs/research/stability-20261105/REPORT.md (33/33 text, 33/33 JSON; measured 2026-11-06 on 0495f5f), same wording as README.md.
8. [x] Footer links: PARTLY OWNER. Local files exist and are linked via BLOB/REPO URLs: SECURITY.md, TRADEMARK.md, MAINTENANCE.md (page /maintenance/ is built from it). NOT verifiable offline: GitHub Issues enabled, and that SECURITY.md / TRADEMARK.md exist in the public mirror. OWNER-TO-CONFIRM after the mirror push (E/F); nothing contacted.
9. [x] tejl.hr: OWNER-TO-CONFIRM. Kept in the footer text and in the existing Organization sameAs (already present before this card); NOT added to Person sameAs. Owner must confirm tejl.hr is his and live, else drop it from Organization sameAs and the footer.
10. [x] Atom feed / og.png / sitemap: DID, verified. website/news/feed.xml and sitemap.xml parse as XML; website/assets/og.png is 1200x630; sitemap lists exactly the 8 indexable built pages (404 is noindex and excluded).
11. [x] Deny-list over website/: DID. Applied all 18 regexes of ops/publish/dctx/deny-list.txt to every file under website/: 0 hits.

No legacy behaviour was added. Nothing deployed, no external contact.
