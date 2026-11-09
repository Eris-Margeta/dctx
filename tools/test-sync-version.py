#!/usr/bin/env python3
"""Regression checks for release synchronization; all scratch files stay in target/."""

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent.parent
FIXTURES = "docs/design/plugin-api/fixtures"


class SyncVersionTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "target").mkdir(exist_ok=True)
        self.scratch = tempfile.TemporaryDirectory(prefix="sync-version-", dir=ROOT / "target")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        paths = [
            "VERSION", "Cargo.lock", "project-meta.yaml", "README.md",
            "tools/sync-version.sh", "tools/website/data/facts.json",
            "tools/plugin-index/Cargo.lock", "tools/plugin-index/src/domain/index.rs",
            "tools/plugin-index/tests/display_properties.rs",
            "crates/dctx-template/src/plugin/catalog.rs",
            "docs/MODALITIES.md", "docs/START.md", "docs/VERIFYING.md", "docs/README.md",
            "docs/protocol/README.md", "docs/design/plugin-api/dts.md",
            "packages/go/README.md", "packages/go/api_generated.go",
            "packages/python/README.md", "packages/python/pyproject.toml",
            "packages/python/src/dctx/_version.py", "packaging/npm/README.md",
        ]
        for pattern in (
            "crates/*/Cargo.toml", "plugins/*/Cargo.toml", "plugins/*/plugin.json",
            "plugins/index/**/*.json", f"{FIXTURES}/**/*.*", "packaging/npm/*/package.json",
        ):
            paths.extend(p.relative_to(ROOT).as_posix() for p in ROOT.glob(pattern))
        for relative in paths:
            destination = self.root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        # Go generation is outside these checks; isolate it from the real Go caches.
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        go = bin_dir / "go"
        go.write_text("#!/bin/sh\nexit 0\n")
        go.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"}

    def run_sync(self, mode, expected=0):
        result = subprocess.run(
            ["sh", "tools/sync-version.sh", mode], cwd=self.root, env=self.env,
            capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result.stdout + result.stderr

    def snapshot(self):
        return {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}

    def test_next_release_updates_the_complete_digest_chain_and_preserves_negatives(self):
        negatives = [
            f"{FIXTURES}/invalid/manifest-prerelease-zero.json",
            f"{FIXTURES}/invalid/manifest-range-reversed.json",
        ]
        before = {p: (self.root / p).read_bytes() for p in negatives}
        (self.root / "VERSION").write_text("9.8.7\n")
        snapshot = self.snapshot()
        output = self.run_sync("--check", expected=1)
        for relative in (
            "plugins/dctx-plugin-git/plugin.json", "crates/dctx-template/src/plugin/catalog.rs",
            f"{FIXTURES}/digests.json", "plugins/index/index.json", "tools/plugin-index/Cargo.lock",
        ):
            self.assertIn(relative, output)
        self.assertEqual(self.snapshot(), snapshot, "--check must be read-only")
        self.run_sync("--write")
        self.run_sync("--check")
        snapshot = self.snapshot()
        self.run_sync("--write")
        self.assertEqual(self.snapshot(), snapshot, "--write must be idempotent")
        for relative, original in before.items():
            self.assertEqual((self.root / relative).read_bytes(), original)
        floating = json.loads((self.root / FIXTURES / "invalid/dts-floating-version.json").read_text())
        self.assertEqual(floating["plugins"][0]["resolution"]["version"], "^1.0.0")
        duplicate = json.loads((self.root / "plugins/index/invalid/duplicate-id.json").read_text())
        release = duplicate["plugins"][1]["releases"][0]
        self.assertEqual(release["version"], "1.0.1")
        self.assertIn("/1.0.1/", release["manifest_url"])
        manifest = (self.root / FIXTURES / "valid/manifest-git.json").read_bytes()
        manifest_digest = hashlib.sha256(manifest).hexdigest()
        catalog = (self.root / "crates/dctx-template/src/plugin/catalog.rs").read_text()
        self.assertIn(manifest_digest, catalog)
        plan = json.loads((self.root / FIXTURES / "valid/dts-plan.json").read_text())
        self.assertEqual(plan["plugins"][0]["resolution"]["version"], "9.8.7")
        self.assertEqual(plan["plugins"][0]["resolution"]["manifest_digest"], f"sha256:{manifest_digest}")
        canonical = json.dumps(plan, sort_keys=True, separators=(",", ":")).encode()
        plan_digest = hashlib.sha256(canonical).hexdigest()
        for name in ("initialize", "receipt"):
            self.assertIn(plan_digest, (self.root / FIXTURES / f"valid/{name}.json").read_text())

    def test_check_detects_stale_version_and_digest_without_a_version_bump(self):
        self.run_sync("--write")
        relative = f"{FIXTURES}/valid/dts-plan.json"
        path = self.root / relative
        text = re.sub(r'("manifest_digest": "sha256:)[0-9a-f]{64}',
                      lambda match: match[1] + "0" * 64, path.read_text())
        text = re.sub(r'("version": ")\d+\.\d+\.\d+(")', r'\g<1>0.0.0\2', text)
        path.write_text(text)
        output = self.run_sync("--check", expected=1)
        self.assertIn(relative, output)
        self.run_sync("--write")
        self.run_sync("--check")

    def test_missing_catalog_digest_names_file_and_prevents_partial_writes(self):
        relative = "crates/dctx-template/src/plugin/catalog.rs"
        path = self.root / relative
        path.write_text(path.read_text().replace("manifest_digest:", "missing_digest:"))
        (self.root / "VERSION").write_text("9.8.7\n")
        snapshot = self.snapshot()
        output = self.run_sync("--write", expected=1)
        self.assertIn(relative, output)
        self.assertEqual(self.snapshot(), snapshot)


if __name__ == "__main__":
    unittest.main()
