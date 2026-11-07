// SPDX-License-Identifier: Apache-2.0
//! Discovery and failure isolation without mutating process environment.

use std::{fs, path::Path};

use super::{discover::Context, inspect, render, Scope};

fn context(root: &Path) -> Context {
    Context {
        cwd: root.to_path_buf(),
        running: std::env::current_exe().unwrap(),
        path: vec![root.join("system"), root.join("system"), "relative".into()],
        virtual_env: Some(root.join("venv")),
        explicit: Some(root.join("missing-explicit")),
    }
}

fn executable(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn discovery_deduplicates_paths_but_keeps_all_scopes_and_the_running_binary() {
    let dir = tempfile::tempdir().unwrap();
    let context = context(dir.path());
    let name = if cfg!(windows) { "dctx.exe" } else { "dctx" };
    let system = dir.path().join("system").join(name);
    let relative = dir.path().join("relative").join(name);
    let node = dir.path().join("node_modules/.bin").join(name);
    let venv = dir
        .path()
        .join(if cfg!(windows) {
            "venv/Scripts"
        } else {
            "venv/bin"
        })
        .join(name);
    for path in [&system, &relative, &node, &venv] {
        executable(path);
    }
    let candidates = context.candidates();
    assert_eq!(candidates.len(), 6);
    for path in [
        &system,
        &relative,
        &node,
        &venv,
        &context.running,
        context.explicit.as_ref().unwrap(),
    ] {
        assert!(candidates.contains(path), "{}", path.display());
    }
    assert_eq!(context.scope(&system), Scope::System);
    assert_eq!(context.scope(&node), Scope::Project);
    assert_eq!(context.scope(&venv), Scope::Project);
    assert_eq!(
        context.scope(context.explicit.as_ref().unwrap()),
        Scope::Explicit
    );
}

#[test]
fn inactive_virtualenv_is_project_owned_even_when_discovered_through_path() {
    let dir = tempfile::tempdir().unwrap();
    let context = context(dir.path());
    let path = dir.path().join("inactive/bin/dctx");
    executable(&path);
    fs::write(dir.path().join("inactive/pyvenv.cfg"), b"home = fixture").unwrap();
    assert_eq!(context.scope(&path), Scope::Project);
}

#[test]
fn cargo_install_roots_within_the_current_project_are_project_owned() {
    let dir = tempfile::tempdir().unwrap();
    let mut context = context(dir.path());
    context.cwd = dir.path().join("project/src");
    fs::create_dir_all(&context.cwd).unwrap();
    fs::write(dir.path().join("project/Cargo.toml"), b"fixture").unwrap();
    let install = dir.path().join("project/.tools");
    executable(&install.join("bin/dctx"));
    fs::write(install.join(".crates.toml"), b"fixture").unwrap();
    assert_eq!(context.scope(&install.join("bin/dctx")), Scope::Project);
    let system = dir.path().join("cargo-home");
    executable(&system.join("bin/dctx"));
    fs::write(system.join(".crates.toml"), b"fixture").unwrap();
    assert_eq!(context.scope(&system.join("bin/dctx")), Scope::System);
}

#[test]
fn size_and_checksum_are_read_only_and_missing_caches_are_zero() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("cache");
    assert_eq!(inspect::directory_size(&root).unwrap(), 0);
    assert!(!root.exists());
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("a"), b"abc").unwrap();
    fs::write(root.join("nested/b"), b"12345").unwrap();
    assert_eq!(inspect::directory_size(&root).unwrap(), 8);
    assert_eq!(
        inspect::checksum(&root.join("a")).unwrap(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(fs::read(root.join("a")).unwrap(), b"abc");
}

#[cfg(unix)]
#[test]
fn cache_accounting_does_not_follow_symlinks_or_loops() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("cache");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("owned"), b"123").unwrap();
    fs::write(dir.path().join("outside"), b"123456789").unwrap();
    symlink(dir.path(), root.join("loop")).unwrap();
    symlink(dir.path().join("outside"), root.join("linked-file")).unwrap();
    assert_eq!(inspect::directory_size(&root).unwrap(), 3);
    symlink(&root, dir.path().join("linked-root")).unwrap();
    assert!(inspect::directory_size(&dir.path().join("linked-root")).is_err());
}

#[test]
fn versions_cannot_escape_the_cache_namespace() {
    assert_eq!(
        inspect::version_line("dctx 1.2.3 (abc123)\n"),
        Some("1.2.3".into())
    );
    assert_eq!(inspect::namespace("2.4.0-rc.1+build"), Some("2.4".into()));
    for text in [
        "dctx 1.2/../../secret",
        "other 1.2.3",
        "dctx 1.2",
        "dctx 1.2.x",
        "dctx 1.2.3/4",
        "dctx 1.2.3\u{1b}",
    ] {
        assert!(inspect::version_line(text).is_none(), "{text}");
    }
}

#[test]
fn missing_explicit_override_is_a_row_with_errors_and_valid_json_escaping() {
    let dir = tempfile::tempdir().unwrap();
    let mut context = context(dir.path());
    context.explicit = Some(dir.path().join("missing\"\n\u{1b}"));
    let cache = dir.path().join("cache");
    let rows = inspect::binaries(&context, Ok(cache.clone()));
    assert_eq!(rows.len(), 2);
    let missing = rows
        .iter()
        .find(|row| row.scope == Scope::Explicit)
        .unwrap();
    assert!(!missing.errors.is_empty());
    assert!(missing.version.is_none());
    assert!(missing.sha256.is_none());
    let running = rows.iter().find(|row| row.running).unwrap();
    assert_eq!(running.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(running.cache_size_bytes, Some(0));
    assert!(!cache.exists());
    let text = render::report(&rows, true);
    dctx_plugin_protocol::decode_object(text.as_bytes(), dctx_plugin_protocol::DecodeLimits::WIRE)
        .unwrap();
    assert!(text.contains("missing\\\"\\n\\u001b"));
    assert!(text.contains("\"version\":null"));
    assert!(!text.contains('\u{1b}'));
    assert!(!render::report(&rows, false).contains('\u{1b}'));
}

#[cfg(unix)]
#[test]
fn probes_are_bounded_and_bad_candidates_do_not_hide_other_installations() {
    let dir = tempfile::tempdir().unwrap();
    let candidate = dir.path().join("dctx");
    executable(&candidate);
    fs::write(&candidate, b"#!/bin/sh\nprintf 'dctx 9.8.7 (fixture)\\n'\n").unwrap();
    assert_eq!(inspect::probe_version(&candidate).unwrap(), "9.8.7");
    fs::write(&candidate, b"#!/bin/sh\nexit 42\n").unwrap();
    assert!(inspect::probe_version(&candidate)
        .unwrap_err()
        .contains("42"));
    fs::write(&candidate, b"#!/bin/sh\nwhile :; do printf 'noise'; done\n").unwrap();
    assert!(inspect::probe_version(&candidate).is_err());
    fs::write(&candidate, b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
    let start = std::time::Instant::now();
    assert!(inspect::probe_version(&candidate)
        .unwrap_err()
        .contains("timed out"));
    assert!(start.elapsed() < std::time::Duration::from_secs(10));
}
