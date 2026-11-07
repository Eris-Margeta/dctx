// SPDX-License-Identifier: Apache-2.0
//! Black-box installation diagnostics and version-scoped cache isolation.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("project")).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .env_clear()
            .current_dir(self.root.path().join("project"))
            .env("PATH", self.root.path().join("system"))
            .env("DCTX_CACHE_DIR", self.root.path().join("cache"))
            .env("HOME", self.root.path().join("home"))
            .env("LOCALAPPDATA", self.root.path().join("local"));
        // Keep Windows process startup independent of any developer environment.
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        let output = self.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn root(&self) -> &Path {
        self.root.path()
    }
}

#[test]
fn doctor_and_verbose_version_never_create_a_cache_or_project_files() {
    let fixture = Fixture::new();
    let verbose = fixture.run(&["--version", "--verbose"]);
    let text = String::from_utf8(verbose.stdout).unwrap();
    assert!(text.contains("executable: "));
    assert!(text.contains("scope: system\n"));
    let output = fixture.run(&["doctor", "--json"]);
    assert!(output.stderr.is_empty());
    let json = String::from_utf8(output.stdout).unwrap();
    dctx_plugin_protocol::decode_object(json.as_bytes(), dctx_plugin_protocol::DecodeLimits::WIRE)
        .unwrap();
    assert!(json.starts_with("{\"schema_version\":1,\"binaries\":["));
    assert!(json.contains("\"running\":true"));
    assert!(json.contains("\"cache_size_bytes\":0"));
    assert!(json.contains("\"errors\":[]"));
    let output = fixture.run(&["doctor"]);
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("scope: system"));
    assert!(text.contains("sha256: "));
    assert!(!fixture.root().join("cache").exists());
    assert_eq!(
        fs::read_dir(fixture.root().join("project"))
            .unwrap()
            .count(),
        0
    );
    let output = fixture
        .command()
        .args(["doctor", "--bogus"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn old_and_other_engine_caches_survive_new_indexing_and_purge() {
    let fixture = Fixture::new();
    let cache = fixture.root().join("cache");
    let namespace = dctx_cli::host::engine_cache_namespace();
    fs::create_dir_all(cache.join("99.7")).unwrap();
    fs::write(
        cache.join("ledger.v1"),
        b"legacy bytes must not be read or overwritten",
    )
    .unwrap();
    fs::write(cache.join("99.7/ledger.v1"), b"other engine").unwrap();
    fs::write(
        fixture.root().join("project/example.rs"),
        b"fn versioned() {}\n",
    )
    .unwrap();
    let output = fixture.run(&["map", "find", "versioned"]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("versioned"));
    assert!(cache.join(namespace).join("ledger.v1").is_file());
    assert_eq!(
        fs::read(cache.join("ledger.v1")).unwrap(),
        b"legacy bytes must not be read or overwritten"
    );
    fixture.run(&["cache", "purge", "--all", "--yes"]);
    assert_eq!(
        fs::read(cache.join("ledger.v1")).unwrap(),
        b"legacy bytes must not be read or overwritten"
    );
    assert_eq!(
        fs::read(cache.join("99.7/ledger.v1")).unwrap(),
        b"other engine"
    );
}

#[cfg(unix)]
#[test]
fn all_discovery_sources_report_distinct_versions_checksums_and_cache_sizes() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let fixture = Fixture::new();
    let install = |relative: &str, version: &str| {
        let path = fixture.root().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            format!("#!/bin/sh\nprintf 'dctx {version} (fixture)\\n'\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    install("system/dctx", "4.5.6");
    install("project/node_modules/.bin/dctx", "7.8.9");
    install("venv/bin/dctx", "10.11.12");
    install("explicit", "13.14.15");
    fs::create_dir_all(fixture.root().join("cache/7.8/objects")).unwrap();
    fs::write(
        fixture.root().join("cache/7.8/objects/example"),
        b"123456789",
    )
    .unwrap();
    fs::write(fixture.root().join("cache/legacy"), b"ignored").unwrap();
    let before = fs::read(fixture.root().join("cache/7.8/objects/example")).unwrap();
    let aliases = fixture.root().join("aliases");
    fs::create_dir(&aliases).unwrap();
    symlink(env!("CARGO_BIN_EXE_dctx-cli"), aliases.join("dctx")).unwrap();
    let path = std::env::join_paths([
        fixture.root().join("system"),
        aliases.clone(),
        fixture.root().join("system"),
    ])
    .unwrap();
    let output = fixture
        .command()
        .env("PATH", path)
        .env("VIRTUAL_ENV", fixture.root().join("venv"))
        .env("DCTX_BIN", fixture.root().join("explicit"))
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let json = String::from_utf8(output.stdout).unwrap();
    let decoded = dctx_plugin_protocol::decode_object(
        json.as_bytes(),
        dctx_plugin_protocol::DecodeLimits::WIRE,
    )
    .unwrap();
    for row in decoded.get("binaries").unwrap().as_array().unwrap() {
        let checksum = row.get("sha256").unwrap().as_str().unwrap();
        assert_eq!(checksum.len(), 64);
        assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    assert_eq!(json.matches("\"path\":").count(), 6, "{json}");
    assert_eq!(json.matches("\"scope\":\"project\"").count(), 2, "{json}");
    assert_eq!(json.matches("\"scope\":\"explicit\"").count(), 1);
    assert_eq!(json.matches("\"running\":true").count(), 2);
    assert_eq!(json.matches("\"errors\":[]").count(), 6, "{json}");
    assert!(json.contains("\"cache_size_bytes\":9"));
    for version in ["4.5.6", "7.8.9", "10.11.12", "13.14.15"] {
        assert!(
            json.contains(&format!("\"version\":\"{version}\"")),
            "{json}"
        );
    }
    assert_eq!(
        fs::read(fixture.root().join("cache/7.8/objects/example")).unwrap(),
        before
    );
    assert!(!fixture.root().join("cache/4.5").exists());
    assert!(!fixture.root().join("cache/ledger.v1").exists());
    assert!(!fixture.root().join("project/.dctx").exists());

    // Scope metadata belongs only to diagnostics, even when invoked through a project alias.
    let project_alias = fixture.root().join("project/node_modules/.bin/dctx");
    fs::remove_file(&project_alias).unwrap();
    symlink(env!("CARGO_BIN_EXE_dctx-cli"), &project_alias).unwrap();
    fs::write(
        fixture.root().join("project/source.rs"),
        b"fn stable() {}\n",
    )
    .unwrap();
    let expected = fixture.run(&["pack", ".", "--no-auto-clean"]);
    let command = fixture.command();
    // Reuse the fixture environment for both executable locations.
    let aliased = || {
        let mut alias = Command::new(&project_alias);
        alias
            .env_clear()
            .envs(
                command
                    .get_envs()
                    .filter_map(|(key, value)| value.map(|value| (key, value))),
            )
            .current_dir(fixture.root().join("project"));
        alias
    };
    let actual = aliased()
        .args(["pack", ".", "--no-auto-clean"])
        .output()
        .unwrap();
    assert!(actual.status.success());
    assert_eq!(actual.stdout, expected.stdout);
    let details = aliased().args(["--version", "--verbose"]).output().unwrap();
    assert!(details.status.success());
    assert!(String::from_utf8_lossy(&details.stdout).contains("scope: project\n"));
}
