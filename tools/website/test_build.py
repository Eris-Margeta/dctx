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


class FullCheck(unittest.TestCase):
    def test_committed_website(self):
        binary = Path(os.environ.get("DCTX_BIN") or ROOT / "target/release/dctx-cli")
        args = [] if binary.is_file() else ["--no-capture"]
        result = subprocess.run([sys.executable, str(HERE / "check.py"), *args], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
