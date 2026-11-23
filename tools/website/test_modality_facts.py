"""Offline regression check for release and doctor statuses in the site matrix."""

import unittest
from unittest.mock import patch
from xml.etree import ElementTree

import facts
from pages.paths import responsibilities


class ModalityFacts(unittest.TestCase):
    def test_system_binary_statuses_in_responsibility_cells(self):
        # Read the live facts without loading unrelated benchmark and CLI captures.
        with patch.object(facts, "_cache", None), patch.object(facts, "measured", return_value=[]):
            table = ElementTree.fromstring(responsibilities())

        rows = [["".join(cell.itertext()) for cell in row.findall("td")]
                for row in table.findall("./table/tbody/tr")]
        system_rows = [row for row in rows if row and row[0] == "System binary"]
        self.assertEqual(len(system_rows), 1)
        _, provides, responsible_for = system_rows[0]
        self.assertEqual(
            provides,
            "Release archives with cosign signatures, signed SHA256SUMS, SBOMs and build provenance, "
            "cache caps, dctx doctor for local binary inventory; journals (planned), receipts (planned)",
        )
        self.assertIn("a policy file is planned", responsible_for)


if __name__ == "__main__":
    unittest.main()
