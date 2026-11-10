"""Unit tests of the website builder, and the full isolated check.

    tools/website/.venv/bin/python -m unittest tools/website/test_build.py
"""
import os
import re
import subprocess
import sys
import tempfile
import unittest
from html.parser import HTMLParser
from unittest.mock import patch
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import data  # noqa: E402
import md  # noqa: E402
from common import ROOT, demo, slugify  # noqa: E402
from pages.news_content import NEWS  # noqa: E402
from routes import DOCS, build_routes, doc_path  # noqa: E402


class DeploymentCheck(unittest.TestCase):
    def test_working_tree_comparison_detects_drift_and_extra_files(self):
        import check

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            current, rebuilt = root / "website", root / "rebuilt"
            current.mkdir()
            rebuilt.mkdir()
            (current / "index.html").write_text("same")
            (rebuilt / "index.html").write_text("same")
            with patch.object(check, "ROOT", root), patch.object(check, "git") as git_mock:
                errors = []
                self.assertEqual(check.staleness(rebuilt, errors, working_tree=True), 1)
                self.assertEqual(errors, [])
                (current / "index.html").write_text("different")
                (current / "stale.txt").write_text("obsolete")
                (rebuilt / "new.txt").write_text("missing in deployment")
                check.staleness(rebuilt, errors, working_tree=True)
                self.assertEqual(len(errors), 3)
                git_mock.assert_not_called()


class PageScripts(unittest.TestCase):
    class Page(HTMLParser):
        def __init__(self, html):
            super().__init__()
            self.scripts = []
            self.builds = []
            self.has_sections = False
            self.feed(html)

        def handle_starttag(self, tag, attrs):
            attrs = dict(attrs)
            if tag == "script" and attrs.get("src"):
                self.scripts.append(attrs["src"])
            if tag == "meta" and attrs.get("name") == "dctx-build":
                self.builds.append(attrs.get("content", ""))
            if set(attrs.get("class", "").split()) & {"toc", "toc-m", "toc-row", "how-list"}:
                self.has_sections = True

    def test_every_section_navigation_loads_scroll_spy(self):
        source = (HERE / "src/site.js").read_text()
        spy = source[source.index("  /* Scroll-spy:"):source.index("  /* New version check:")]
        pages = sorted((ROOT / "website").rglob("*.html"))
        self.assertTrue(pages)
        covered = []
        for page in pages:
            parsed = self.Page(page.read_text())
            if parsed.has_sections:
                with self.subTest(page=page.relative_to(ROOT)):
                    scripts = [(ROOT / "website" / src.lstrip("/")).read_text() for src in parsed.scripts]
                    self.assertTrue(any(spy in script for script in scripts), "section navigation needs scroll-spy")
                covered.append(page.relative_to(ROOT / "website").as_posix())
        self.assertIn("docs/spec/index.html", covered)
        self.assertIn("cli/index.html", covered)
        self.assertIn("changelog/index.html", covered)
        self.assertIn("examples/index.html", covered)
        self.assertIn("how-it-works/index.html", covered)

    def test_every_html_page_has_build_meta_and_version_check(self):
        import json

        build_id = json.loads((ROOT / "website/build.json").read_text())["build_id"]
        self.assertRegex(build_id, r"^[0-9a-f]{64}$")
        source = (HERE / "src/site.js").read_text()
        version_check = source[source.index("  /* New version check:"):]
        pages = sorted((ROOT / "website").rglob("*.html"))
        self.assertTrue(pages)
        for page in pages:
            with self.subTest(page=page.relative_to(ROOT)):
                parsed = self.Page(page.read_text())
                self.assertEqual(parsed.builds, [build_id])
                scripts = [(ROOT / "website" / src.lstrip("/")).read_text() for src in parsed.scripts]
                self.assertTrue(any(version_check in script for script in scripts), "every page needs the version check")


class Routes(unittest.TestCase):
    def test_paths_are_unique_and_canonical(self):
        routes = build_routes(NEWS)
        paths = [r.path for r in routes]
        self.assertEqual(len(paths), len(set(paths)))
        for r in routes:
            self.assertTrue(r.path.startswith("/") and (r.path.endswith("/") or r.path.endswith(".html")), r.path)

    def test_every_document_exists(self):
        for slug, file, *_ in DOCS:
            self.assertTrue((ROOT / file).is_file(), file)
            self.assertEqual(doc_path(slug), "/why/" if slug == "why" else f"/docs/{slug}/")


class Markdown(unittest.TestCase):
    def test_github_slugs(self):
        self.assertEqual(slugify("Security & Privacy"), "security--privacy")
        self.assertEqual(slugify("2. The core belief: prompt-processing efficiency"), "2-the-core-belief-prompt-processing-efficiency")
        self.assertEqual(slugify("<code>dctx map</code> verbs"), "dctx-map-verbs")

    def test_link_rewriter(self):
        linker = md.Linker({"docs/SPEC.md": "/docs/spec/", "README.md": "/docs/readme/"})
        self.assertEqual(linker.resolve("README.md", "docs/SPEC.md#3-zero-rag-defects"), "/docs/spec/#3-zero-rag-defects")
        self.assertEqual(linker.resolve("docs/guides/MAP-MODE.md", "../../README.md"), "/docs/readme/")
        self.assertTrue(linker.resolve("README.md", "LICENSE").endswith("/blob/main/LICENSE"))
        self.assertTrue(linker.resolve("README.md", "legacy-c/").endswith("/tree/main/legacy-c"))
        self.assertEqual(linker.resolve("README.md", "https://www.dctx.internet.technology/"), "/")
        self.assertIsNone(linker.resolve("README.md", "https://img.shields.io/badge/x"))
        self.assertIsNone(linker.resolve("README.md", "no/such/file.md"))
        self.assertIsNone(linker.resolve("README.md", "../outside.md"))

    def test_strip_targets_ignores_only_link_targets(self):
        a = "See [the spec](docs/SPEC.md) and <a href=\"x\">y</a>.\n[ref]: docs/a.md\n"
        b = "See [the spec](https://example/docs/spec/) and <a href=\"z\">y</a>.\n[ref]: https://example/a\n"
        self.assertEqual(md.strip_targets(a), md.strip_targets(b))
        self.assertNotEqual(md.strip_targets(a), md.strip_targets(a.replace("the spec", "a spec")))

    def test_one_h1_per_rendered_document(self):
        html, _, title = md.to_html("# A\n\ntext\n\n# B\n\n## C\n", "x.md", md.Linker({}))
        self.assertEqual(html.count("<h1"), 1)
        self.assertEqual(title, "A")


class Facts(unittest.TestCase):
    """Numbers on the site equal the repository files they come from."""

    def test_table_parser(self):
        _, head, rows = data.md_tables("| a | b |\n|---|--:|\n| 1 | `x` |\n| 2 | **3** |\n")[0]
        self.assertEqual(head, ["a", "b"])
        self.assertEqual(rows, [["1", "`x`"], ["2", "**3**"]])

    def test_run7(self):
        r7 = data.run7()
        self.assertEqual(r7["points"], {"dctx": 140, "trail": 122, "graphify": 96})
        self.assertEqual(r7["tokens"], {"dctx": 29340, "trail": 88923, "graphify": 197203})
        lo, hi, n = r7["cold"]["dctx"]
        self.assertEqual((f"{lo:.2f}", f"{hi:.2f}", n), ("0.13", "1.20", 6))
        readme = (ROOT / "README.md").read_text()
        self.assertIn(f"took dctx {lo:.2f}–{hi:.2f} s", readme, "the README and the site agree on the cold index")

    def test_stability_languages_network(self):
        self.assertEqual((data.stability()["text"], data.stability()["json"]), ("33/33", "33/33"))
        langs = data.languages()
        self.assertEqual((langs["total"], langs["tiers"]), (50, {"full": 36, "outline": 8, "docs": 6}))
        self.assertEqual(data.network_zero()[0], 0)
        self.assertEqual(len(data.mcp_tools()), 10)

    def test_languages_capture_equals_readme(self):
        readme = (ROOT / "README.md").read_text()
        block = readme.split("<!-- languages:begin -->\n")[1].split("<!-- languages:end -->")[0]
        self.assertEqual(data.rec("languages-md")["stdout"].strip(), block.strip())


class Capture(unittest.TestCase):
    def test_pack_excerpt_is_verbatim_with_an_exact_omission_count(self):
        from output_examples import PACK_LINES, render

        records = demo()["records"]
        pack = next(r for r in records if r["id"] == "pack")["stdout"].splitlines()
        rendered = render(records)
        excerpt = rendered.split("$ dctx pack .\n", 1)[1].split("\n```", 1)[0].splitlines()
        self.assertEqual(excerpt[:-1], pack[:PACK_LINES])
        self.assertEqual(excerpt[-1], f"… ({len(pack) - PACK_LINES} more lines)")
        self.assertIn("<DIRECTORY_TREE>", excerpt)
        self.assertTrue(any(line.startswith('<FILE_CONTENT_START ID=') for line in excerpt))

    def test_readme_output_matches_the_shared_capture_renderer(self):
        from output_examples import render

        block = (ROOT / "README.md").read_text().split("<!-- output:begin -->\n", 1)[1].split("<!-- output:end -->", 1)[0]
        self.assertEqual(block, render(demo()["records"]))

    def test_records(self):
        d = demo()
        ids = [r["id"] for r in d["records"]]
        self.assertEqual(len(ids), len(set(ids)))
        self.assertGreaterEqual(len(ids), 30)
        for r in d["records"]:
            self.assertRegex(r["id"], r"^[a-z0-9-]+$")
            self.assertEqual(r["est_tokens"], -(-2 * r["bytes"] // 7))
            self.assertEqual(r["argv"][0], "dctx")
            self.assertNotIn("example-not-a-secret", r["stdout"] + r["stderr"])

    def test_ids_have_the_playground_shape(self):
        for r in demo()["records"]:
            for arg in r["argv"][1:]:
                if re.match(r"^[smdkp]:", arg):
                    self.assertRegex(arg, r"^[smdkp]:[^\0\n]{1,200}$")

    def test_env_is_listed_never_read(self):
        status = {f["path"]: f["status"] for f in demo()["sample"]["files"]}
        self.assertEqual(status[".env"], "listed-not-read")
        self.assertEqual(status["target/debug/kiosk.d"], "ignored")
        self.assertEqual(status["assets/logo.png"], "binary")


class FactsSource(unittest.TestCase):
    """data/facts.json is the one source of the notice, the personas and the evaluator facts."""

    def test_facts_load_and_personas_have_pages(self):
        import facts
        f = facts.load()
        self.assertTrue(f["notice"].startswith("dctx is an advanced tool with several ways to use it."))
        self.assertTrue(f["notice"].endswith("Measure before you change anything."))
        paths = {r.path for r in build_routes(NEWS)}
        self.assertEqual(len(f["personas"]), 4)
        for p in f["personas"]:
            self.assertIn(facts.persona_path(p["id"]), paths)
            self.assertIn(p["record"], [r["id"] for r in demo()["records"]], "persona runs are real captures")
        self.assertIn("/for-ai-evaluators/", paths)
        import llms
        self.assertIn(f["notice"], llms.facts_markdown(), "the llms files and the pages read the same facts")
        for doc in ("README.md", "docs/START.md"):
            self.assertIn(f["notice"], (ROOT / doc).read_text(), f"the notice is verbatim in {doc}")

    def test_animation_hook_is_optional(self):
        import importlib.machinery
        import types
        from pages import home
        with patch("importlib.util.find_spec", return_value=None):
            self.assertEqual(home.anim("two_scopes"), "")
        fake = types.ModuleType("pages.animations")
        fake.__spec__ = importlib.machinery.ModuleSpec("pages.animations", None)
        fake.two_scopes = lambda: "<svg></svg>"
        with patch.dict(sys.modules, {"pages.animations": fake}):
            self.assertEqual(home.anim("two_scopes"), '<div class="anim-slot" data-anim="two_scopes"><svg></svg></div>')
            self.assertEqual(home.anim("safe_path"), "")


class LlmsPipeline(unittest.TestCase):
    """llms-small.txt, llms.txt and llms-full.txt: facts block, route table, then the dctx binary's own output."""

    def setUp(self):
        import llms
        self.llms = llms
        self.cap = llms.llms_capture()

    def test_facts_match_the_repository(self):
        import json
        facts = self.llms.facts()
        version = (ROOT / "VERSION").read_text().strip()
        self.assertEqual(facts["version"], version)
        self.assertEqual(facts["release_tag"], f"v{version}")
        self.assertIn(f"## [{version}] - {facts['released']}", (ROOT / "CHANGELOG.md").read_text())
        self.assertTrue(facts["notice"].startswith("dctx is an advanced tool with several ways to use it. Used well, it cuts"))
        self.assertTrue(facts["notice"].endswith(" Measure before you change anything."))
        if "three-tool profile" not in facts["notice"]:  # interim wording until the profile and receipts ship
            self.assertIn("follow the safe path: one project, the system binary, the lean MCP profile.", facts["notice"])
        for item in facts["removed"]["items"]:
            for pattern in item["patterns"]:
                re.compile(pattern)
        self.assertEqual(json.loads((HERE / "data/facts.json").read_text())["schema"], 1)

    def test_every_file_is_facts_routes_then_the_capture(self):
        routes = build_routes(NEWS)
        pages = {r.path: (ROOT / "website" / (r.path.lstrip("/") + ("index.html" if r.path.endswith("/") else ""))).read_text()
                 for r in routes}
        for t in self.cap["tiers"]:
            name = t["file"]
            facts = self.llms.facts_for_file(name)
            table = self.llms.route_table(pages, routes, index=name == "llms.txt")
            text = (ROOT / "website" / t["file"]).read_text()
            self.assertTrue(text.startswith("# dctx\n\n> "), t["file"])
            about = "**About this file**" if name == "llms.txt" else "## About this file"
            self.assertLess(text.index(about), text.index(facts), t["file"])
            self.assertLess(text.index(facts), text.index(table), t["file"])
            if t["file"] != "llms.txt":
                from glyphs import plain
                self.assertIn(plain(t["output"]), text, t["file"])
                self.assertLess(text.index(table), text.index(plain(t["output"])), t["file"])
            self.assertIn(f"Engine: dctx {self.cap['version']}.", text)
            self.assertIn(f"Docs commit: {self.cap['docs_commit']} ({self.cap['docs_date']})", text)

    def test_index_shape_uses_only_h1_and_h2_file_lists(self):
        text = (ROOT / "website/llms.txt").read_text()
        self.assertTrue(text.startswith("# dctx\n\n> "))
        self.assertNotRegex(text, r"(?m)^#{3,} ")
        sections = re.split(r"(?m)^## .+\n", text)[1:]
        self.assertTrue(sections)
        for section in sections:
            for line in section.strip().splitlines():
                if line:
                    self.assertRegex(line, r"^- \[.+\]\(https://[^)]+\): .+")

    def test_stale_full_text_fails_freshness_gate(self):
        import check
        from routes import Route
        route = Route("/", "Home", "A description from HTML.", "home")
        page = ('<meta name="description" content="A description from HTML.">'
                '<main id="main"><h1>Home</h1>\n<p>Fresh prose that the previous build did not contain.</p></main>')
        with tempfile.TemporaryDirectory() as tmp:
            site = Path(tmp)
            (site / "index.html").write_text(page)
            pages = {"/": page}
            texts = {"llms.txt": self.llms.llms_txt(pages, [route]),
                     "llms-small.txt": self.llms.llms_small(pages, [route]),
                     "llms-full.txt": self.llms.llms_full(pages, [route])}
            for name, text in texts.items():
                (site / name).write_text(text)
            errors = []
            check.llms_fresh(site, errors, [route])
            self.assertEqual(errors, [])
            (site / "llms-full.txt").write_text(texts["llms-full.txt"].replace(
                "Fresh prose that the previous build did not contain.", "Old prose."))
            check.llms_fresh(site, errors, [route])
            self.assertTrue(any("missing extracted text from /" in error for error in errors), errors)
            for name, text in texts.items():
                for field in ("version", "released", "release_tag"):
                    (site / name).write_text(text.replace(self.llms.facts()[field], "stale"))
                    errors = []
                    check.llms_fresh(site, errors, [route])
                    self.assertTrue(any(f"{name} facts differ" in error for error in errors), errors)
                (site / name).write_text(text)
            (site / "unlisted.html").write_text(page)
            errors = []
            check.llms_fresh(site, errors, [route])
            self.assertTrue(any("/unlisted.html is missing from the route table" in error for error in errors), errors)

    def test_shared_extraction_and_packed_prose_deduplication(self):
        from routes import Route
        route = Route("/", "Home", "Ignored route description", "home", source="README.md")
        page = ('<meta name="description" content="Actual &amp; current description.">'
                '<main id="main"><nav>Navigation only</nav><h1>Home</h1>\n'
                '<p>Verbatim packed prose.</p>\n<p>Rendered <code>details</code> &amp; text.</p>'
                '<script>hidden()</script></main>')
        extracted = self.llms.extract_page(page)
        self.assertNotIn("Navigation", extracted["text"])
        self.assertNotIn("hidden", extracted["text"])
        self.assertIn("Rendered details & text.", extracted["text"])
        index = self.llms.search_index({"/": page}, [route])
        self.assertIn("Actual & current description.", index)
        text = self.llms.page_texts({"/": page}, [route], "Verbatim packed prose.")
        self.assertNotIn("Verbatim packed prose.", text)
        self.assertIn("[Documents](#documents)", text)
        self.assertIn("Rendered details & text.", text)

    def test_budget_shortens_only_supplemental_generated_listings(self):
        from routes import Route
        route = Route("/", "Home", "Description", "home")
        pages = {"/": '<meta name="description" content="Description"><main id="main">Page prose must stay.</main>'}
        full = self.llms.llms_full(pages, [route])
        with patch.dict(self.llms.LLMS_LIMIT, {"llms-full.txt": len(full.encode()) - 1}):
            shortened = self.llms.llms_full(pages, [route])
        self.assertIn("Page prose must stay.", shortened)
        self.assertIn(self.llms.tier("llms-full.txt")["output"], shortened)
        self.assertIn("the generated listing is available", shortened)
        self.assertLess(len(shortened.encode()), len(full.encode()))

    def test_tiers_respect_their_budgets(self):
        from routes import llms_tiers
        tiers = {name: (mode, budget, files) for name, mode, budget, files in llms_tiers()}
        self.assertEqual([t["file"] for t in self.cap["tiers"]], ["llms-small.txt", "llms.txt", "llms-full.txt"])
        for t in self.cap["tiers"]:
            mode, budget, files = tiers[t["file"]]
            self.assertEqual((t["mode"], t["budget"]), (mode, budget))
            self.assertEqual(sorted(t["files"] + t["dropped"]), sorted(files))
            self.assertLessEqual(t["est_tokens"], budget)
            self.assertEqual(t["est_tokens"], -(-2 * len(t["output"].encode()) // 7))
            self.assertTrue(all(c[0] == "dctx" for c in t["commands"]))
        self.assertLess(self.cap["tiers"][0]["bytes"], self.cap["tiers"][1]["bytes"])
        self.assertLess(self.cap["tiers"][1]["bytes"], self.cap["tiers"][2]["bytes"])

    def test_capture_is_current(self):
        self.assertEqual(self.llms.stale_sources(), [])

    def test_stale_sources_detects_an_edit(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for rel in self.cap["sources"]:
                (root / rel).parent.mkdir(parents=True, exist_ok=True)
                (root / rel).write_bytes((ROOT / rel).read_bytes())
            with patch.object(self.llms, "ROOT", root):
                self.assertEqual(self.llms.stale_sources(), [])
                (root / "README.md").write_text((ROOT / "README.md").read_text() + "\nedit\n")
                self.assertEqual(self.llms.stale_sources(), ["README.md"])

    def test_pack_body_uses_the_line_count(self):
        from capture import pack_body
        page = ('<FILE_CONTENT_START ID="F001" PATH="a.md" LINES="3">\nx\n</FILE_CONTENT_END ID="F001">\ny\n'
                '</FILE_CONTENT_END ID="F001">\n')
        self.assertEqual(pack_body(page, "a.md"), 'x\n</FILE_CONTENT_END ID="F001">\ny')
        self.assertIsNone(pack_body(page, "b.md"))

    def test_removed_commands_only_in_the_removed_list(self):
        import check
        ok = ("## Facts\n### Removed in 1.2.0\n- `dctx .` and `-d, --diff`: use pack\n### How to verify a claim\n"
              '<FILE_CONTENT_START ID="F001" PATH="CHANGELOG.md" LINES="1">\nrun dctx . -t\n</FILE_CONTENT_END ID="F001">\n'
              '<FILE_CONTENT_START ID="F002" PATH="docs/SPEC.md" LINES="1">\nthe legacy tree (`dctx . -t`)\n</FILE_CONTENT_END ID="F002">\n')
        errors = []
        check.removed_outside_list("t", ok, errors)
        self.assertEqual(errors, [])
        for bad in ("Usage: dctx [PATH] [OPTIONS]\n", "  -t, --tree   Tree only\n", "put rules in .dctxomitcontent\n",
                    '<FILE_CONTENT_START ID="F001" PATH="README.md" LINES="1">\n$ dctx .\n</FILE_CONTENT_END ID="F001">\n',
                    "### Removed in 1.2.0\n### How to verify a claim\ndctx cache gc --legacy\n"):
            errors = []
            check.removed_outside_list("t", bad, errors)
            self.assertEqual(len(errors), 1, bad)

    def test_pages_may_show_the_removed_list_in_a_marked_element(self):
        import check
        page = '<p>x</p><section data-facts="removed"><p><code>.legacyignore</code></p></section><p>y</p>'
        self.assertEqual(check.strip_removed_list(page), "<p>x</p><p>y</p>")

    @unittest.skipUnless(os.environ.get("DCTX_BIN"), "set DCTX_BIN to re-run the capture")
    def test_capture_is_byte_stable(self):
        import capture
        binary = str(Path(os.environ["DCTX_BIN"]).resolve())
        version = (ROOT / "VERSION").read_text().strip()
        a = capture.capture_llms(binary, version, self.cap)
        b = capture.capture_llms(binary, version, self.cap)
        self.assertEqual(a, b)
        self.assertEqual(a, self.cap)


class VectorAndSocialStandard(unittest.TestCase):
    def test_forbidden_scan_rejects_literals_and_browser_escapes(self):
        import check
        cases = {"a.html": "<p>&rarr; &#x2705;</p>", "b.css": r'a::after{content:"\2192"}',
                 "c.js": r'button.textContent="\u2192"', "d.js": r'x="\uD83D\uDE00"',
                 "e.js": r'x="\u{1f600}"', "f.md": chr(0x2190)}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name, content in cases.items():
                (root / name).write_text(content)
            errors = []
            check.forbidden_glyphs(root, errors)
            self.assertEqual(len(errors), len(cases), errors)
            for path in root.iterdir():
                path.unlink()
            (root / "ok.html").write_text('<p>“Quotes” — prose · … ©</p>')
            errors = []
            check.forbidden_glyphs(root, errors)
            self.assertEqual(errors, [])

    def test_vector_conversion_keeps_punctuation_and_names_the_symbol(self):
        import check
        from glyphs import vectorize
        result = vectorize('<p>Go &rarr; <code>' + chr(0x2705) + '</code> — … ©</p>')
        self.assertNotRegex(result, check.FORBIDDEN)
        self.assertEqual(result.count('<svg '), 2)
        self.assertIn('aria-label="-&gt;"', result)
        self.assertIn('stroke="currentColor"', result)
        self.assertIn('width="16" height="16"', result)
        self.assertIn('aria-hidden="true" focusable="false"', result)
        self.assertIn(' — … ©', result)

    def test_head_check_rejects_swapped_groups_missing_dimensions_and_shared_twitter(self):
        import check
        from assets import Assets
        import shell
        from routes import Route
        import hashlib
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            assets = Assets()
            assets.social = {"home": {kind: {"file": "home" + suffix}
                                      for kind, suffix in (("gif", ".gif"), ("png", ".png"), ("twitter", ".twitter.png"))}}
            for name in ("home.gif", "home.png", "home.twitter.png"):
                path = root / "assets/og" / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
                url = "/assets/og/" + name
                assets.urls[url] = url + "?v=" + hashlib.sha256(path.read_bytes()).hexdigest()[:12]
            og, twitter = shell.og_tags(Route("/", "Home", "Actual page copy", "home"), assets)
            parser = check.Doc()
            parser.feed('<head>' + og + '<meta name="twitter:card" content="summary_large_image">' + twitter + '</head>')
            errors = []
            check.check_social_head("test", parser.meta, root, errors)
            self.assertEqual(errors, [])
            for mutated in (parser.meta[5:10] + parser.meta[:5] + parser.meta[10:],
                            [m for m in parser.meta if m.get("property") != "og:image:height"],
                            [dict(m, content=assets.url('/assets/og/home.png')) if m.get('name') == 'twitter:image' else m
                             for m in parser.meta]):
                errors = []
                check.check_social_head("test", mutated, root, errors)
                self.assertTrue(errors)

    def test_image_checks_reject_wrong_size_and_nonmatching_frame_zero(self):
        import check
        from PIL import Image
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            folder = root / 'assets/og'
            folder.mkdir(parents=True)
            first = Image.new('RGB', (1200, 630), 'black')
            middle = first.copy()
            middle.putpixel((600, 300), (255, 255, 255))
            first.save(folder / 'test.gif', save_all=True, append_images=[middle], duration=1500, loop=0)
            first.save(folder / 'test.png')
            Image.new('RGB', (1200, 600)).save(folder / 'test.twitter.png')
            doc = check.Doc()
            doc.meta = [{'property': 'og:image', 'content': check.ORIGIN + '/assets/og/test.gif'}]
            errors = []
            check.check_social_images(root, {'test': doc}, errors, {})
            self.assertEqual(errors, [])
            Image.new('RGB', (1200, 630), 'red').save(folder / 'test.png')
            Image.new('RGB', (1200, 630)).save(folder / 'test.twitter.png')
            errors = []
            check.check_social_images(root, {'test': doc}, errors, {})
            self.assertTrue(any('frame 0 differs' in e for e in errors))
            self.assertTrue(any('1200, 600' in e for e in errors))


class SocialManifest(unittest.TestCase):
    def test_digest_binds_each_copy_geometry_and_source_input(self):
        from social_manifest import card_inputs, inputs_sha256
        from routes import Route
        inputs = card_inputs([Route("/test/", "Title", "Description", "page")], {"nodes": []})["test"]
        sources = (b"og source", b"js source", b"css source")
        original = inputs_sha256(inputs, sources)
        self.assertEqual(original, inputs_sha256(dict(reversed(list(inputs.items()))), sources))
        for key in ("title", "crumb", "desc", "seed", "geo"):
            changed = {**inputs, key: {"changed": True} if key == "geo" else 999 if key == "seed" else "changed"}
            self.assertNotEqual(original, inputs_sha256(changed, sources), key)
        for i in range(3):
            changed = list(sources)
            changed[i] += b"changed"
            self.assertNotEqual(original, inputs_sha256(inputs, tuple(changed)))
        self.assertNotEqual(inputs_sha256(inputs, (b"ab", b"c", b"d")),
                            inputs_sha256(inputs, (b"a", b"bc", b"d")))

    def test_manifest_rejects_missing_slug_stale_copy_and_corrupted_files(self):
        import json
        from social_manifest import KINDS, SocialStale, card_inputs, file_record, inputs_sha256, load_cards
        from routes import Route
        routes = [Route("/test/", "Title", "Description", "page")]
        geo = {"nodes": []}
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            manifest = folder / "manifest.json"
            entry = {"inputs_sha256": inputs_sha256(card_inputs(routes, geo)["test"])}
            for kind, suffix in KINDS:
                name = "test" + suffix
                content = kind.encode()
                (folder / name).write_bytes(content)
                entry[kind] = file_record(name, content)
            def load(value):
                manifest.write_text(json.dumps(value))
                return load_cards(routes, geo, manifest_path=manifest, folder=folder)
            self.assertEqual(len(load({"test": entry})[1]), 3)
            with self.assertRaisesRegex(SocialStale, "social images stale: run tools/website/social.py"):
                load({})
            with self.assertRaisesRegex(SocialStale, "inputs digest differs"):
                load({"test": {**entry, "inputs_sha256": "stale"}})
            for kind, suffix in KINDS:
                for field, value in (("sha256", "0" * 64), ("bytes", entry[kind]["bytes"] + 1), ("file", "../escape")):
                    with self.assertRaises(SocialStale, msg=f"{kind}.{field}"):
                        load({"test": {**entry, kind: {**entry[kind], field: value}}})
            (folder / "test.gif").write_bytes(b"corrupt")
            with self.assertRaisesRegex(SocialStale, "file sha256 or byte size differs"):
                load({"test": entry})
            (folder / "test.gif").unlink()
            with self.assertRaises(SocialStale):
                load({"test": entry})

    def test_stale_description_fails_before_writing_output(self):
        import shutil
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(HERE, root / "tools/website", ignore=shutil.ignore_patterns(".venv", "__pycache__"))
            shutil.copyfile(ROOT / "VERSION", root / "VERSION")
            # Preserve optional documents that affect the card set and seed order.
            for name in ("START.md", "WHY-DCTX.md"):
                if (ROOT / "docs" / name).is_file():
                    (root / "docs").mkdir(exist_ok=True)
                    shutil.copyfile(ROOT / "docs" / name, root / "docs" / name)
            routes = root / "tools/website/routes.py"
            content = routes.read_text()
            old = "Installation, quick start, map mode, language tiers and the CLI reference."
            self.assertIn(old, content)
            routes.write_text(content.replace(old, "Changed route description in scratch copy.", 1))
            output = root / "output"
            output.mkdir()
            sentinel = output / "keep.txt"
            sentinel.write_bytes(b"unchanged")
            result = subprocess.run([sys.executable, str(root / "tools/website/build.py"), "--out", str(output)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("social images stale: run tools/website/social.py", result.stderr)
            self.assertIn("docs-readme: inputs digest differs", result.stderr)
            self.assertEqual(list(output.iterdir()), [sentinel])
            self.assertEqual(sentinel.read_bytes(), b"unchanged")


class FullCheck(unittest.TestCase):
    def test_generated_website(self):
        binary = Path(os.environ.get("DCTX_BIN") or ROOT / "target/release/dctx-cli")
        args = ["--working-tree"]
        if not binary.is_file():
            args.append("--no-capture")
        result = subprocess.run([sys.executable, str(HERE / "check.py"), *args], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
