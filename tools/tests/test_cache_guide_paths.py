#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Check cache guide prose and rendered tables offline with Python-Markdown.

The paths follow dctx-cli's host/cache.rs and the v1.2.0 version of that file.
The upgrade behavior is exercised by the CLI test
old_and_other_engine_caches_survive_new_indexing_and_purge in tests/doctor.rs.
"""

import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import unittest
from xml.etree import ElementTree

import markdown


ROOT = Path(__file__).resolve().parents[2]
MAP_GUIDE = "docs/guides/MAP-MODE.md"
SCOPES = "docs/SCOPES.md"
PARENTS = (
    ("Linux / other Unix (XDG_CACHE_HOME set and nonempty)", PurePosixPath("$XDG_CACHE_HOME/dctx/v2")),
    ("Linux / other Unix (XDG_CACHE_HOME unset or empty)", PurePosixPath("$HOME/.cache/dctx/v2")),
    ("macOS", PurePosixPath("$HOME/Library/Caches/dctx/v2")),
    ("Windows", PureWindowsPath(r"%LOCALAPPDATA%\dctx\Cache\v2")),
)
OVERRIDE = PurePosixPath("/path/to/dctx-cache")


def equal(actual, expected, label):
    if actual != expected:
        raise AssertionError(
            f"{json.dumps(label)} expected {json.dumps(expected)}, got {json.dumps(actual)}"
        )


def read_guide(guide):
    try:
        return (ROOT / guide).read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise AssertionError(f"Cannot read {json.dumps(guide)} ({json.dumps(str(error))})") from None


class CacheGuidePathsTest(unittest.TestCase):
    def check_rendered_table(self, guide):
        rendered = markdown.markdown(read_guide(guide), extensions=["fenced_code", "tables"])
        tables = [
            table for table in re.findall(r"<table>.*?</table>", rendered, re.DOTALL)
            if "<th>Cache parent</th>" in table
        ]
        equal(len(tables), 1, f"{guide} rendered cache table count")
        try:
            table = ElementTree.fromstring(tables[0])
        except ElementTree.ParseError as error:
            self.fail(f"Invalid cache table in {json.dumps(guide)} ({json.dumps(str(error))})")

        headers = [cell.text for cell in table.findall("./thead/tr/th")]
        equal(headers, ["Platform", "Cache parent", "Active cache directory (1.3)",
                        "Active ledger (1.3)"], f"{guide} rendered headings")
        rows = table.findall("./tbody/tr")
        equal(len(rows), len(PARENTS), f"{guide} rendered platform count")
        for row, (platform, parent) in zip(rows, PARENTS):
            cells = row.findall("td")
            equal(len(cells), 4, f"{guide} {platform} column count")
            equal("".join(cells[0].itertext()), platform, f"{guide} platform label")
            root = parent / "1.3"
            paths = [cell.findtext("code") for cell in cells[1:]]
            equal(paths, [str(parent), str(root), str(root / "ledger.v1")],
                  f"{guide} {platform} rendered paths")

    def test_map_guide_rendered_table(self):
        self.check_rendered_table(MAP_GUIDE)

    def test_scopes_rendered_table(self):
        self.check_rendered_table(SCOPES)

    def test_prose_describes_override_and_patch_sharing(self):
        root = OVERRIDE / "1.3"
        for guide in (MAP_GUIDE, SCOPES):
            prose = " ".join(read_guide(guide).split())
            for phrase in (
                "Set `DCTX_CACHE_DIR` to an absolute cache parent path",
                "With `DCTX_CACHE_DIR` unset or empty",
                f"`DCTX_CACHE_DIR={OVERRIDE}`",
                f"`{root}`",
                f"`{root / 'ledger.v1'}`",
                "1.3.1 and 1.3.2 share the `1.3` directory and ledger when they use the same parent",
                "`dctx doctor --json`",
                "`cache_root`",
                "`running: true`",
            ):
                self.assertTrue(phrase in prose, f"{json.dumps(guide)} missing {json.dumps(phrase)}")

    def test_prose_describes_upgrade_and_backup(self):
        for guide in (MAP_GUIDE, SCOPES):
            prose = " ".join(read_guide(guide).split())
            for phrase in (
                "1.2.0",
                "`<cache-parent>/ledger.v1`",
                "leaves the unscoped ledger in place",
                "builds an index in its own child directory on first use",
                "copy the whole `1.3` directory",
            ):
                self.assertTrue(phrase in prose, f"{json.dumps(guide)} missing {json.dumps(phrase)}")
        prose = " ".join(read_guide(SCOPES).split())
        legacy = f"`{OVERRIDE / 'ledger.v1'}`"
        self.assertTrue(legacy in prose, f"{json.dumps(SCOPES)} missing {json.dumps(legacy)}")
        self.assertTrue("back up the parent containing the unscoped ledger and its data" in prose,
                        "Scopes must identify the legacy backup directory")

    def test_mismatch_escapes_hostile_values(self):
        hostile = 'bad\n\x1b[31m"'
        with self.assertRaises(AssertionError) as failure:
            equal(hostile, "expected", hostile)
        self.assertEqual(str(failure.exception),
                         r'"bad\n\u001b[31m\"" expected "expected", got "bad\n\u001b[31m\""')


if __name__ == "__main__":
    unittest.main()
