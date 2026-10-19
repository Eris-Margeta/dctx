//! Stderr profile for a five-file map fixture, and byte-identical stdout.
//! Covers DCTX-SERIOUS-GAPS G7 and DCTX-COLD-SCAN C5.
//! Depends on the compiled dctx-cli and temporary Rust sources. No network.
//! `--profile` and `DCTX_PROFILE=1` both opt in. Durations are ordered, never bounded.
//! File bytes and symbol counts are the measured values. Finalization is outside the file sum.

#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

const STAGES: [&str; 9] = [
    "walk",
    "read",
    "hash",
    "parse/lex",
    "span conversion",
    "graph assembly",
    "canonicalize/seal",
    "persist/reuse",
    "query",
];

struct Repository {
    root: PathBuf,
    files: Vec<(String, u64, u64)>,
}

impl Repository {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dctx-e02m-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("repo")).expect("repo");
        for folder in ["home", "config", "cache", "plugins", "map-cache", "local-app-data", "empty-path", "tmp"] {
            fs::create_dir(root.join(folder)).expect("isolated directory");
        }
        let specs = [
            ("a.rs", "only_once", 0u64),
            ("b.rs", "beta", 1),
            ("c.rs", "gamma", 2),
            ("d.rs", "delta", 3),
            ("e.rs", "epsilon", 4),
        ];
        let mut files = Vec::new();
        for (name, symbol, extra) in specs {
            let mut body = format!("fn {symbol}() {{}}\n");
            for index in 0..extra {
                body.push_str(&format!("fn {symbol}_x{index}() {{}}\n"));
            }
            fs::write(root.join("repo").join(name), &body).expect("source");
            files.push((name.to_string(), body.len() as u64, extra + 1));
        }
        Self { root, files }
    }

    fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    fn output(&self, cache: &str, args: &[&str], profile_env: bool) -> Output {
        let cache_dir = self.root.join(cache);
        fs::create_dir_all(&cache_dir).expect("cache");
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command.current_dir(self.repo()).env_clear().args(args).env("DCTX_AUTO_CLEAN", "0").env("NO_COLOR", "1");
        if profile_env {
            command.env("DCTX_PROFILE", "1");
        }
        for (variable, folder) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("LOCALAPPDATA", "local-app-data"),
            ("DCTX_PLUGIN_ROOT", "plugins"),
            ("DCTX_CACHE_DIR", cache),
            ("DCTX_MAP_CACHE", "map-cache"),
            ("PATH", "empty-path"),
            ("TMPDIR", "tmp"),
            ("TMP", "tmp"),
            ("TEMP", "tmp"),
        ] {
            let path = if variable == "DCTX_CACHE_DIR" { cache_dir.clone() } else { self.root.join(folder) };
            command.env(variable, path);
        }
        command.output().expect("run compiled CLI")
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stages_and_files(stderr: &str) -> (Vec<(&str, u128)>, Vec<(u128, u64, u64, &str)>) {
    let mut stages = Vec::new();
    let mut files = Vec::new();
    for line in stderr.lines() {
        if let Some(rest) = line.strip_prefix("stage ") {
            let (name, value) = rest.rsplit_once(' ').expect("stage line");
            stages.push((name, value.parse::<u128>().expect("stage nanos")));
        } else if let Some(rest) = line.strip_prefix("file ") {
            let mut parts = rest.splitn(4, ' ');
            let nanos = parts.next().and_then(|value| value.parse().ok()).expect("file nanos");
            let bytes = parts.next().and_then(|value| value.parse().ok()).expect("file bytes");
            let symbols = parts.next().and_then(|value| value.parse().ok()).expect("file symbols");
            let path = parts.next().expect("file path");
            files.push((nanos, bytes, symbols, path));
        }
    }
    (stages, files)
}

fn assert_profile(stderr: &str, expected: &[(String, u64, u64)]) {
    let (stages, files) = stages_and_files(stderr);
    let names: Vec<_> = stages.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, STAGES, "{stderr}");
    assert_eq!(files.len(), 5, "{stderr}");
    assert!(files.windows(2).all(|pair| pair[0].0 >= pair[1].0), "{stderr}");
    let mut seen: Vec<_> = files.iter().map(|(_, bytes, symbols, path)| ((*path).to_string(), *bytes, *symbols)).collect();
    seen.sort();
    let mut want = expected.to_vec();
    want.sort();
    assert_eq!(seen, want, "{stderr}");
    let file_sum: u128 = files.iter().map(|row| row.0).sum();
    let stage_sum: u128 = stages
        .iter()
        .filter(|(name, _)| matches!(*name, "read" | "hash" | "parse/lex" | "span conversion" | "graph assembly"))
        .map(|(_, ns)| *ns)
        .sum();
    assert_eq!(file_sum, stage_sum, "{stderr}");
    let distinct: std::collections::BTreeSet<_> = files.iter().map(|row| row.3).collect();
    assert_eq!(distinct.len(), 5, "{stderr}");
}

#[test]
fn profiled_map_reports_stages_and_keeps_stdout() {
    let repo = Repository::new();
    let place = repo.repo().display().to_string();
    let args = ["map", "find", "only_once", "--place", place.as_str()];
    let plain = repo.output("cache-off", &args, false);
    let flagged = repo.output("cache-flag", &["map", "find", "only_once", "--profile", "--place", place.as_str()], false);
    let enved = repo.output("cache-env", &args, true);
    for (label, output) in [("plain", &plain), ("flag", &flagged), ("env", &enved)] {
        assert!(
            output.status.success(),
            "{label}: status {}; stdout {}; stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    assert!(plain.stderr.is_empty(), "plain stderr {}", String::from_utf8_lossy(&plain.stderr));
    assert_eq!(flagged.stdout, plain.stdout, "flag stdout diverged");
    assert_eq!(enved.stdout, plain.stdout, "env stdout diverged");
    assert!(!plain.stdout.is_empty(), "find produced no stdout");
    assert!(!plain.stdout.windows(b"dctx profile".len()).any(|window| window == b"dctx profile"));
    let flagged_err = String::from_utf8(flagged.stderr).expect("flag stderr utf-8");
    let enved_err = String::from_utf8(enved.stderr).expect("env stderr utf-8");
    assert_profile(&flagged_err, &repo.files);
    assert_profile(&enved_err, &repo.files);
    assert!(!flagged.stdout.windows(flagged_err.len().max(1)).any(|window| window == flagged_err.as_bytes()));
}
