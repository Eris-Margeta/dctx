# SPDX-License-Identifier: Apache-2.0
"""Check the repository's wrapper guidance and the README shipped by npm."""

import html
import json
import re
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path

import md
from common import ROOT, read
from pages import animations, home, tour
from routes import DOCS, doc_path


SHIM = "packaging/npm/dctx"
PLATFORMS = ("linux-x64", "linux-arm64", "darwin-x64", "darwin-arm64", "win32-x64")
ROUND_TWO = (
    "70.91% median strict citation recall versus 19.34%",
    "$0.195418 versus $0.179670",
    "(+8.8%)",
    "missed the prespecified target of at least 15% lower cost per question",
)
WRAPPERS = ("checkout", "replacement", "matching", "executable", "CI wheel artifacts",
            "npm install @tejl/dctx", "five pinned optional native packages")


def visible(markup):
    return " ".join(html.unescape(re.sub(r"<[^>]+>", " ", markup)).split())


def pack_failure(result):
    return (f"npm pack exited {result.returncode}; "
            f"stdout={json.dumps(result.stdout)} stderr={json.dumps(result.stderr)}")


class InstallGuidance(unittest.TestCase):
    def contains(self, text, *fragments):
        for fragment in fragments:
            self.assertTrue(fragment in text, f"missing guidance {json.dumps(fragment)}")

    def excludes_old_guidance(self, text):
        for pattern in (r"\bfrom (?:version )?1\.3\.1\b",
                        r"CI artifacts for Python and Go",
                        r"Python and Go wrappers from CI",
                        r"Under ~200 files, use grep",
                        r"dctx pays off"):
            self.assertFalse(re.search(pattern, text) is not None, f"stale guidance matching {pattern!a}")

    def test_root_readme_and_benchmark_source(self):
        readme = read("README.md")
        install = readme.split("## Installation\n", 1)[1].split("<!-- output:begin -->", 1)[0]
        self.contains(install, "checkout and a local `go.mod` replacement", "`DCTX_BIN`",
                      "matching release executable", "CI wheel artifacts", "npm install @tejl/dctx",
                      "five optional native")
        generated = readme.split("<!-- bench:begin -->", 1)[1].split("<!-- bench:end -->", 1)[0]
        report = read("docs/BENCHMARK-20261111.md")
        for text in (generated, report):
            self.contains(text, *ROUND_TWO, "As a heuristic", "under about 200 files",
                          "A shared dctx index may pay off")
        self.contains(report, "did not measure a repository-size crossover",
                      "pilot missed its separate prespecified threshold of a 25% reduction")
        self.excludes_old_guidance(readme)

    def test_rendered_guides(self):
        routes = {source: doc_path(slug) for slug, source, *_ in DOCS}
        start = visible(md.render("docs/START.md", md.Linker(routes)).html)
        modalities = visible(md.render("docs/MODALITIES.md", md.Linker(routes)).html)
        index = visible(md.render("docs/README.md", md.Linker(routes)).html)
        self.contains(start, *WRAPPERS, "builds a fresh index on first use",
                      "leaves the old unscoped files untouched", "Patch releases in the same family share an index",
                      "CLI cache commands target the current engine namespace")
        self.contains(modalities, "local replace", "go.mod", "packages/go", "DCTX_BIN",
                      "version-matched executable", "CI wheel artifacts", "npm install @tejl/dctx",
                      "optional native packages at its own version")
        self.contains(index, "Go from a checkout with a local module replacement",
                      "Python from CI wheel artifacts", "five pinned optional native packages")
        for text in (start, modalities, index):
            self.excludes_old_guidance(text)

    def test_rendered_home(self):
        markup = home.body()
        hero = visible(markup.split('<section class="hero"', 1)[1].split("</section>", 1)[0])
        self.contains(hero, "As a heuristic", "under about 200 files", "agents can reuse one index",
                      "Measure the payoff on your own workload", *WRAPPERS)
        self.contains(visible(markup), *ROUND_TWO, "Go checkout, Python wheels, Node on npm")
        # The generated facts also appear on home. Check the owned tour and
        # animation copy without requiring any revision to those facts.
        library_tour = visible(tour.tour())
        scopes = visible(animations.two_scopes())
        for text in (hero, library_tour, scopes):
            self.contains(text, *WRAPPERS)
            self.excludes_old_guidance(text)

    def test_packaged_shim_readme(self):
        try:
            with tempfile.TemporaryDirectory(prefix=".dctx-install-guide-", dir=ROOT) as tmp:
                result = subprocess.run(
                    ["npm", "pack", str(ROOT / SHIM), "--offline", "--ignore-scripts",
                     "--pack-destination", tmp, "--logs-max=0", "--no-update-notifier"],
                    cwd=tmp, capture_output=True, text=True, encoding="utf-8", errors="replace",
                    timeout=30, check=False,
                )
                if result.returncode:
                    self.fail(pack_failure(result))
                tarballs = list(Path(tmp).glob("*.tgz"))
                self.assertEqual(len(tarballs), 1)
                with tarfile.open(tarballs[0], "r:gz") as archive:
                    readme = archive.extractfile("package/README.md")
                    self.assertIsNotNone(readme)
                    with readme:
                        shipped = readme.read().decode("utf-8")
        except (OSError, subprocess.TimeoutExpired, tarfile.TarError, KeyError, UnicodeError) as exc:
            self.fail(f"Could not check the packed npm README: {json.dumps(str(exc))}")
        self.assertTrue(shipped == read(f"{SHIM}/README.md"), "packed README differs from shim source")
        self.contains(shipped, "npm install @tejl/dctx", "pins five optional native packages at its own version",
                      "matching native platform package by default",
                      *(f"`@tejl/dctx-{platform}`" for platform in PLATFORMS))
        self.excludes_old_guidance(shipped)

    def test_packaging_guide_and_release_history(self):
        packaging = " ".join(read("packaging/npm/README.md").split())
        self.contains(packaging, "npm install @tejl/dctx", "optional native packages at its own version")
        self.excludes_old_guidance(packaging)
        history = read("CHANGELOG.md")
        current = history.split("## [1.3.2]", 1)[1].split("## [1.3.1]", 1)[0]
        previous = history.split("## [1.3.1]", 1)[1].split("## [1.3.0]", 1)[0]
        self.contains(current, "available as `@tejl/dctx`", "five pinned optional")
        self.contains(previous, "registry refused the unscoped name `dctx`")
        self.assertFalse("is published" in previous, "release notes imply a scoped publication date")

    def test_pack_failure_escapes_subprocess_output(self):
        hostile = 'bad\n\x1b[31m"quoted"'
        result = subprocess.CompletedProcess(["npm"], 1, hostile, hostile)
        diagnostic = pack_failure(result)
        escaped = r'"bad\n\u001b[31m\"quoted\""'
        self.assertEqual(diagnostic, f"npm pack exited 1; stdout={escaped} stderr={escaped}")


if __name__ == "__main__":
    unittest.main()
