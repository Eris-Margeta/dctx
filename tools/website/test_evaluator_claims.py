"""Render discovery claims in memory without writing the published site."""

import html
import re
import unittest

import llms
from glyphs import plain
from pages import home, paths
from routes import build_routes


def visible(markup):
    return " ".join(html.unescape(re.sub(r"<[^>]+>", "", markup)).split())


class EvaluatorClaims(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bodies = {
            "/": home.body(),
            "/start/": paths.start(),
            "/start/library/": paths.persona("library"),
            "/for-ai-evaluators/": paths.evaluators()[0],
        }
        cls.routes = [route for route in build_routes([]) if route.path in cls.bodies]
        cls.pages = {
            route.path: f'<meta name="description" content="{html.escape(route.desc)}">'
            f'<main id="main">{cls.bodies[route.path]}</main>'
            for route in cls.routes
        }

    def one(self, pattern, block):
        matches = re.findall(pattern, block, re.S | re.M)
        self.assertEqual(len(matches), 1, f"Expected one match for {pattern!a}, got {len(matches)}")
        return matches[0]

    def has(self, pattern, entry):
        self.assertTrue(re.search(pattern, entry, re.I), f"Missing {pattern!a} in {entry!a}")

    def lacks(self, pattern, entry):
        self.assertFalse(re.search(pattern, entry, re.I), f"Unexpected {pattern!a} in {entry!a}")

    def section(self, body, heading):
        return self.one(rf'<section\b[^>]* aria-labelledby="{heading}">(.*?)</section>', body)

    def install_claim(self, entry):
        self.has(r"npm install @tejl/dctx@1\.3\.2 --save-exact", entry)
        self.has(r"Python (?:with|from) matching wrapper and platform CI wheels", entry)
        self.has(r"\bGo\b.*v1\.3\.2.*source checkout.*local .*replace.*go\.mod.*matching .*1\.3\.2.*engine.*DCTX_BIN", entry)
        self.lacks(r"Python and Go from CI|CI artifacts for Python and Go|Go (?:from|via) CI", entry)

    def scope_claim(self, entry):
        self.has(r"Project binary:.*application and its CI.*engine pinned by the project", entry)
        self.has(r"Node's npm package and Python's CI wheels.*node_modules or the virtualenv, respectively", entry)
        self.has(r"Go from a source checkout and local .*replace.*project supplies the matching engine", entry)
        self.has(r"never silently fall back to PATH", entry)

    def wrappers_claim(self, entry):
        self.has(r"Node on npm, Python wheels from CI, Go from source", entry)
        self.has(r"matching pinned .*@tejl/dctx-<platform>.* optional dependency", entry)
        self.install_claim(entry)

    def ci_claim(self, entry):
        self.has(r"pushes to main, pull requests and manual dispatch.*stage range and test filter", entry)
        self.has(r"each OS label .*ubuntu-24\.04.*macos-15\b.*macos-15-intel.*windows-2022.*with stable Rust and MSRV 1\.91 for eight jobs", entry)
        self.has(r"Linux test jobs use self-hosted dctx-fast runners for upstream pushes and GitHub-hosted runners for pull requests, fork runs and manual dispatch", entry)
        self.has(r"Linux quality job uses GitHub-hosted ubuntu-24\.04", entry)
        self.lacks(r"every push", entry)

    def benchmark_claims(self, entries):
        october, november, stability = (
            self.one(rf"^([^\n]*{label}[^\n]*)$", "\n".join(entries))
            for label in ("October 2026 run 7", "12 November 2026", "Byte stability")
        )
        self.has(r"historical.*70 task questions.*140/140 points.*353/354 gold facts", october)
        self.has(r"raw tool output without a model.*estimates tokens from stdout bytes", october)
        self.lacks(r"November|whole.session|140 (?:of 140 )?(?:task )?questions", october)
        self.has(r"measured.*whole.session.*claude-sonnet-5-5.*20 questions.*three repeats.*300 graded answers.*five arms", november)
        self.has(r"pack \(P\).*70\.91% median strict citation recall.*19\.34%.*grep plus file reads \(A\)", november)
        self.has(r"median list-price cost per question.*\$0\.195418 for P.*\$0\.179670 for A.*\+8\.8% for P", november)
        self.has(r"pricing uncached input, cache creation, cache reads and output separately", november)
        self.has(r"P met the recall requirement and missed the prespecified 15% cost reduction", november)
        self.has(r"measured.*Byte stability.*Linux only.*33 text answers.*33 JSON answers.*byte-identical", stability)
        model_free = r"without (?:an? )?(?:agent )?model|\bno (?:agent )?model\b|model[- ]free"
        for entry in entries:
            if entry != october:
                self.lacks(model_free, entry)

    def responsibility_claims(self, body):
        section = self.section(body, "resp-h")
        table = self.one(r'<table class="resp">(.*?)</table>', section)
        row = self.one(r"<tr><td><b>Project library</b></td>(.*?)</tr>", table)
        cells = re.findall(r"<td>(.*?)</td>", row, re.S)
        self.assertEqual(len(cells), 2)
        provides, owns = map(visible, cells)
        self.has(r"Rust facade from Git.*same output bytes as the CLI", provides)
        self.has(r"Node on npm and Python CI wheels offer pinned native binaries", provides)
        self.has(r"Go .*source checkout with local replace", provides)
        self.has(r"supplying the matching engine for Go", owns)

    def test_evaluator(self):
        body = self.bodies["/for-ai-evaluators/"]
        modalities = self.section(body, "mod-h")
        wrapper = self.one(r"<li>(<a [^>]*><b>Python, Node and Go wrappers</b>.*?)</li>", modalities)
        self.wrappers_claim(visible(wrapper))
        self.scope_claim(visible(self.one(r"<li>(Project binary:.*?)</li>", modalities)))
        benchmarks = self.section(body, "bench-h")
        self.benchmark_claims([visible(entry) for entry in re.findall(r"<li>(.*?)</li>", benchmarks)])
        verify = self.section(body, "verify-h")
        self.ci_claim(visible(self.one(r">CI</a></td><td>(.*?)</td>", verify)))
        self.responsibility_claims(body)

    def test_start(self):
        body = self.bodies["/start/"]
        card = self.one(r'<li class="path-card" data-path="library">(.*?)</details>', body)
        self.install_claim(visible(self.one(r'<dd class="path-mod">(.*?)</dd>', card)))
        ways = self.one(r'<figure class="ways">(.*?)</figure>', body)
        library_way = self.one(r"(<b>Library and wrappers</b>.*?)</li>", ways)
        status = self.one(r'<p class="way-st">(.*?)</p>', library_way)
        self.has(r"Rust facade from Git, Node 1\.3\.2 on npm, Python CI wheels, Go source checkout with local replace", visible(status))
        scope = self.section(body, "scopes-h")
        self.scope_claim(visible(self.one(r"<li>(Project binary:.*?)</li>", scope)))
        self.responsibility_claims(body)

    def test_library_path(self):
        body = self.bodies["/start/library/"]
        self.install_claim(visible(self.one(r"<dt>modality</dt><dd>(.*?)</dd>", body)))

    def test_home_install(self):
        body = self.bodies["/"]
        install = self.section(body, "install-h")
        project = self.one(r"<h3>Project libraries[^\n]*</h3>\s*<p>(.*?)</p>", install)
        self.install_claim(visible(project))
        self.scope_claim(visible(self.one(r"<li>(Project binary:.*?)</li>", install)))
        self.responsibility_claims(body)

    def test_llms_feeds(self):
        for render, opening, closing in (
            (llms.llms_txt, "**Facts**", "## Overview"),
            (llms.llms_small, "## Facts", "## Pages"),
            (llms.llms_full, "## Facts", "## Pages"),
        ):
            with self.subTest(feed=render.__name__):
                feed = plain(render(self.pages, self.routes))
                _, start, after_start = feed.partition(f"\n{opening}\n")
                block, end, _ = after_start.partition(f"\n{closing}\n")
                self.assertTrue(start and end, "Missing facts block boundaries")
                self.wrappers_claim(self.one(r"^- (Python, Node and Go wrappers[^\n]*)$", block))
                self.scope_claim(self.one(r"^- (Project binary:[^\n]*)$", block))
                benchmarks = self.one(r"^[^\n]*Benchmarks(?:\*\*)?\n(.*?)^[^\n]*Removed in ", block)
                self.benchmark_claims(re.findall(r"^- ([^\n]*)$", benchmarks, re.M))
                self.ci_claim(self.one(r"^- CI: ([^\n]*)$", block))

    def test_failure_escapes_hostile_rendered_text(self):
        hostile = 'outside\n\x1b[31m"value"'
        with self.assertRaises(AssertionError) as failure:
            self.has("expected", hostile)
        message = str(failure.exception)
        self.assertTrue(ascii(hostile) in message and message.isprintable(), ascii(message))


if __name__ == "__main__":
    unittest.main()
