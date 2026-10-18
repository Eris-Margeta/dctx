//! Binary smoke test for the new map languages in SPEC-MAP-MODE §4–6.
//! Uses only std and Cargo's compiled CLI, with inline temporary source files.
//! Exercises automatic indexing, find, outlines and repeat queries without git.
//! Home, plugins and cache state are isolated from the developer's environment.

#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const SOURCES: &[(&str, &str, &str)] = &[
    ("sample.py", "def PythonBeacon():\n    return 1\n", "PythonBeacon"),
    ("sample.go", "package sample\nfunc GoBeacon() {}\n", "GoBeacon"),
    ("Sample.java", "public class JavaBeacon {}\n", "JavaBeacon"),
    ("sample.c", "int CBeacon(void) { return 1; }\n", "CBeacon"),
    ("sample.cpp", "int CppBeacon() { return 1; }\n", "CppBeacon"),
    ("sample.json", "{\"JsonBeacon\": 1}\n", "JsonBeacon"),
    ("sample.jsonc", "// Settings\n{\"JsoncBeacon\": 1,}\n", "JsoncBeacon"),
    ("sample.toml", "TomlBeacon = 1\n", "TomlBeacon"),
    ("sample.yaml", "YamlBeacon: 1\n", "YamlBeacon"),
    ("Sample.cs", "public class CSharpBeacon {}\n", "CSharpBeacon"),
    ("sample.kt", "fun KotlinBeacon() {}\n", "KotlinBeacon"),
    ("sample.rb", "def RubyBeacon\n  1\nend\n", "RubyBeacon"),
    ("sample.php", "<?php\nfunction PhpBeacon() {}\n", "PhpBeacon"),
    ("sample.swift", "func SwiftBeacon() {}\n", "SwiftBeacon"),
];

struct Repository {
    root: PathBuf,
}

impl Repository {
    fn new() -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        let root = std::env::temp_dir().join(format!("dctx-map-cli-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).expect("unique temporary directory");
        let repo = Self { root };
        for folder in [
            "repo", "home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp",
        ] {
            fs::create_dir(repo.root.join(folder)).expect("isolated directory");
        }
        for (path, source, _) in SOURCES {
            fs::write(repo.root.join("repo").join(path), source).expect("language source");
        }
        fs::write(
            repo.root.join("repo/sample.unknown"),
            "def OpaqueBeacon():\n    return 1\n",
        )
        .expect("unknown extension source");
        repo
    }

    fn run(&self, args: &[&str]) -> String {
        // Cargo's default binary target takes the package name, dctx-cli.
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(self.root.join("repo")).env_clear().args(args);
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("LOCALAPPDATA", "local-app-data"),
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", "cache"),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            command.env(variable, self.root.join(folder));
        }
        let empty_path = self.root.join("empty-path");
        assert_eq!(fs::read_dir(&empty_path).expect("PATH directory").count(), 0);
        assert!(command.get_envs().any(|(key, value)| {
            key == OsStr::new("PATH") && value == Some(empty_path.as_os_str())
        }));
        let output = command.output().expect("run compiled CLI");
        assert!(
            output.status.success(),
            "{args:?}: status {}; stdout: {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(output.stderr.is_empty(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).expect("UTF-8 stdout")
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn map_binary_finds_and_outlines_new_languages_without_git() {
    let repo = Repository::new();
    let help = repo.run(&["map", "--help"]);
    assert!(help.contains("dctx map <map|find|neighbors|explain|read>"), "{help}");
    assert!(help.contains("--in PATH"), "{help}");
    assert!(help.contains("--depth N"), "{help}");

    // Map builds its index automatically; there is no CLI index/admin verb.
    let outline = repo.run(&["map", "map", "m:sample.py", "--depth", "2", "--budget", "2000"]);
    assert!(outline.contains("s:sample.py#PythonBeacon"), "{outline}");
    let mut answers = Vec::new();
    for (path, _, symbol) in SOURCES {
        let found = repo.run(&["map", "find", symbol, "--in", path]);
        let target = format!("m:{path}");
        let mapped = repo.run(&["map", "map", &target]);
        let id = format!("s:{path}#{symbol}");
        assert!(found.contains(&id), "missing {symbol}: {found}");
        assert!(mapped.contains(&id), "missing {symbol}: {mapped}");
        answers.push((found, mapped));
    }

    // The facade index is ephemeral; repeat processes share the host cache root.
    assert!(repo.root.join("cache/ledger.v1").is_file());
    for ((path, _, symbol), (found, mapped)) in SOURCES.iter().zip(answers) {
        assert_eq!(repo.run(&["map", "find", symbol, "--in", path]), found);
        assert_eq!(repo.run(&["map", "map", &format!("m:{path}")]), mapped);
    }

    let missing = repo.run(&["map", "find", "OpaqueBeacon", "--json"]);
    assert!(missing.contains("\"records\":[]"), "{missing}");
    assert!(!missing.contains("OpaqueBeacon"), "{missing}");
    let opaque = repo.run(&["map", "map", "m:sample.unknown", "--json"]);
    assert!(opaque.contains("\"records\":[]"), "{opaque}");
    assert!(!opaque.contains("OpaqueBeacon"), "{opaque}");

    let root = repo.root.clone();
    drop(repo);
    assert!(!root.exists(), "temporary repository was not removed");
}
