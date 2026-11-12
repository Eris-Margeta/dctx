# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Exercise deployment decisions with GitHub and git subprocesses stubbed."""

import json
import subprocess
import unittest
from unittest.mock import patch

import deploy_changed


SHA = "a" * 40
GH = [
    "gh", "run", "list", "--workflow", "deploy-site.yml",
    "--status", "success", "--branch", "main", "-L", "1", "--json", "headSha",
]
ANCESTOR = ["git", "merge-base", "--is-ancestor", SHA, "HEAD"]
DIFF = ["git", "diff", "--name-only", "-z", SHA, "HEAD", "--"]


class DeployChanged(unittest.TestCase):
    def decide(self, *, event="push", runs=None, ancestor=0, paths="", error=None):
        commands = []

        def output(command, *, text):
            self.assertTrue(text)
            commands.append(command)
            if command == GH:
                if error:
                    raise error
                return json.dumps(runs if runs is not None else [{"headSha": SHA}])
            self.assertEqual(command, DIFF)
            return paths

        def run(command, *, stderr):
            commands.append(command)
            self.assertEqual(command, ANCESTOR)
            self.assertEqual(stderr, subprocess.DEVNULL)
            return subprocess.CompletedProcess(command, ancestor)

        with patch.object(deploy_changed.subprocess, "check_output", side_effect=output), \
                patch.object(deploy_changed.subprocess, "run", side_effect=run):
            result = deploy_changed.changed(event)
        return result, commands

    def test_website_change_before_non_website_head_still_deploys(self):
        # The complete baseline..HEAD diff includes an earlier website commit.
        result, commands = self.decide(paths="website/index.html\0crates/example.rs\0")
        self.assertTrue(result)
        self.assertEqual(commands, [GH, ANCESTOR, DIFF])

    def test_ancestor_without_website_changes_skips(self):
        result, commands = self.decide(paths="crates/example.rs\0README.md\0")
        self.assertFalse(result)
        self.assertEqual(commands, [GH, ANCESTOR, DIFF])

    def test_no_changes_skips(self):
        self.assertFalse(self.decide()[0])

    def test_non_ancestor_deploys_without_diff(self):
        self.assertEqual(self.decide(ancestor=1), (True, [GH, ANCESTOR]))

    def test_no_successful_run_deploys_without_git(self):
        self.assertEqual(self.decide(runs=[]), (True, [GH]))

    def test_manual_dispatch_always_deploys_without_lookups(self):
        self.assertEqual(self.decide(event="workflow_dispatch"), (True, []))

    def test_api_failure_deploys(self):
        for error in (FileNotFoundError(), subprocess.CalledProcessError(1, GH)):
            with self.subTest(error=type(error).__name__):
                self.assertEqual(self.decide(error=error), (True, [GH]))

    def test_invalid_baseline_deploys(self):
        for runs in ([{}], [{"headSha": None}], [{"headSha": ""}], {"unexpected": []}):
            with self.subTest(runs=runs):
                self.assertEqual(self.decide(runs=runs), (True, [GH]))

    def test_unavailable_history_deploys(self):
        self.assertEqual(self.decide(ancestor=128), (True, [GH, ANCESTOR]))

    def test_diff_failure_deploys(self):
        with patch.object(deploy_changed.subprocess, "check_output", side_effect=[
            json.dumps([{"headSha": SHA}]), subprocess.CalledProcessError(1, DIFF),
        ]), patch.object(deploy_changed.subprocess, "run", return_value=subprocess.CompletedProcess(ANCESTOR, 0)):
            self.assertTrue(deploy_changed.changed("push"))

    def test_all_website_inputs_deploy(self):
        for path in (
            "tools/website/build.py", "docs/guide.md", "CHANGELOG.md",
            ".github/workflows/deploy-site.yml", "tools/ci/deploy_changed.py",
            "tools/ci/test_deploy_changed.py", "website/old-name.html",
        ):
            with self.subTest(path=path):
                self.assertTrue(self.decide(paths=f"{path}\0")[0])


if __name__ == "__main__":
    unittest.main(verbosity=2)
