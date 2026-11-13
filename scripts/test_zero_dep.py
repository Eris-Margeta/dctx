# SPDX-License-Identifier: Apache-2.0
"""Offline Cargo.lock/Cargo.toml fixture trees; all temporary files stay local."""

from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest

from scripts import check_zero_dep as checker


class ZeroDependencyTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(dir=checker.ROOT)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.write("Cargo.toml", '[workspace]\nmembers = ["crates/*"]\n')
        self.write("scripts/zero_dep_allow.txt", "# Empty baseline\n")
        self.packages = []
        self.member("dctx")
        self.member("dctx-cli")

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def package(self, name, dependencies=(), version="1.0.0", external=True):
        package = {"name": name, "version": version}
        if external:
            package["source"] = "registry+fixture"
        if dependencies:
            package["dependencies"] = list(dependencies)
        self.packages.append(package)

    def member(self, name, manifest="", dependencies=()):
        self.write(f"crates/{name}/Cargo.toml",
                   f'[package]\nname = "{name}"\nversion = "1.0.0"\n{manifest}')
        self.packages[:] = [p for p in self.packages if p["name"] != name]
        self.package(name, dependencies, external=False)

    def allow(self, *entries):
        self.write("scripts/zero_dep_allow.txt",
                   "".join(f"{entry} # Fixture dependency needed for normal functionality\n"
                           for entry in entries))

    def run_check(self):
        text = "version = 4\n"
        for package in self.packages:
            text += "\n[[package]]\n"
            text += "".join(f"{key} = {json.dumps(value)}\n"
                            for key, value in package.items())
        self.write("Cargo.lock", text)
        output = io.StringIO()
        with redirect_stdout(output), redirect_stderr(output):
            status = checker.main(["--root", str(self.root)])
        return status, output.getvalue()

    def test_no_dependencies(self):
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("0 crates", output)

    def test_one_allowed_dependency(self):
        self.member("dctx", '[dependencies]\nnormal = "1"\n', ["normal"])
        self.package("normal")
        self.allow("normal 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("normal 1.0.0 # dctx -> normal", output)
        self.assertIn("Zero-dependency promise: NOT MET", output)

    def test_new_dependency_fails(self):
        self.member("dctx-cli", '[dependencies]\nnew = "1"\n', ["new"])
        self.package("new")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("EXTRA: new 1.0.0", output)

    def test_dev_dependency_and_its_closure_ignored(self):
        self.member("dctx", '[dev-dependencies]\ndev = "1"\n', ["dev"])
        self.package("dev", ["dev-child"])
        self.package("dev-child")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertNotIn("dev 1.0.0", output)
        self.assertNotIn("dev-child 1.0.0", output)

    def test_build_dependency_reported_separately(self):
        self.member("dctx", '[build-dependencies]\nbuilder = "1"\n', ["builder"])
        self.package("builder", ["build-child"])
        self.package("build-child")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("normal-rooted lock closure: 0 crates", output)
        self.assertIn("build: builder 1.0.0", output)
        self.assertIn("build: build-child 1.0.0", output)

    def test_stale_allowlist_fails(self):
        self.allow("removed 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("STALE: removed 1.0.0", output)

    def test_workspace_and_external_transitive_closure_with_cycle(self):
        self.member("dctx-cli", '[dependencies]\nengine = { path = "../engine" }\n', ["engine"])
        self.member("engine", '[dependencies]\nfirst = "1"\n', ["first"])
        self.package("first", ["second"])
        self.package("second", ["first"])
        self.allow("first 1.0.0", "second 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("dctx-cli -> engine -> first -> second", output)
        self.assertIn("lock closure: 2 crates", output)

    def test_target_normal_dev_and_build_tables(self):
        self.member("dctx", '''[target.'cfg(unix)'.dependencies]
normal = "1"
[target.'cfg(windows)'.dev-dependencies]
dev = "1"
[target.'cfg(unix)'.build-dependencies]
builder = "1"
''', ["normal", "dev", "builder"])
        for name in ("normal", "dev", "builder"):
            self.package(name)
        self.allow("normal 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertNotIn("dev 1.0.0", output)
        self.assertIn("build: builder 1.0.0", output)

    def test_renamed_workspace_inherited_dependency(self):
        self.write("Cargo.toml", '''[workspace]
members = ["crates/*"]
[workspace.dependencies]
alias = { package = "real-name", version = "1" }
''')
        self.member("dctx", '[dependencies]\nalias = { workspace = true }\n', ["real-name"])
        self.package("real-name")
        self.allow("real-name 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)

    def test_same_name_normal_and_dev_use_different_versions(self):
        self.member("dctx", '''[dependencies]
normal = { package = "shared", version = "=1.0.0" }
[dev-dependencies]
dev = { package = "shared", version = "=2.0.0" }
''', ["shared 1.0.0", "shared 2.0.0"])
        self.package("shared")
        self.package("shared", version="2.0.0")
        self.allow("shared 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertNotIn("shared 2.0.0", output)

    def test_dev_dependency_also_normal_is_counted(self):
        self.member("dctx", '''[dependencies]
shared = "1"
[dev-dependencies]
shared = "1"
''', ["shared"])
        self.package("shared")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("EXTRA: shared 1.0.0", output)

    def test_unreachable_member_dependencies_ignored(self):
        self.member("desktop", '''[dependencies]
desktop-only = "1"
[build-dependencies]
desktop-build = "1"
''', ["desktop-only", "desktop-build"])
        self.package("desktop-only")
        self.package("desktop-build")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertNotIn("desktop-only 1.0.0", output)
        self.assertNotIn("desktop-build 1.0.0", output)

    def test_optional_dependency_retained_in_conservative_envelope(self):
        self.member("dctx", '[dependencies]\noptional = { version = "1", optional = true }\n', ["optional"])
        self.package("optional")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("EXTRA: optional 1.0.0", output)

    def test_ambiguous_locked_edge_fails_closed(self):
        self.member("dctx", '[dependencies]\nshared = "1"\n', ["shared"])
        self.package("shared")
        self.package("shared", version="2.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("ambiguous locked dependency", output)

    def test_source_qualified_locked_edge(self):
        self.member("dctx", '[dependencies]\nnormal = "1"\n', ["normal 1.0.0 (registry+fixture)"])
        self.package("normal")
        self.allow("normal 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)

    def test_missing_release_root_fails_closed(self):
        self.packages[:] = [p for p in self.packages if p["name"] != "dctx-cli"]
        (self.root / "crates/dctx-cli/Cargo.toml").unlink()
        (self.root / "crates/dctx-cli").rmdir()
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("release root: dctx-cli", output)

    def test_malformed_allowlist_reason_required(self):
        self.write("scripts/zero_dep_allow.txt", "normal 1.0.0\n")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("expected NAME VERSION # reason", output)

    def test_usage_error_exits_two(self):
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as caught:
            checker.main(["--unknown"])
        self.assertEqual(caught.exception.code, 2)

    def test_upgrade_reports_extra_and_stale_versions(self):
        self.member("dctx", '[dependencies]\nnormal = "2"\n', ["normal"])
        self.package("normal", version="2.0.0")
        self.allow("normal 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("EXTRA: normal 2.0.0", output)
        self.assertIn("STALE: normal 1.0.0", output)

    def test_new_transitive_dependency_fails(self):
        self.member("dctx", '[dependencies]\nnormal = "1"\n', ["normal"])
        self.package("normal", ["new-child"])
        self.package("new-child")
        self.allow("normal 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 1, output)
        self.assertIn("EXTRA: new-child 1.0.0", output)

    def test_registry_package_sharing_workspace_name_is_external(self):
        self.member("dctx", '[dependencies]\nengine = "1"\n',
                    ["engine 1.0.0 (registry+fixture)"])
        self.member("engine")
        self.package("engine")
        self.allow("engine 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("lock closure: 1 crates", output)

    def test_workspace_inherited_version_excludes_noncrate_directory(self):
        self.write("Cargo.toml", '''[workspace]
members = ["crates/*", "assets/*"]
exclude = ["assets/not-a-crate"]
[workspace.package]
version = "1.0.0"
''')
        self.write("assets/not-a-crate/data.txt", "not a Cargo member")
        self.write("crates/dctx/Cargo.toml",
                   '[package]\nname = "dctx"\nversion.workspace = true\n')
        status, output = self.run_check()
        self.assertEqual(status, 0, output)

    def test_build_workspace_member_normal_closure_only_reported_as_build(self):
        self.member("dctx", '[build-dependencies]\nengine = { path = "../engine" }\n', ["engine"])
        self.member("engine", '[dependencies]\nhelper = "1"\n', ["helper"])
        self.package("helper")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertIn("normal-rooted lock closure: 0 crates", output)
        self.assertIn("build: helper 1.0.0", output)

    def test_nonexact_normal_version_excludes_dev_version(self):
        self.member("dctx", '''[dependencies]
normal = { package = "shared", version = "^1.0" }
[dev-dependencies]
dev = { package = "shared", version = "2" }
''', ["shared 1.0.0", "shared 2.0.0"])
        self.package("shared")
        self.package("shared", version="2.0.0")
        self.allow("shared 1.0.0")
        status, output = self.run_check()
        self.assertEqual(status, 0, output)
        self.assertNotIn("shared 2.0.0", output)


if __name__ == "__main__":
    unittest.main()
