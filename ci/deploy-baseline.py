# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Print the newest default-branch SHA actually deployed, or nothing (fail open)."""

import json
import subprocess
import sys


def api_pages(endpoint, **fields):
    command = ["gh", "api", "--method", "GET", "--paginate", "--slurp", endpoint]
    for key, value in fields.items():
        command.extend(["-f", f"{key}={value}"])
    return json.loads(subprocess.check_output(command, text=True))


def baseline(branch):
    try:
        # Repository runs include reusable deployments recorded under CI. The API
        # lists newest runs first; pagination keeps older real deploys reachable.
        pages = api_pages("repos/{owner}/{repo}/actions/runs", branch=branch, per_page=100)
        for page in pages:
            for run in page["workflow_runs"]:
                if run["head_branch"] != branch or run["path"].split("@", 1)[0] not in {
                    ".github/workflows/ci.yml",
                    ".github/workflows/deploy-site.yml",
                }:
                    continue
                job_pages = api_pages(
                    f"repos/{{owner}}/{{repo}}/actions/runs/{run['id']}/jobs",
                    filter="latest",
                    per_page=100,
                )
                for job_page in job_pages:
                    for job in job_page["jobs"]:
                        if job["name"] not in {"deploy", "deploy-site / deploy"}:
                            continue
                        # A green job with missing credentials has a skipped rsync
                        # step. Only success of the actual upload is a baseline.
                        if job["conclusion"] == "success" and any(
                            step["name"].startswith("Deploy to ")
                            and step["conclusion"] == "success"
                            for step in job["steps"]
                        ):
                            return run["head_sha"]
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError, TypeError):
        # No older baseline is safe after any lookup failure.
        return ""
    return ""


if __name__ == "__main__":
    sha = baseline(sys.argv[1])
    if sha:
        print(sha)
