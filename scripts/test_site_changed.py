"""Exercise website change detection against real Git history."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("site_changed.py").resolve()


class SiteChangedTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=SCRIPT.parent)
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git("init", "--quiet")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.invalid")
        self.paths_file = self.repo / "paths.txt"
        self.paths_file.write_text("website/\ntools/website/\ndocs/\n")
        self.commit("README.md", "initial")
        self.baseline = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.check_output(
            ["git", *args], cwd=self.repo, text=True, stderr=subprocess.PIPE,
        )

    def commit(self, path, content):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
        self.git("add", path)
        self.git("commit", "--quiet", "-m", "test change")

    def decide(self, baseline):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--last-sha", baseline,
             "--head", self.git("rev-parse", "HEAD").strip(),
             "--paths-file", str(self.paths_file)],
            cwd=self.repo, text=True, capture_output=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_website_commit_before_non_website_commit(self):
        self.commit("website/index.html", "updated")
        self.commit("README.md", "unrelated")
        self.assertEqual(self.decide(self.baseline), "CHANGED")

    def test_no_website_diff(self):
        self.commit("README.md", "unrelated")
        self.assertEqual(self.decide(self.baseline), "UNCHANGED")

    def test_unknown_last_sha(self):
        self.assertEqual(self.decide("0" * 40), "CHANGED")

    def test_empty_last_sha(self):
        self.assertEqual(self.decide(""), "CHANGED")

    def test_deleted_website_file(self):
        self.commit("docs/guide.md", "guide")
        baseline = self.git("rev-parse", "HEAD").strip()
        self.git("rm", "docs/guide.md")
        self.git("commit", "--quiet", "-m", "remove guide")
        self.assertEqual(self.decide(baseline), "CHANGED")


if __name__ == "__main__":
    unittest.main()
