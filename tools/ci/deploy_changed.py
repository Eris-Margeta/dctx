# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Skip deployment only when a successful main run proves no website changes."""

import json
import os
import re
import subprocess


def changed(event):
    if event == "workflow_dispatch":
        return True
    try:
        runs = json.loads(subprocess.check_output([
            "gh", "run", "list", "--workflow", "deploy-site.yml",
            "--status", "success", "--branch", "main", "-L", "1", "--json", "headSha",
        ], text=True))
        baseline = runs[0]["headSha"]
        if not isinstance(baseline, str) or not re.fullmatch(r"[0-9a-fA-F]{40}", baseline):
            return True
        if subprocess.run([
            "git", "merge-base", "--is-ancestor", baseline, "HEAD",
        ], stderr=subprocess.DEVNULL).returncode != 0:
            return True
        paths = subprocess.check_output([
            "git", "diff", "--name-only", "-z", baseline, "HEAD", "--",
        ], text=True).split("\0")
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError, IndexError, TypeError):
        # Missing history or an unavailable API must never hide a website update.
        return True
    return any(
        path.startswith(("website/", "tools/website/", "docs/"))
        or path in {
            "CHANGELOG.md", ".github/workflows/deploy-site.yml",
            "tools/ci/deploy_changed.py", "tools/ci/test_deploy_changed.py",
        }
        for path in paths
    )


if __name__ == "__main__":
    print(f"changed={str(changed(os.environ.get('GITHUB_EVENT_NAME', ''))).lower()}")
