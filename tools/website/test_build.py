"""Unit tests of the website builder, and the full isolated check.

    tools/website/.venv/bin/python -m unittest tools/website/test_build.py
"""
import os
import re
import subprocess
import sys
import tempfile
import unittest
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
        facts, routes = self.llms.facts_markdown(), self.llms.route_table()
        for t in self.cap["tiers"]:
            text = (ROOT / "website" / t["file"]).read_text()
            self.assertTrue(text.startswith("# dctx\n\n> "), t["file"])
            self.assertLess(text.index("## About this file"), text.index(facts), t["file"])
            self.assertLess(text.index(facts), text.index(routes), t["file"])
            self.assertIn(t["output"], text, t["file"])
            self.assertLess(text.index(routes), text.index(t["output"]), t["file"])
            self.assertIn(f"Engine: dctx {self.cap['version']}.", text)
            self.assertIn(f"Docs commit: {self.cap['docs_commit']} ({self.cap['docs_date']})", text)

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
