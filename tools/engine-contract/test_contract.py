# SPDX-License-Identifier: Apache-2.0
"""Regression tests for the CI guards, including deliberate boundary violations."""
import tempfile
import unittest
from contextlib import redirect_stdout
from io import StringIO
from pathlib import Path
from unittest.mock import patch

import check_imports
import parity
import warm_read


class ImportGuard(unittest.TestCase):
    def errors(self, source, name="wrapper.rs", api=()):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / name
            path.write_text(source)
            return check_imports.check_file(path, Path(name), api)

    def test_grouped_alias_and_qualified_imports_are_denied(self):
        for source in [
            "use dctx_pack::engine::Engine;",
            "use ::dctx_core::{selection::{Policy as P}, facts::*};",
            "pub use dctx::{Session, context::ContextRequest};",
            "fn f() { dctx_map_model::render::answer(); }",
            "extern crate dctx_pack as engine;",
            "use crate::{host::open, map::output};",
            "use r#dctx_pack::r#engine::Engine;",
            '''const Q: char = '"'; dctx_pack::engine::render(); const A: char = '"';''',
        ]:
            with self.subTest(source=source):
                self.assertTrue(self.errors(source))

    def test_exact_allowlist_and_group_expansion(self):
        api = ["dctx_cli::exit::Failure", "dctx_cli::exit::CommandOutput", "dctx_cli::rpc"]
        self.assertFalse(self.errors("use dctx_cli::{exit::{Failure as F, CommandOutput}, rpc::{self, Verb}};", api=api))
        for source in ["use dctx_cli::exit::*;", "use dctx_cli::exit::FailureExtra;", "use dctx_cli::host::open;", "use dctx_cli::exit;"]:
            self.assertTrue(self.errors(source, api=api))

    def test_comments_and_literals_do_not_count(self):
        self.assertFalse(self.errors('''
            // use dctx_pack::engine::Engine;
            /* outer /* nested */ dctx_core::facts::X */
            const TEXT: &str = r###"use dctx_pack::engine::Engine;"###;
            const OTHER: &str = "dctx_core::facts::X";
            const QUOTE: char = '"';
            const SLASH: char = '/';
        '''))

    def test_included_code_cannot_escape_scan(self):
        self.assertTrue(self.errors('include!("outside.rs");'))
        self.assertTrue(self.errors('#[path = "outside.rs"] mod hidden;'))
        self.assertTrue(self.errors("use super::super::host::open;"))
        self.assertTrue(self.errors("use super::host::open;", "mod.rs"))

    def test_cargo_aliases_and_target_dependencies(self):
        self.assertTrue(self.errors('[target.\'cfg(unix)\'.dependencies]\nsecret = { package = "dctx-pack", version = "1" }', "Cargo.toml"))

    def test_shared_crate_dependency_alias_is_resolved(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text('[dependencies]\nsecret = { package = "dctx-pack", version = "1" }')
            wrapper = root / "wrapper.rs"
            wrapper.write_text("use secret::engine::Engine;")
            self.assertTrue(check_imports.check_file(wrapper, Path("wrapper.rs"), []))

    def test_foreign_language_engine_imports(self):
        for source, name in [('const p = require("dctx-map-model");', "binding.js"),
                             ('from dctx_pack import engine', "binding.py")]:
            self.assertTrue(self.errors(source, name))

    def test_missing_required_surface_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            count, errors = check_imports.check(Path(tmp), {"version": 1, "surface": [
                {"name": "rpc", "paths": ["rpc"], "required": True, "rust_api": []}
            ]})
            self.assertEqual(count, 0)
            self.assertTrue(errors)

    def test_new_binding_sources_are_discovered(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binding = root / "bindings/new-package/src/wrapper.rs"
            binding.parent.mkdir(parents=True)
            binding.write_text("use dctx_pack::engine::Engine;")
            count, errors = check_imports.check(root, {"version": 1, "surface": [
                {"name": "bindings", "paths": ["bindings"], "required": False, "rust_api": []}
            ]})
            self.assertEqual(count, 1)
            self.assertTrue(errors)


class GoldenGuard(unittest.TestCase):
    def test_changed_bytes_and_non_bytes_fail(self):
        case = {"name": "guard", "golden": "cases.json"}
        expected = (parity.CORPUS / "cases.json").read_bytes()
        for actual in [expected.rstrip(), expected + b"\n", expected.decode()]:
            with self.subTest(actual=type(actual)):
                with self.assertRaisesRegex(AssertionError, "byte parity failed"):
                    parity.verify(None, [case], {"bad": lambda context, item: actual})
        self.assertEqual(parity.verify(None, [case], {"good": lambda context, item: expected}), 1)


class TraceGuard(unittest.TestCase):
    repo = Path("/fixture/repo")
    caches = [Path("/fixture/cache")]
    target_open = 'openat(AT_FDCWD</fixture/repo>, "src/selected.rs", O_RDONLY) = 3</fixture/repo/src/selected.rs>\n'

    def test_target_only_passes(self):
        opens, sources = warm_read.check_trace(self.target_open, self.repo, self.caches)
        self.assertFalse(opens)
        self.assertEqual(sources["src/selected.rs"], 1)

    def test_cache_source_reads_and_tree_walks_fail(self):
        for violation in [
            'openat(AT_FDCWD, "/fixture/cache/ledger.v1", O_RDONLY) = 4</fixture/cache/ledger.v1>',
            'openat(4</fixture/cache>, "unexpected", O_RDONLY) = -1 ENOENT',
            'openat(AT_FDCWD, "/fixture/repo/other.rs", O_RDONLY) = 4</fixture/repo/other.rs>',
            'read(4</fixture/repo/other.rs>, "secret", 6) = 6',
            'mmap(NULL, 100, PROT_READ, MAP_PRIVATE, 4</fixture/repo/other.rs>, 0) = 0x12',
            'getdents64(4</fixture/repo/unrelated>, [], 4096) = 0',
            'read(4</fixture/cache/objects/1>, "secret", 6) = 6',
            self.target_open * 3,
        ]:
            with self.subTest(violation=violation):
                with self.assertRaises(AssertionError):
                    warm_read.check_trace(self.target_open + violation, self.repo, self.caches)

    def test_empty_or_unparsed_trace_fails(self):
        for trace in ["", 'openat(7, "ledger.v1", O_RDONLY) = -1 ENOENT']:
            with self.assertRaises(AssertionError):
                warm_read.check_trace(trace, self.repo, self.caches)

    def test_unsupported_host_and_missing_strace_report_skip(self):
        for host, tool, reason in [("darwin", "/usr/bin/strace", "Linux required"),
                                   ("linux", None, "strace is unavailable")]:
            with self.subTest(host=host), patch("sys.argv", ["warm_read.py", "unused-binary"]), \
                    patch.object(warm_read.sys, "platform", host), \
                    patch.object(warm_read.shutil, "which", return_value=tool):
                output = StringIO()
                with redirect_stdout(output):
                    self.assertEqual(warm_read.main(), 0)
                self.assertIn("SKIP warm-read syscall contract", output.getvalue())
                self.assertIn(reason, output.getvalue())


if __name__ == "__main__":
    unittest.main()
