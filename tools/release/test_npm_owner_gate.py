# SPDX-License-Identifier: Apache-2.0
"""Check the npm publication boundary using workflow text only."""

import re
import unittest
from pathlib import Path

WORKFLOWS = Path(__file__).resolve().parents[2] / ".github" / "workflows"


def workflow_text(name):
    lines = (WORKFLOWS / name).read_text(encoding="utf-8").splitlines()
    return "\n".join(line for line in lines if not line.lstrip().startswith("#"))


class NpmOwnerGate(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.release = workflow_text("release.yml")
        cls.npm = workflow_text("npm.yml")

    def test_workflows_do_not_publish_npm_packages(self):
        for name, workflow in (("release.yml", self.release), ("npm.yml", self.npm)):
            with self.subTest(workflow=name):
                self.assertNotRegex(workflow, r"(?m)^  npm-publish:")
                self.assertNotRegex(workflow, r"\bnpm[ \t]+publish\b")

    def test_release_has_no_npm_credentials_or_outputs(self):
        self.assertNotRegex(self.release, r"\b(?:NPM_TOKEN|NODE_AUTH_TOKEN)\b")
        self.assertNotRegex(self.release, r"(?m)^      npm:|\bnpm=true\b|\boutputs\.npm\b")

    def test_independent_npm_workflow_checks_pack_and_parity(self):
        tests = re.search(r"(?m)^ +npm exec -- node --test (.+)$", self.npm)
        self.assertIsNotNone(tests, "npm workflow must execute the Node test suite")
        self.assertIn("packaging/npm/test/parity.test.cjs", tests[1].split())
        self.assertIn("packaging/npm/test/pack.test.cjs", tests[1].split())
        self.assertRegex(self.npm, r"(?m)^ +run: npm exec -- node packaging/npm/scripts/build-pack\.mjs$")

    def test_release_build_requires_ci_for_the_tagged_commit(self):
        preflight = re.search(r"(?ms)^  preflight:\n(.*?)(?=^  \S|\Z)", self.release)
        self.assertIsNotNone(preflight, "release workflow must have a preflight job")
        self.assertIn("ref: refs/tags/${{ env.RELEASE_TAG }}", preflight[1])
        self.assertIn(
            './tools/release/require-green-ci.sh "$(git rev-parse HEAD)" "$GITHUB_REPOSITORY"',
            preflight[1],
        )
        self.assertRegex(self.release, r"(?m)^  build:\n(?:^ {4,}[^\n]*\n)*?^    needs: preflight$")


if __name__ == "__main__":
    unittest.main()
