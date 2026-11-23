# SPDX-License-Identifier: Apache-2.0
"""Guard the hosted website workflow with standard-library-only checks."""

from pathlib import Path
from textwrap import dedent
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/deploy-site.yml"
STEP_START = "\n      - "
BUILD_STEP = "name: Build and check website"

# Approve the workflow shape and commands explicitly. A list of transfer
# utilities cannot rule out uploads through arbitrary shell or Python code.
APPROVED_WORKFLOW = dedent("""\
    name: Build and validate website

    on:
      workflow_call:
      workflow_dispatch:

    permissions:
      contents: read

    jobs:
      build:
        permissions:
          contents: read
          actions: read
        runs-on: ubuntu-24.04
        steps:
          - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5
            with:
              fetch-depth: 0
              persist-credentials: false
          - name: Has the website changed since the last successful run?
            id: changed
            env:
              GH_TOKEN: ${{ github.token }}
            run: |
              python3 tools/ci/deploy_changed.py >> "$GITHUB_OUTPUT"
          - uses: actions/setup-python@a26af69be951a213d495a4c3e4e4022e16d87065
            if: steps.changed.outputs.changed == 'true'
            with:
              python-version: '3.13'
          - name: Install pinned website dependencies in a venv
            if: steps.changed.outputs.changed == 'true'
            run: |
              python -m venv tools/website/.venv
              tools/website/.venv/bin/python -m pip install -r tools/website/requirements.txt
          - name: Build and check website
          - name: Upload the built website artifact
            if: steps.changed.outputs.changed == 'true'
            uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02
            with:
              name: website
              path: website/
              include-hidden-files: true
              if-no-files-found: error
              retention-days: 1
""").rstrip()

APPROVED_BUILD_STEP = (
    f"{BUILD_STEP}\n"
    "        if: steps.changed.outputs.changed == 'true'\n"
    "        run: |\n"
    "          tools/website/.venv/bin/python tools/website/build.py\n"
    "          tools/website/.venv/bin/python tools/website/check.py --working-tree --no-capture"
)


class SiteWorkflowScope(unittest.TestCase):
    maxDiff = None

    @classmethod
    def setUpClass(cls):
        workflow = "\n".join(
            line.partition(" # ")[0]
            if line.lstrip().startswith(("- uses: ", "uses: "))
            else line
            for line in WORKFLOW.read_text(encoding="utf-8").splitlines()
            if not line.lstrip().startswith("#")
        )
        cls.sections = workflow.split(STEP_START)

    def test_workflow_has_no_remote_transfer(self):
        self.assertMultiLineEqual(
            STEP_START.join(
                BUILD_STEP if section.startswith(f"{BUILD_STEP}\n") else section
                for section in self.sections
            ),
            APPROVED_WORKFLOW,
        )

    def test_workflow_runs_website_build_and_validation(self):
        self.assertEqual(
            [section for section in self.sections if section.startswith(f"{BUILD_STEP}\n")],
            [APPROVED_BUILD_STEP],
        )


if __name__ == "__main__":
    unittest.main()
