// SPDX-License-Identifier: Apache-2.0
//! Trusted directory boundaries resolve links; project children and rule leaves never do.
#![cfg(unix)]
#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::{Command, Output};

struct Fixture {
    _temp: tempfile::TempDir,
    real: PathBuf,
    linked: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(temp.path()).unwrap().join("real");
        let linked = temp.path().join("linked");
        for dir in ["project/src", "config/dctx", "home", "tmp", "outside"] {
            fs::create_dir_all(real.join(dir)).unwrap();
        }
        fs::write(real.join("project/kept.rs"), "pub fn KeptBeacon() {}\n").unwrap();
        fs::write(
            real.join("project/src/lib.rs"),
            "pub fn NestedBeacon() {}\n",
        )
        .unwrap();
        fs::write(real.join("project/src/.gitignore"), "local.rs\n").unwrap();
        fs::write(
            real.join("project/src/local.rs"),
            "pub fn LocalBeacon() {}\n",
        )
        .unwrap();
        fs::write(real.join("project/global.rs"), "pub fn GlobalBeacon() {}\n").unwrap();
        fs::write(real.join("config/dctx/ignore"), "global.rs\n").unwrap();
        fs::write(
            real.join("outside/secret.rs"),
            "pub fn OUTSIDE_SENTINEL() {}\n",
        )
        .unwrap();
        symlink(&real, &linked).unwrap();
        symlink(real.join("config"), real.join("home/.config")).unwrap();
        symlink(
            real.join("outside/secret.rs"),
            real.join("project/escape.rs"),
        )
        .unwrap();
        symlink(real.join("outside"), real.join("project/escape-dir")).unwrap();
        Self {
            _temp: temp,
            real,
            linked,
        }
    }

    fn command(&self, home_config: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.env_clear().current_dir(self.linked.join("project"));
        common::isolated_dirs(&mut command, &self.linked);
        command
            .env("DCTX_CACHE_DIR", self.linked.join("cache/dctx"))
            .env("DCTX_AUTO_CLEAN", "0");
        if home_config {
            command.env_remove("XDG_CONFIG_HOME");
        }
        command
    }

    fn success(&self, args: &[&str], home_config: bool) -> String {
        let output = self.command(home_config).args(args).output().unwrap();
        no_outside_bytes(&output);
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

fn no_outside_bytes(output: &Output) {
    for bytes in [&output.stdout, &output.stderr] {
        assert!(
            !String::from_utf8_lossy(bytes).contains("OUTSIDE_SENTINEL"),
            "{output:?}"
        );
    }
}

#[test]
fn map_find_read_and_pack_accept_linked_boundaries_and_refuse_in_tree_links() {
    let fixture = Fixture::new();
    let place = fixture.linked.join("project");
    let place = place.to_str().unwrap();
    // Exercise both XDG_CONFIG_HOME through a linked ancestor and a dotfile-managed ~/.config.
    for home_config in [false, true] {
        let found = fixture.success(
            &["map", "find", "Beacon", "--place", place, "--json"],
            home_config,
        );
        assert!(found.contains("KeptBeacon"), "{found}");
        assert!(found.contains("NestedBeacon"), "{found}");
        assert!(!found.contains("GlobalBeacon"), "{found}");
        assert!(!found.contains("LocalBeacon"), "{found}");
        let read = fixture.success(
            &["map", "read", "m:kept.rs", "--place", place, "--json"],
            home_config,
        );
        assert!(read.contains("pub fn KeptBeacon"), "{read}");
        let pack = fixture.success(&["pack", place], home_config);
        assert!(pack.contains("pub fn KeptBeacon"), "{pack}");
        assert!(pack.contains("pub fn NestedBeacon"), "{pack}");
        assert!(!pack.contains("GlobalBeacon"), "{pack}");
        assert!(!pack.contains("LocalBeacon"), "{pack}");
        for target in ["m:escape.rs", "m:escape-dir/secret.rs"] {
            let output = fixture
                .command(home_config)
                .args(["map", "read", target, "--place", place, "--json"])
                .output()
                .unwrap();
            no_outside_bytes(&output);
            assert!(!output.status.success(), "{target}: {output:?}");
        }
    }
}

#[test]
fn linked_global_rule_file_is_refused_and_linked_project_rule_file_is_skipped() {
    let fixture = Fixture::new();
    let place = fixture.linked.join("project");
    let rule = fixture.real.join("config/dctx/ignore");
    fs::remove_file(&rule).unwrap();
    symlink(fixture.real.join("outside/secret.rs"), &rule).unwrap();
    let output = fixture
        .command(false)
        .args(["map", "find", "Beacon", "--place"])
        .arg(&place)
        .output()
        .unwrap();
    no_outside_bytes(&output);
    assert!(!output.status.success(), "{output:?}");
    fs::remove_file(rule).unwrap();
    symlink(
        fixture.real.join("outside/secret.rs"),
        fixture.real.join("project/.gitignore"),
    )
    .unwrap();
    // A linked project rule file is skipped with a warning, never read through the link, and the
    // warning names the place-relative path, not the absolute root.
    let output = fixture
        .command(false)
        .arg("pack")
        .arg(&place)
        .output()
        .unwrap();
    no_outside_bytes(&output);
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: skipped rule file .gitignore"),
        "{stderr}"
    );
    for root in [&fixture.real, &fixture.linked] {
        assert!(!stderr.contains(root.to_str().unwrap()), "{stderr}");
    }
}

/// Plugin command under the fixture, with the plugin root explicit or derived from the config home.
fn plugin(fixture: &Fixture, root: Option<&std::path::Path>, args: &[&str]) -> Output {
    let mut command = fixture.command(false);
    if let Some(root) = root {
        command.env("DCTX_PLUGIN_ROOT", root);
    }
    command.args(args).output().unwrap()
}

#[test]
fn plugin_root_under_a_linked_ancestor_works_and_a_linked_root_leaf_is_refused() {
    let fixture = Fixture::new();
    let explicit = fixture.linked.join("plugins");
    // macOS temporary directories live under /var -> /private/var: the explicit root and the
    // HOME/XDG default must both resolve that ancestor instead of failing as Interrupted.
    let default_real = if cfg!(target_os = "macos") {
        fixture
            .real
            .join("home/Library/Application Support/dctx/plugin-config")
    } else {
        fixture.real.join("config/dctx/plugins")
    };
    for (root, real) in [
        (Some(explicit.as_path()), fixture.real.join("plugins")),
        (None, default_real),
    ] {
        let list = plugin(&fixture, root, &["plugin", "list"]);
        assert!(list.status.success(), "{root:?}: {list:?}");
        assert!(String::from_utf8_lossy(&list.stdout).contains("dctx/git"));
        if cfg!(feature = "git-worker") {
            let refused = plugin(&fixture, root, &["git", "status"]);
            let stderr = String::from_utf8_lossy(&refused.stderr);
            assert!(stderr.contains("(ConsentRequired)"), "{root:?}: {stderr}");
            let enabled = plugin(
                &fixture,
                root,
                &["plugin", "enable", "dctx/git", "--approve"],
            );
            assert!(enabled.status.success(), "{root:?}: {enabled:?}");
            assert!(real.join("registry.json").is_file(), "{}", real.display());
            let admitted = plugin(&fixture, root, &["git", "status"]);
            assert!(admitted.status.success(), "{root:?}: {admitted:?}");
        }
    }
    // The root leaf itself stays no-follow, even below a resolved ancestor.
    let target = fixture.real.join("outside/plugins");
    fs::create_dir(&target).unwrap();
    let leaf = fixture.real.join("plugins-link");
    symlink(&target, &leaf).unwrap();
    let linked_leaf = fixture.linked.join("plugins-link");
    for args in [
        &["plugin", "list"][..],
        &["plugin", "enable", "dctx/git", "--approve"],
    ] {
        let output = plugin(&fixture, Some(&linked_leaf), args);
        assert!(!output.status.success(), "{args:?}: {output:?}");
    }
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
}

#[test]
fn output_files_under_a_linked_ancestor_are_written() {
    let fixture = Fixture::new();
    let place = fixture.linked.join("project");
    let place = place.to_str().unwrap();
    let pack = fixture.linked.join("tmp/pack.txt");
    let output = fixture
        .command(false)
        .args(["pack", place, "--output"])
        .arg(&pack)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let packed = fs::read_to_string(fixture.real.join("tmp/pack.txt")).unwrap();
    assert!(packed.contains("pub fn KeptBeacon"), "{packed}");
    let export = fixture.linked.join("tmp/map.json");
    let output = fixture
        .command(false)
        .args(["map", "export", "--place", place, "--output"])
        .arg(&export)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(fixture.real.join("tmp/map.json").is_file());
}
