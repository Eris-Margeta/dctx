# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Replay redacted GitHub REST response fixtures without network access."""

import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("deploy_baseline", ROOT / "deploy-baseline.py")
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)
FIXTURES = ROOT / "deploy-baseline-fixtures"


class DeployBaseline(unittest.TestCase):
    def replay(self, name):
        fixture = json.loads((FIXTURES / f"{name}.json").read_text())
        responses = iter(fixture["responses"])

        def gh(command, *, text):
            response = next(responses)
            self.assertTrue(text)
            self.assertEqual(command[:6], ["gh", "api", "--method", "GET", "--paginate", "--slurp"])
            self.assertEqual(command[6], response["endpoint"])
            self.assertEqual(command[7::2], ["-f"] * len(command[8::2]))
            self.assertEqual(dict(field.split("=", 1) for field in command[8::2]), response["fields"])
            if "error" in response:
                raise subprocess.CalledProcessError(response["error"], command)
            return json.dumps(response["pages"])

        with patch.object(selector.subprocess, "check_output", side_effect=gh):
            self.assertEqual(selector.baseline(fixture["branch"]), fixture["expected"])
        self.assertEqual(list(responses), [], "expected API calls were not made")

    def test_ci_called_deploy_success(self):
        self.replay("ci-success")

    def test_manual_deploy_success(self):
        self.replay("manual-success")

    def test_success_with_skipped_upload_uses_older_deploy(self):
        self.replay("skipped-deploy")

    def test_no_runs(self):
        self.replay("no-runs")

    def test_runs_api_error_fails_open(self):
        self.replay("runs-api-error")

    def test_jobs_api_error_does_not_use_older_deploy(self):
        self.replay("jobs-api-error")

    def test_non_default_branch_and_unrelated_workflow_are_ignored(self):
        self.replay("branch-and-workflow")

    def test_only_skipped_uploads_fail_open(self):
        fixture = json.loads((FIXTURES / "skipped-deploy.json").read_text())
        runs, skipped_jobs = [response["pages"] for response in fixture["responses"][:2]]
        runs[0]["workflow_runs"] = runs[0]["workflow_runs"][:1]
        with patch.object(selector, "api_pages", side_effect=[runs, skipped_jobs]):
            self.assertEqual(selector.baseline(fixture["branch"]), "")

    def test_skipped_or_failed_job_cannot_supply_baseline(self):
        fixture = json.loads((FIXTURES / "manual-success.json").read_text())
        runs, jobs = [response["pages"] for response in fixture["responses"]]
        runs[0]["workflow_runs"] = runs[0]["workflow_runs"][:1]
        for conclusion in ("skipped", "failure", None):
            with self.subTest(conclusion=conclusion):
                jobs[0]["jobs"][0]["conclusion"] = conclusion
                with patch.object(selector, "api_pages", side_effect=[runs, jobs]):
                    self.assertEqual(selector.baseline(fixture["branch"]), "")

    def test_unrelated_job_cannot_supply_baseline(self):
        fixture = json.loads((FIXTURES / "manual-success.json").read_text())
        runs, jobs = [response["pages"] for response in fixture["responses"]]
        runs[0]["workflow_runs"] = runs[0]["workflow_runs"][:1]
        jobs[0]["jobs"][0]["name"] = "build"
        with patch.object(selector, "api_pages", side_effect=[runs, jobs]):
            self.assertEqual(selector.baseline(fixture["branch"]), "")

    def test_malformed_json_fails_open(self):
        with patch.object(selector.subprocess, "check_output", return_value="{"):
            self.assertEqual(selector.baseline("main"), "")

    def test_missing_gh_fails_open(self):
        with patch.object(selector.subprocess, "check_output", side_effect=FileNotFoundError):
            self.assertEqual(selector.baseline("main"), "")

    def test_runs_and_jobs_pagination(self):
        fixture = json.loads((FIXTURES / "ci-success.json").read_text())
        runs, jobs = [response["pages"] for response in fixture["responses"][:2]]
        with patch.object(selector, "api_pages", side_effect=[
            [{"workflow_runs": []}, *runs], [{"jobs": []}, *jobs]
        ]):
            self.assertEqual(selector.baseline(fixture["branch"]), fixture["expected"])

    def test_workflow_skips_after_ci_deploy_and_always_deploys_manually(self):
        # Execute the real changed-path gate and selector, stubbing only GitHub
        # and git. A stale manual-only baseline reports a website change here.
        workflow = (ROOT.parent / ".github/workflows/deploy-site.yml").read_text()
        body = textwrap.dedent(re.search(
            r"        run: \|\n((?:          [^\n]*\n|\n)+)", workflow
        ).group(1))
        fixture = json.loads((FIXTURES / "ci-success.json").read_text())
        temp_root = ROOT.parent / "target/deploy-baseline-tests"
        temp_root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=temp_root) as directory:
            temp = Path(directory)
            for name, response in zip(("runs", "jobs"), fixture["responses"]):
                (temp / f"{name}.json").write_text(json.dumps(response["pages"]))
            (temp / "gh").write_text('''#!/bin/sh
case "$*" in
  *"/301/jobs"*) cat "$FIXTURE_JOBS" ;;
  *"actions/runs"*) cat "$FIXTURE_RUNS" ;;
  "run list "*) echo "$STALE_SHA" ;;
  *) exit 1 ;;
esac
''')
            (temp / "git").write_text('''#!/bin/sh
case "$1" in
  merge-base) exit 0 ;;
  diff)
    if [ "$3" = "$DEPLOYED_SHA" ]; then echo crates/example.rs
    else echo docs/changed.md; fi ;;
  *) exit 1 ;;
esac
''')
            for name in ("gh", "git"):
                (temp / name).chmod(0o755)
            env = dict(os.environ, PATH=f"{temp}{os.pathsep}{os.environ['PATH']}",
                       DEFAULT_BRANCH=fixture["branch"], DEPLOYED_SHA=fixture["expected"],
                       STALE_SHA="b" * 40, FIXTURE_RUNS=str(temp / "runs.json"),
                       FIXTURE_JOBS=str(temp / "jobs.json"), GITHUB_OUTPUT=str(temp / "output"))
            for event, expected in (("push", "false"), ("workflow_dispatch", "true")):
                with self.subTest(event=event):
                    (temp / "output").write_text("")
                    subprocess.run(["bash", "-e", "-c", body], check=True,
                                   cwd=ROOT.parent, env=dict(env, GITHUB_EVENT_NAME=event))
                    self.assertEqual((temp / "output").read_text(), f"changed={expected}\n")


if __name__ == "__main__":
    unittest.main(verbosity=2)
