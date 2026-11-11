// SPDX-License-Identifier: Apache-2.0
//! Outside definitions, policy confinement and shared multi-root queries through the CLI.
#![forbid(unsafe_code)]
mod common;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture {
    temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let this = Self {
            temp: tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap(),
        };
        for dir in ["app", "lib", "other", "config", "home", "cache", "tmp"] {
            fs::create_dir(this.path(dir)).unwrap();
        }
        fs::write(
            this.path("app/main.py"),
            "from lib.shared import shared_value\n\ndef run():\n    return shared_value()\n",
        )
        .unwrap();
        fs::write(
            this.path("lib/shared.py"),
            "def shared_value():\n    return 42\n",
        )
        .unwrap();
        fs::write(
            this.path("other/other.py"),
            "def other_value():\n    return 7\n",
        )
        .unwrap();
        this
    }
    fn path(&self, path: &str) -> PathBuf {
        self.temp.path().join(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .env_clear()
            .current_dir(self.temp.path())
            .args(args)
            .env("DCTX_CACHE_DIR", self.path("cache"))
            .env("DCTX_AUTO_CLEAN", "0");
        common::isolated_dirs(&mut command, self.temp.path());
        command.output().unwrap()
    }
    fn page(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success() || output.status.code() == Some(1),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
}

#[test]
fn missing_names_and_unresolved_references_report_the_import_target() {
    let f = Fixture::new();
    for verb in ["find", "neighbors", "explain", "read", "grep"] {
        let page = f.page(&["map", verb, "shared_value", "--place", "app"]);
        assert!(
            page.contains("defined outside index: ../lib/shared.py\nadd it with --also lib\n"),
            "{verb}: {page}"
        );
    }
    let page = f.page(&["map", "read", "s:main.py#run", "--place", "app"]);
    assert!(
        page.contains("defined outside index: ../lib/shared.py"),
        "{page}"
    );
    let context = f.page(&["context", "main.py", "--place", "app"]);
    assert!(
        context.contains("defined outside index: ../lib/shared.py"),
        "{context}"
    );
    let unrelated = f.page(&["map", "read", "s:missing.py#unrelated", "--place", "app"]);
    assert!(!unrelated.contains("defined outside index"), "{unrelated}");
    let output = f.run(&["context", "shared_value", "--place", "app", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["defined_outside_index"][0]["path"], "../lib/shared.py");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&*f.temp.path().to_string_lossy()));
}

#[test]
fn also_resolves_symbols_reads_and_both_context_pages() {
    let f = Fixture::new();
    for args in [
        vec![
            "map",
            "find",
            "shared_value",
            "--place",
            "app",
            "--also",
            "lib",
            "--json",
        ],
        vec![
            "map",
            "read",
            "s:lib/shared.py#shared_value",
            "--place",
            "app",
            "--also",
            "lib",
            "--json",
        ],
        vec![
            "context",
            "shared_value",
            "--place",
            "app",
            "--also",
            "lib",
            "--json",
        ],
        vec![
            "pack",
            "app",
            "--also",
            "lib",
            "--for",
            "shared_value",
            "--json",
        ],
    ] {
        let output = f.run(&args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let page = String::from_utf8(output.stdout).unwrap();
        assert!(page.contains("s:lib/shared.py#shared_value"), "{page}");
        assert!(!page.contains("defined_outside_index"), "{page}");
    }
    let page = f.page(&["pack", "app", "--also", "lib"]);
    assert!(
        page.contains("lib/shared.py") && page.contains("return 42"),
        "{page}"
    );
    let page = f.page(&[
        "map",
        "read",
        "lib/shared.py",
        "--place",
        "app",
        "--also",
        "lib",
    ]);
    assert!(page.contains("return 42"), "{page}");
}

#[test]
fn cache_identity_covers_sorted_root_sets_and_live_changes() {
    let f = Fixture::new();
    let first = f.page(&[
        "map", "find", "value", "--place", "app", "--also", "lib", "--also", "other", "--json",
    ]);
    let reordered = f.page(&[
        "map", "find", "value", "--place", "app", "--also", "other", "--also", "lib", "--json",
    ]);
    assert_eq!(first, reordered);
    assert!(
        first.contains("other_value") && first.contains("shared_value"),
        "{first}"
    );
    let separate = f.page(&[
        "map", "find", "value", "--place", "app", "--also", "other", "--json",
    ]);
    assert!(!separate.contains("s:lib/"), "{separate}");
    fs::write(
        f.path("lib/shared.py"),
        "def replaced_value():\n    return 43\n",
    )
    .unwrap();
    let changed = f.page(&[
        "map",
        "find",
        "replaced_value",
        "--place",
        "app",
        "--also",
        "lib",
        "--json",
    ]);
    assert!(
        changed.contains("s:lib/shared.py#replaced_value"),
        "{changed}"
    );
    assert!(!f.path("app/.dctx").exists() && !f.path("lib/.dctx").exists());
}

#[test]
fn denied_and_unimported_paths_are_never_reported_or_indexed() {
    let f = Fixture::new();
    fs::write(f.path("lib/.dctxignore"), "shared.py\n").unwrap();
    let page = f.page(&["map", "find", "shared_value", "--place", "app", "--json"]);
    assert!(
        !page.contains("defined_outside_index") && !page.contains("../lib/"),
        "{page}"
    );
    let page = f.page(&[
        "map",
        "find",
        "shared_value",
        "--place",
        "app",
        "--also",
        "lib",
        "--json",
    ]);
    assert!(!page.contains("s:lib/"), "{page}");
    let page = f.page(&["map", "find", "other_value", "--place", "app"]);
    assert!(!page.contains("../other/"), "{page}");
    fs::write(f.path("app/main.py"), "import '../lib/.env'\n").unwrap();
    fs::write(f.path("lib/.env"), "def hidden_secret():\n    return 1\n").unwrap();
    let page = f.page(&["map", "find", "hidden_secret", "--place", "app"]);
    assert!(!page.contains("../lib/.env"), "{page}");
}

#[cfg(unix)]
#[test]
fn symlink_import_components_are_refused() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(f.path("lib"), f.path("linked")).unwrap();
    fs::write(
        f.path("app/main.py"),
        "from linked.shared import shared_value\n",
    )
    .unwrap();
    let page = f.page(&["map", "find", "shared_value", "--place", "app", "--json"]);
    assert!(
        !page.contains("defined_outside_index") && !page.contains("../linked/"),
        "{page}"
    );
}

#[test]
fn relative_imports_are_scanned_literally_and_large_targets_are_skipped() {
    let f = Fixture::new();
    fs::write(
        f.path("app/main.ts"),
        "import { shared_value } from '../lib/shared';\n",
    )
    .unwrap();
    let page = f.page(&["map", "find", "shared_value", "--place", "app", "--json"]);
    let json: serde_json::Value = serde_json::from_str(&page).unwrap();
    assert_eq!(json["defined_outside_index"].as_array().unwrap().len(), 1);
    fs::write(
        f.path("lib/shared.py"),
        format!(
            "def shared_value():\n    return '{}'\n",
            "x".repeat(65 * 1024)
        ),
    )
    .unwrap();
    let page = f.page(&["map", "find", "shared_value", "--place", "app", "--json"]);
    assert!(!page.contains("defined_outside_index"), "{page}");
}

#[test]
fn hints_quote_directory_names_and_extra_roots_accept_spaces() {
    let f = Fixture::new();
    fs::create_dir(f.path("my lib")).unwrap();
    fs::write(
        f.path("my lib/shared.py"),
        "def spaced_value():\n    return 9\n",
    )
    .unwrap();
    fs::write(
        f.path("app/import.ts"),
        "import { spaced_value } from '../my lib/shared';\n",
    )
    .unwrap();
    let page = f.page(&["map", "find", "spaced_value", "--place", "app"]);
    assert!(
        page.contains("defined outside index: ../my lib/shared.py\nadd it with --also 'my lib'\n"),
        "{page}"
    );
    let page = f.page(&[
        "map",
        "find",
        "spaced_value",
        "--place",
        "app",
        "--also",
        "my lib",
    ]);
    assert!(page.contains("s:my lib/shared.py#spaced_value"), "{page}");
}

#[test]
fn an_indexed_definition_does_not_report_a_duplicate_outside_name() {
    let f = Fixture::new();
    fs::write(
        f.path("app/local.py"),
        "def shared_value():\n    return 1\n",
    )
    .unwrap();
    for verb in ["find", "grep"] {
        let page = f.page(&["map", verb, "shared_value", "--place", "app"]);
        assert!(page.contains("s:local.py#shared_value"), "{page}");
        assert!(!page.contains("defined outside index"), "{page}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn indexed_reads_do_not_invoke_the_import_fallback() {
    let f = Fixture::new();
    // The same name is defined inside and outside the index. A literal import
    // must not trigger an outside probe when its definition is already indexed.
    fs::write(
        f.path("app/main.py"),
        "from lib.shared import shared_value\n\ndef shared_value():\n    return 1\n",
    )
    .unwrap();
    fs::create_dir_all(f.path("app/unrelated/branch")).unwrap();
    fs::write(
        f.path("app/unrelated/branch/other.py"),
        "def other(): pass\n",
    )
    .unwrap();
    let prime = f.run(&["map", "find", "shared_value", "--place", "app"]);
    assert!(prime.status.success());
    let trace = f.path("trace");
    let probe = Command::new("strace")
        .args(["-o"])
        .arg(&trace)
        .args(["--", "/bin/true"])
        .output();
    let probe = match probe {
        Ok(probe) => probe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("strace unavailable; syscall regression requires Linux with strace");
            return;
        }
        Err(error) => panic!("strace probe: {error}"),
    };
    if !probe.status.success() {
        let error = String::from_utf8_lossy(&probe.stderr);
        assert!(
            error.contains("Operation not permitted") || error.contains("Permission denied"),
            "{error}"
        );
        eprintln!("ptrace unavailable; syscall regression requires tracing permission");
        return;
    }
    for target in ["m:main.py", "s:main.py#shared_value"] {
        let output = Command::new("strace")
            .args([
                "-f",
                "-qq",
                "-yy",
                "-s",
                "4096",
                "-e",
                "trace=open,openat,openat2,getdents64",
                "-o",
            ])
            .arg(&trace)
            .arg("--")
            .arg(env!("CARGO_BIN_EXE_dctx-cli"))
            .args(["map", "read", target, "--place", "app", "--json"])
            .env_clear()
            .current_dir(f.temp.path())
            .env("HOME", f.path("home"))
            .env("TMPDIR", f.path("tmp"))
            .env("XDG_CONFIG_HOME", f.path("config"))
            .env("DCTX_CACHE_DIR", f.path("cache"))
            .env("DCTX_AUTO_CLEAN", "0")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let page = String::from_utf8(output.stdout).unwrap();
        assert!(!page.contains("defined_outside_index"), "{page}");
        let syscalls = fs::read_to_string(&trace).unwrap();
        assert!(
            !syscalls.contains("lib/shared.py"),
            "outside fallback ran: {syscalls}"
        );
        assert!(
            !syscalls
                .lines()
                .any(|line| line.contains("getdents64(") && line.contains("/unrelated")),
            "unrelated directory enumerated: {syscalls}"
        );
        let opens = syscalls
            .lines()
            .filter(|line| {
                (line.contains("open(") || line.contains("openat(") || line.contains("openat2("))
                    && line.contains("main.py\"")
            })
            .count();
        assert!(
            (1..=3).contains(&opens),
            "selected source opened {opens} times: {syscalls}"
        );
    }
}
