"""Render the historical release post and its benchmark destination in memory."""

import json
import re
import runpy
import unittest
from unittest.mock import Mock, patch

import common
import md
import shell
from pages import misc, news_content
from routes import build_routes


POST_PATH = "/news/dctx-1-1/"
RELEASE_DATE = "2026-11-06"
SUMMARY = (
    "dctx 1.1.0 adds Perl, Fortran and Verilog to map mode and makes dctx pack "
    "prefix-stable. This post records the benchmark and byte-stability results."
)


class NewsHistory(unittest.TestCase):
    def assert_contains(self, output, snippet):
        self.assertTrue(snippet in output, f"Missing rendered text {json.dumps(snippet)}")

    def test_release_post_under_two_site_versions(self):
        assets = Mock(theme_inline="", url=lambda name: f"/assets/{name}")
        for version in ("1.3.2", "9.8.7"):
            with self.subTest(version=json.dumps(version)):
                # Execute the content module afresh so import-time version interpolation is tested.
                with patch.object(common, "VERSION", version):
                    news = runpy.run_path(news_content.__file__)["NEWS"]
                route = next(route for route in build_routes(news) if route.path == POST_PATH)
                post = route.extra["post"]
                body, graph = misc.news_post(post)
                self.assert_contains(body, "<code>v1.1.0-rc1</code>) came out on 4 November 2026")
                self.assert_contains(body, "<code>v1.1.0</code>) followed on 6 November 2026")
                self.assert_contains(body, "on three independently selected repositories")
                self.assert_contains(body, "22 of 28 task points and retrieved 31 of 36 facts")
                self.assertFalse("pending" in body.lower(), "The post still calls the result pending")
                self.assertFalse(version in body, f"The post repeats site version {json.dumps(version)}")
                self.assert_contains(body, f'<time datetime="{RELEASE_DATE}">6 November 2026</time>')
                self.assert_contains(body, f'<p class="lede">{SUMMARY}</p>')

                with patch.object(shell, "VERSION", version), patch.object(shell, "og_tags", return_value=("", "")):
                    page = shell.page(route, body, assets, graph=graph, parents=[("/news/", "News")],
                                      og_type="article", article_times=(post["date"], None))
                self.assert_contains(page, f'<span class="ver">{version}</span>')
                self.assert_contains(page, f'<meta property="article:published_time" content="{RELEASE_DATE}">')
                self.assert_contains(page, f'<meta name="description" content="{SUMMARY}">')
                article = next(node for node in graph if node["@type"] == "BlogPosting")
                self.assertEqual(json.dumps(article["datePublished"]), json.dumps(RELEASE_DATE))
                self.assertEqual(json.dumps(article["description"]), json.dumps(SUMMARY))

                with patch.object(misc, "NEWS", news):
                    index = misc.news_index()
                self.assert_contains(index, f'<li><p class="meta"><time datetime="{RELEASE_DATE}">6 November 2026</time>'
                                     f'<span class="tag">Release</span></p><h2><a href="{POST_PATH}">'
                                     f'{common.e(route.title)}</a></h2><p>{SUMMARY}</p></li>')

    def result_destination(self, href, routes):
        path, _, fragment = href.partition("#")
        if path not in routes:
            self.fail(f"Missing result route for {json.dumps(href)}")
        return routes[path], fragment

    def test_result_link_resolves_to_the_rendered_october_report_heading(self):
        routes = {route.path: route for route in build_routes(news_content.NEWS)}
        body, _ = misc.news_post(routes[POST_PATH].extra["post"])
        link = re.search(r'<a href="([^"]*)">31 October 2026 independent benchmark</a>', body)
        self.assertIsNotNone(link, "The post needs a dated independent benchmark link")
        target, fragment = self.result_destination(link[1], routes)
        self.assertEqual(json.dumps((target.kind, target.source)),
                         json.dumps(("doc", "docs/BENCHMARK-20261030.md")))

        source_routes = {route.source: route.path for route in routes.values() if route.source}
        report = md.render(target.source, md.Linker(source_routes))
        heading = f'<h2 id="{fragment}">Independent benchmark, 31.10.2026,'
        self.assert_contains(report.html, heading)
        section = report.html.split(heading, 1)[1].split("<h2", 1)[0]
        self.assert_contains(section, "dctx 22/28 points, 31/36 facts")

    def test_missing_result_route_diagnostic_escapes_the_href(self):
        href = '/missing\n\x1b"route/#result'
        with self.assertRaises(AssertionError) as error:
            self.result_destination(href, {})
        expected = 'Missing result route for "/missing\\n\\u001b\\"route/#result"'
        self.assertEqual(json.dumps(str(error.exception)), json.dumps(expected))


if __name__ == "__main__":
    unittest.main()
