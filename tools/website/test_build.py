"""The committed website must pass the isolated rebuild and integrity check."""
import subprocess
import sys
import unittest
from pathlib import Path


class WebsiteBuildTest(unittest.TestCase):
    def test_committed_website(self):
        result = subprocess.run(
            [sys.executable, str(Path(__file__).with_name("check.py"))],
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
