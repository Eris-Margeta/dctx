// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs at the CLI's library and process boundaries.
//!
//! Byte APIs receive invalid UTF-8 and every prefix of a small valid sample;
//! string APIs receive the valid prefixes plus NUL/control/Unicode strings.
//! Limits are exercised where the API defines one (files, RPC, roots, targets,
//! numeric flags). Unbounded presentation APIs use a 64 KiB stress input.
//! Interactive terminal actions are covered through the non-terminal refusal.
//! Fixtures, subprocess homes and caches stay in the worktree. No random inputs,
//! elapsed-time assertions, network operations or process-global env mutations.
#![forbid(unsafe_code)]

mod common;

use dctx_cli::{cache, doctor, exit, help, host, languages, pack, template, tui};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const RPC_CAP: usize = 1024 * 1024;
const TEMPLATE: &str = "[DTS]\nversion = 2\n[SELECT:code]\nroots = [\"src\"]\nlanguages = [\"rust\"]\n[INDEX:code]\nfacts = [\"symbols\"]\nparser = \"structural\"\n[BUDGET:agent]\nestimated_tokens = 500\nrecords = 40\n[VIEW:pack]\nselection = \"code\"\nbudget = \"agent\"\nlayout = \"SNAPSHOT\"\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.codex/n1113/fixtures")
            .join(name);
        if root.exists() {
            fs::remove_dir_all(&root).expect("remove prior fixture");
        }
        fs::create_dir_all(&root).expect("fixture");
        Self(fs::canonicalize(root).expect("absolute fixture"))
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .current_dir(&self.0)
            .env_clear()
            .env("DCTX_AUTO_CLEAN", "0")
            .env("DCTX_CACHE_DIR", self.0.join("state/cache"))
            .env("DCTX_MAP_CACHE", self.0.join("state/map"))
            .env("NO_COLOR", "1")
            .stdin(Stdio::null());
        common::isolated_dirs(&mut command, &self.0.join("state"));
        command
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("CLI")
    }

    fn rpc(&self, bytes: &[u8]) -> Vec<u8> {
        let mut child = self
            .command()
            .arg("rpc")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("RPC");
        let mut input = child.stdin.take().expect("stdin");
        // Drain replies while writing so large suites cannot fill both pipes.
        let output = std::thread::scope(|scope| {
            let writer = scope.spawn(move || input.write_all(bytes).expect("requests"));
            let output = child.wait_with_output().expect("RPC exit");
            writer.join().expect("writer");
            output
        });
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        output.stdout
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove fixture");
    }
}

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn same_process_result(left: &Output, right: &Output) {
    assert_eq!(left.status.code(), right.status.code());
    assert_eq!(left.stdout, right.stdout);
    assert_eq!(left.stderr, right.stderr);
    assert!(!String::from_utf8_lossy(&left.stderr).contains("panicked"));
}

fn actionable(failure: &exit::Failure) {
    assert!((1..=3).contains(&failure.code), "{failure:?}");
    assert!(!failure.message.is_empty());
    assert!(!failure.hint.trim().is_empty(), "{failure:?}");
    assert_eq!(failure.render(), failure.clone().render());
}

fn replies(bytes: &[u8]) -> Vec<Value> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("JSON response"))
        .collect()
}

#[test]
fn argument_entry_points_reject_empty_one_byte_and_all_flag_prefixes() {
    let fixture = Fixture::new("argument-entry-points");
    let mut corpus = vec![
        String::new(),
        "x".into(),
        "\0".into(),
        "\u{1b}".into(),
        "é🦀".into(),
        "x".repeat(65_536),
    ];
    let sample = "--unknown=é🦀";
    corpus.extend((0..=sample.len()).filter_map(|end| sample.get(..end).map(str::to_owned)));
    for word in corpus {
        let input = vec![word.clone()];
        for failure in [
            cache::output_in(&input, Some(&fixture.0)).expect_err("cache word"),
            template::execute(&input).expect_err("template word"),
            pack::output(&input).expect_err("pack word"),
            languages::output(&input).expect_err("language flag"),
            doctor::run(&input).expect_err("doctor flag"),
        ] {
            actionable(&failure);
        }
        assert!(cache::run_in(&input, Some(&fixture.0)).is_err());
        assert!(pack::execute(&input).is_err());
        let mut written = Vec::new();
        actionable(&pack::write_output(&input, &mut written).expect_err("pack writer"));
        assert!(written.is_empty());
        assert!(languages::run(&input).is_err());
        assert!(cache::run(&input).is_err());
        assert!(template::run(&input).is_err());
        assert!(pack::run(&input).is_err());
        assert!(host::also_arguments(&input).is_ok());
    }
    assert!(cache::output_in(&[], Some(&fixture.0)).is_err());
    assert!(template::execute(&[]).is_err());
    assert!(pack::output(&[]).is_err());
    assert_eq!(languages::page(&[]).expect("default"), languages::text());
}

#[test]
fn all_process_routes_refuse_hostile_flags_with_reproducible_diagnostics() {
    let fixture = Fixture::new("process-routes");
    let routes: &[&[&str]] = &[
        &[],
        &["pack"],
        &["pack", "--for", "x"],
        &["template", "build"],
        &["template", "list"],
        &["cache", "status"],
        &["cache", "gc"],
        &["cache", "purge"],
        &["cache", "config"],
        &["cache", "auto"],
        &["languages"],
        &["doctor"],
        &["rpc"],
        &["context"],
        &["agent-setup"],
        &["map", "find"],
        &["map", "read"],
        &["map", "map"],
        &["map", "neighbors"],
        &["map", "explain"],
        &["map", "skeleton"],
        &["map", "grep"],
        &["map", "hubs"],
        &["map", "path"],
        &["map", "trace"],
        &["map", "impact"],
        &["map", "export"],
        &["read"],
        &["trace"],
        &["impact"],
        &["plugin", "list"],
        &["plugin", "info"],
        &["plugin", "install"],
        &["plugin", "enable"],
        &["plugin", "disable"],
        &["plugin", "remove"],
        &["plugin", "update"],
        &["plugin", "verify"],
        &["plugin", "search"],
        &["git"],
        &["--internal-plugin-worker"],
    ];
    for route in routes {
        let mut input = route.to_vec();
        input.push("--hostile-unknown");
        let first = fixture.cli(&input);
        same_process_result(&first, &fixture.cli(&input));
        assert!(!first.status.success(), "{input:?}: {first:?}");
        let stderr = String::from_utf8_lossy(&first.stderr);
        assert!(
            stderr.contains("hint:")
                || stderr.contains("help")
                || stderr.contains("usage")
                || stderr.contains("Usage"),
            "{input:?}: {stderr}"
        );
    }
    let empty = fixture.cli(&[]);
    assert_eq!(empty.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&empty.stderr).contains("dctx pack"));
    assert_eq!(fixture.cli(&[""]).status.code(), Some(2));
    assert_eq!(fixture.cli(&["x"]).status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn every_process_route_rejects_non_utf8_arguments_before_dispatch() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let fixture = Fixture::new("non-utf8-arguments");
    for route in [
        "pack",
        "map",
        "cache",
        "template",
        "languages",
        "doctor",
        "rpc",
        "context",
        "read",
        "trace",
        "impact",
        "agent-setup",
        "plugin",
        "git",
        "--internal-plugin-worker",
    ] {
        for invalid in [vec![0xff], vec![0xc0, 0xaf], vec![0xf0, 0x9f, 0xa6]] {
            let output = fixture
                .command()
                .arg(route)
                .arg(OsString::from_vec(invalid))
                .output()
                .expect("invalid argument");
            assert_eq!(output.status.code(), Some(2), "{route}: {output:?}");
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8(output.stderr).expect("diagnostic UTF-8");
            assert!(stderr.contains("argument 2 is not valid UTF-8"));
            assert!(stderr.contains("hint:"));
        }
    }
}

#[test]
fn rpc_empty_one_byte_and_every_truncation_recover_in_the_same_stream() {
    let fixture = Fixture::new("rpc-truncation");
    assert!(fixture.rpc(b"").is_empty());
    let sample = br#"{"id":"sample","verb":"hello","args":[]}"#;
    let mut wire = Vec::new();
    for end in 0..sample.len() {
        wire.extend_from_slice(&sample[..end]);
        wire.push(b'\n');
    }
    for bytes in [&[0xff][..], &[0xc0, 0xaf], &[0xf0, 0x9f, 0xa6], b"x", b"\0"] {
        wire.extend_from_slice(bytes);
        wire.push(b'\n');
    }
    wire.extend_from_slice(sample); // EOF also completes a frame.
    let first = fixture.rpc(&wire);
    assert_eq!(first, fixture.rpc(&wire));
    let rows = replies(&first);
    assert_eq!(rows.len(), sample.len() + 6);
    for row in &rows[..rows.len() - 1] {
        assert_eq!(row["error"]["code"], 2, "{row}");
        assert!(!row["error"]["hint"].as_str().expect("hint").is_empty());
    }
    assert!(rows.last().expect("hello").get("result").is_some());
}

#[test]
fn rpc_exact_limit_over_limit_and_crlf_preserve_frame_alignment() {
    let fixture = Fixture::new("rpc-limit");
    let hello = br#"{"id":1,"verb":"hello"}"#;
    let mut wire = hello.to_vec();
    wire.resize(RPC_CAP, b' ');
    wire.push(b'\n');
    wire.extend(std::iter::repeat_n(b' ', RPC_CAP + 1));
    wire.push(b'\n');
    wire.extend_from_slice(hello);
    wire.extend_from_slice(b"\r\n");
    let first = fixture.rpc(&wire);
    assert_eq!(first, fixture.rpc(&wire));
    let rows = replies(&first);
    assert_eq!(rows.len(), 3);
    assert!(rows[0].get("result").is_some());
    assert!(rows[1]["error"]["message"]
        .as_str()
        .expect("overflow")
        .contains("1048576"));
    assert!(rows[2].get("result").is_some());
    let over_at_eof = vec![b'x'; RPC_CAP + 1];
    assert_eq!(replies(&fixture.rpc(&over_at_eof)).len(), 1);
}

#[test]
fn rpc_duplicates_reordering_handshake_and_repeated_calls_are_deterministic() {
    let fixture = Fixture::new("rpc-order");
    let wire = concat!(
        "{\"id\":0,\"verb\":\"languages\"}\n",
        "{\"id\":1,\"verb\":\"hello\",\"args\":[\"x\"]}\n",
        "{\"id\":2,\"verb\":\"languages\"}\n",
        "{\"id\":3,\"id\":4,\"verb\":\"hello\"}\n",
        "{\"id\":5,\"verb\":\"hello\",\"verb\":\"languages\"}\n",
        "{\"id\":6,\"verb\":\"hello\",\"args\":[],\"args\":[]}\n",
        "{\"args\":[],\"verb\":\"hello\",\"id\":7}\n",
        "{\"id\":7,\"verb\":\"hello\"}\n",
        "{\"id\":8,\"verb\":\"languages\"}\n",
        "{\"verb\":\"languages\",\"args\":[],\"id\":8}\n"
    );
    let first = fixture.rpc(wire.as_bytes());
    assert_eq!(first, fixture.rpc(wire.as_bytes()));
    let rows = replies(&first);
    for row in &rows[..6] {
        assert_eq!(row["error"]["code"], 2, "{row}");
    }
    assert_eq!(rows[6], rows[7]);
    assert_eq!(rows[8], rows[9]);
}

#[test]
fn every_rpc_verb_survives_invalid_arguments_then_answers_hello() {
    let fixture = Fixture::new("rpc-verbs");
    let mut wire = b"{\"id\":0,\"verb\":\"hello\"}\n".to_vec();
    for verb in dctx_cli::rpc::Verb::ALL {
        for value in ["", "x", "--hostile", "\0", "é"] {
            let request = json!({"id":verb.as_str(), "verb":verb, "args":["--hostile",value]});
            wire.extend_from_slice(request.to_string().as_bytes());
            wire.push(b'\n');
        }
    }
    wire.extend_from_slice(b"{\"id\":0,\"verb\":\"hello\"}\n");
    let bytes = fixture.rpc(&wire);
    assert_eq!(bytes, fixture.rpc(&wire));
    let rows = replies(&bytes);
    assert_eq!(rows.first(), rows.last());
    for row in &rows[1..rows.len() - 1] {
        assert_eq!(row["error"]["code"], 2, "{row}");
        assert!(!row["error"]["hint"].as_str().expect("hint").is_empty());
    }
}

#[test]
fn concurrent_rpc_sessions_have_independent_handshakes_and_identical_bytes() {
    let fixture = Fixture::new("rpc-concurrency");
    let wire = b"{\"id\":1,\"verb\":\"hello\"}\n{\"id\":2,\"verb\":\"languages\"}\n";
    // Create the shared, read-only subprocess environment before starting threads.
    drop(fixture.command());
    let expected = fixture.rpc(wire);
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..4).map(|_| scope.spawn(|| fixture.rpc(wire))).collect();
        for thread in threads {
            assert_eq!(thread.join().expect("RPC thread"), expected);
        }
    });
    let row = replies(&fixture.rpc(b"{\"id\":2,\"verb\":\"languages\"}"));
    assert_eq!(row[0]["error"]["code"], 2);
}

#[test]
fn bounded_explicit_file_reads_cover_all_prefixes_and_byte_values() {
    let fixture = Fixture::new("explicit-read");
    let file = fixture.0.join("bytes");
    let sample = b"x\0\xff\xc0\xaf\xf0\x9f\xa6\x80";
    for end in 0..=sample.len() {
        fs::write(&file, &sample[..end]).expect("bytes");
        for cap in [0, 1, end as u64, 64, u64::MAX] {
            let first = host::read_explicit_file(&file, cap);
            let second = host::read_explicit_file(&file, cap);
            if end as u64 <= cap {
                assert_eq!(first.expect("within cap"), sample[..end]);
                assert_eq!(second.expect("repeat"), sample[..end]);
            } else {
                assert!(first.is_err());
                assert!(second.is_err());
            }
        }
    }
    assert!(host::read_explicit_file(Path::new(""), 1).is_err());
    assert!(host::read_explicit_file(&fixture.0, 1).is_err());
    fs::write(&file, vec![b'x'; 1025]).expect("over cap");
    assert!(host::read_explicit_file(&file, 1024).is_err());
    fs::write(&file, vec![b'x'; 1024]).expect("at cap");
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let file = &file;
            scope.spawn(move || {
                assert_eq!(
                    host::read_explicit_file(file, 1024).expect("parallel read"),
                    vec![b'x'; 1024]
                )
            });
        }
    });
}

#[test]
fn template_source_all_prefixes_utf8_and_size_boundary() {
    let fixture = Fixture::new("template-source");
    let source = fixture.0.join("source");
    let input = args(&["template", "build", source.to_str().expect("path")]);
    for end in 0..=TEMPLATE.len() {
        fs::write(&source, &TEMPLATE.as_bytes()[..end]).expect("prefix");
        let first = template::execute(&input);
        assert_eq!(first, template::execute(&input), "offset {end}");
        if let Err(failure) = first {
            actionable(&failure);
        }
    }
    for bytes in [vec![0xff], vec![0xc0, 0xaf], vec![0xf0, 0x9f, 0xa6]] {
        fs::write(&source, bytes).expect("invalid UTF-8");
        let failure = template::execute(&input).expect_err("invalid text");
        assert_eq!(failure.code, 2);
        assert!(failure.hint.contains("UTF-8"));
    }
    let mut bytes = TEMPLATE.as_bytes().to_vec();
    bytes.resize(RPC_CAP, b'\n');
    fs::write(&source, &bytes).expect("maximum source");
    assert!(template::execute(&input).is_ok());
    bytes.push(b'\n');
    fs::write(&source, bytes).expect("oversized source");
    assert_eq!(template::execute(&input).expect_err("file cap").code, 3);
}

#[test]
fn template_compiled_plan_all_truncations_and_repeated_builds() {
    let fixture = Fixture::new("template-plan");
    let source = fixture.0.join("source");
    let plans = fixture.0.join("plans");
    fs::create_dir(&plans).expect("plans");
    let plan = plans.join("plan.bin");
    fs::write(&source, TEMPLATE).expect("source");
    let build = args(&[
        "template",
        "build",
        source.to_str().unwrap(),
        "--output",
        plan.to_str().unwrap(),
    ]);
    let first = template::execute(&build).expect("build");
    let bytes = fs::read(&plan).expect("plan");
    assert_eq!(first, template::execute(&build).expect("rebuild"));
    assert_eq!(bytes, fs::read(&plan).expect("identical plan"));
    let list = args(&["template", "list", "--dir", plans.to_str().unwrap()]);
    for end in 0..bytes.len() {
        fs::write(&plan, &bytes[..end]).expect("prefix");
        let report = template::execute(&list).expect("refusal report");
        assert!(report.starts_with("refused "), "offset {end}: {report}");
        assert_eq!(report, template::execute(&list).expect("repeat list"));
    }
    fs::write(&plan, &bytes).expect("valid plan");
    assert!(template::execute(&list)
        .expect("valid list")
        .starts_with("ok "));
}

#[test]
fn template_listing_is_sorted_independently_of_creation_order() {
    let fixture = Fixture::new("template-sort");
    let list = args(&["template", "list", "--dir", fixture.0.to_str().unwrap()]);
    for name in ["z.bin", "a.bin", "m.bin"] {
        fs::write(fixture.0.join(name), b"x").expect("bad plan");
    }
    let first = template::execute(&list).expect("list");
    let lines: Vec<_> = first.lines().collect();
    assert!(lines[0].contains("a.bin"), "{first}");
    assert!(lines[1].contains("m.bin"), "{first}");
    assert!(lines[2].contains("z.bin"), "{first}");
    for name in ["a.bin", "m.bin", "z.bin"] {
        fs::remove_file(fixture.0.join(name)).expect("remove");
    }
    for name in ["m.bin", "z.bin", "a.bin"] {
        fs::write(fixture.0.join(name), b"x").expect("reordered plan");
    }
    assert_eq!(first, template::execute(&list).expect("reordered list"));
}

#[test]
fn template_path_options_refuse_empty_flag_values_and_duplicate_directories() {
    let fixture = Fixture::new("template-path-options");
    let source = fixture.0.join("source");
    fs::write(&source, TEMPLATE).expect("source");
    // Invalid source avoids any accidental writes on the unfixed implementation.
    for value in ["", "--unknown", "--dir", "-o"] {
        let build = args(&["template", "build", "missing-template", "--output", value]);
        assert_eq!(
            template::execute(&build).expect_err("output usage").code,
            2,
            "{value:?}"
        );
        let list = args(&["template", "list", "--dir", value]);
        assert_eq!(
            template::execute(&list).expect_err("directory usage").code,
            2,
            "{value:?}"
        );
    }
    let list = args(&[
        "template",
        "list",
        "--dir",
        fixture.0.to_str().unwrap(),
        "--dir",
        fixture.0.to_str().unwrap(),
    ]);
    assert_eq!(
        template::execute(&list)
            .expect_err("duplicate directory")
            .code,
        2
    );
}

#[test]
fn cache_root_selectors_must_not_silently_override_each_other() {
    let fixture = Fixture::new("cache-roots");
    for action in ["status", "gc", "purge"] {
        for second in ["first", "second"] {
            let input = args(&["cache", action, "--root", "first", "--root", second]);
            let failure = cache::output_in(&input, Some(&fixture.0)).expect_err("duplicate scope");
            assert_eq!(failure.code, 2, "{action}: {failure:?}");
            assert!(failure.message.contains("--root"));
            actionable(&failure);
        }
        let input = args(&["cache", action, "--root", ""]);
        assert_eq!(
            cache::output_in(&input, Some(&fixture.0))
                .expect_err("empty scope")
                .code,
            2
        );
    }
}

#[test]
fn cache_numeric_bounds_duplicates_reordering_and_repeat_reports() {
    let fixture = Fixture::new("cache-bounds");
    for value in ["", "x", "0", "-1", "18446744073709551616", "1.5", "é"] {
        for flag in ["--max-bytes", "--ttl-days"] {
            let input = args(&["cache", "config", flag, value]);
            let failure = cache::output_in(&input, Some(&fixture.0)).expect_err("invalid number");
            assert_eq!(failure.code, 2, "{flag}={value}: {failure:?}");
            actionable(&failure);
        }
    }
    for words in [
        vec!["cache", "config", "--json", "--json"],
        vec!["cache", "config", "auto", "off", "auto", "off"],
        vec!["cache", "config", "--ttl-days", "1", "--ttl-days", "1"],
        vec!["cache", "config", "--max-bytes", "1", "--max-bytes", "1"],
    ] {
        assert_eq!(
            cache::output_in(&args(&words), Some(&fixture.0))
                .expect_err("duplicate")
                .code,
            2
        );
    }
    let left = args(&[
        "cache",
        "config",
        "auto",
        "off",
        "--ttl-days",
        "1",
        "--json",
    ]);
    let right = args(&[
        "cache",
        "config",
        "--json",
        "--ttl-days",
        "1",
        "auto",
        "off",
    ]);
    let report = cache::output_in(&left, Some(&fixture.0)).expect("configure");
    let policy = fs::read(fixture.0.join("policy.v1")).expect("policy");
    assert_eq!(
        report,
        cache::output_in(&right, Some(&fixture.0)).expect("reorder")
    );
    assert_eq!(
        policy,
        fs::read(fixture.0.join("policy.v1")).expect("repeat bytes")
    );
    let max = args(&["cache", "config", "--ttl-days", "18446744073709551615"]);
    assert!(cache::output_in(&max, Some(&fixture.0)).is_ok());
}

#[test]
fn cache_policy_all_prefixes_invalid_utf8_duplicates_and_reordering() {
    let fixture = Fixture::new("policy-bytes");
    let policy = b"enabled=0\nmin_interval_ms=1\noverlay_ttl_ms=2\npreview_ttl_ms=3\nroot_ttl_ms=4\nstaging_ttl_ms=5\nsystem_cap_bytes=6\nlast_reclaimed_bytes=7\n";
    host::ensure_cache_dir(&fixture.0).expect("cache root");
    let path = fixture.0.join("policy.v1");
    for end in 0..policy.len() - 1 {
        fs::write(&path, &policy[..end]).expect("prefix");
        assert!(host::load_policy(&fixture.0).is_err(), "offset {end}");
    }
    fs::write(&path, policy).expect("policy");
    let first = host::load_policy(&fixture.0).expect("valid policy");
    let reversed = std::str::from_utf8(policy)
        .unwrap()
        .lines()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, reversed).expect("reordered policy");
    assert_eq!(
        first,
        host::load_policy(&fixture.0).expect("order independent")
    );
    for bad in [
        vec![0xff],
        vec![b'x'],
        [policy.as_slice(), b"enabled=0\n"].concat(),
        policy.repeat(2),
    ] {
        fs::write(&path, bad).expect("bad policy");
        assert!(host::load_policy(&fixture.0).is_err());
    }
    fs::write(&path, policy).expect("valid policy");
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let fixture = &fixture;
            scope.spawn(move || {
                assert_eq!(
                    first,
                    host::load_policy(&fixture.0).expect("parallel policy")
                )
            });
        }
    });
}

#[test]
fn additional_roots_exact_limit_over_limit_truncation_and_opaque_values() {
    for count in [0, 1, 16, 17] {
        let mut input = args(&["pack", "root"]);
        for index in 0..count {
            input.extend(["--also".into(), format!("root-{index}")]);
        }
        let parsed = host::also_arguments(&input);
        if count <= 16 {
            let (words, roots) = parsed.expect("bounded roots");
            assert_eq!(words, args(&["pack", "root"]));
            assert_eq!(roots.len(), count);
        } else {
            assert!(parsed.is_err());
        }
    }
    for input in [args(&["--also"]), args(&["--also", ""])] {
        assert!(host::also_arguments(&input).is_err());
    }
    let input = args(&["--for", "--also", "--", "--also", "literal"]);
    assert_eq!(
        host::also_arguments(&input).expect("opaque"),
        (input, vec![])
    );
    let repeated = args(&["--also", "a", "--also", "a"]);
    assert_eq!(
        host::also_arguments(&repeated)
            .expect("preserve duplicates")
            .1,
        vec![PathBuf::from("a"), PathBuf::from("a")]
    );
}

#[test]
fn global_auto_clean_flag_preserves_literal_targets_and_option_values() {
    for input in [
        args(&["dctx", "map", "grep", "--", "--no-auto-clean"]),
        args(&["dctx", "context", "x", "--session", "--no-auto-clean"]),
        args(&["dctx", "plugin", "run", "dctx/git", "--", "--no-auto-clean"]),
        args(&["dctx", "pack", "--output", "--no-auto-clean"]),
    ] {
        assert!(host::auto_clean_enabled(&input, None));
        assert_eq!(host::without_auto_flag(input.clone()), input);
    }
    let input = args(&["dctx", "--no-auto-clean", "pack", "--no-auto-clean", "x"]);
    assert!(!host::auto_clean_enabled(&input, None));
    let filtered = host::without_auto_flag(input);
    assert_eq!(filtered, args(&["dctx", "pack", "x"]));
    assert_eq!(host::without_auto_flag(filtered.clone()), filtered);
    assert!(!host::auto_clean_enabled(&[], Some("0")));
    for value in [None, Some(""), Some("1"), Some("x")] {
        assert!(host::auto_clean_enabled(&[], value));
    }
}

#[test]
fn language_renderers_readme_markers_and_menu_values_are_repeatable() {
    let sample = format!("{}é🦀\n{}", languages::README_BEGIN, languages::README_END);
    for end in 0..=sample.len() {
        if let Some(prefix) = sample.get(..end) {
            let block = languages::readme_block(prefix);
            assert_eq!(block, languages::readme_block(prefix));
            assert_eq!(block.is_some(), end == sample.len());
        }
    }
    for text in [
        String::new(),
        "x".into(),
        sample.repeat(2),
        format!("{}{}", languages::README_END, languages::README_BEGIN),
    ] {
        assert!(languages::readme_block(&text).is_none());
    }
    for flags in [
        vec![],
        args(&["--json"]),
        args(&["--markdown"]),
        args(&["--help"]),
    ] {
        let page = languages::page(&flags).expect("page");
        assert_eq!(page, languages::page(&flags).expect("repeat"));
        assert_eq!(page, languages::output(&flags).expect("captured").stdout);
    }
    assert!(languages::page(&args(&["--json", "--json"])).is_err());
    for flags in [["--json", "--markdown"], ["--markdown", "--json"]] {
        assert!(languages::page(&args(&flags)).is_err());
    }
    for text in [
        String::new(),
        "x".into(),
        "é\0\u{1b}".into(),
        "x".repeat(65_536),
    ] {
        let context = tui::MenuContext::new(text.clone());
        assert_eq!(context.format(), format!("Directory: {text}"));
        let item = tui::MenuItem::new('é', &text, &text);
        assert_eq!(item.label, text);
        let mut menu = tui::Menu::new(&text);
        menu.add_item('x', &text, &text)
            .add_item('x', &text, &text)
            .set_footer(&text);
        assert_eq!(menu.items.len(), 2);
        assert_eq!(menu.title, text);
        assert_eq!(menu.footer.as_deref(), Some(text.as_str()));
        assert!(!help::create_main_menu(&context).items.is_empty());
        assert!(!tui::create_template_menu().items.is_empty());
        assert!(!tui::create_cache_menu().items.is_empty());
    }
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                assert_eq!(languages::text(), languages::text());
                assert_eq!(languages::json(), languages::json());
                assert_eq!(languages::markdown(), languages::markdown());
                assert_eq!(help::text(), help::text());
                assert_eq!(help::limits(), help::limits());
                assert!(!help::pack_usage().is_empty());
                assert_eq!(
                    dctx_cli::rpc::schema().unwrap(),
                    dctx_cli::rpc::schema().unwrap()
                );
            });
        }
    });
}

struct ShortWriter {
    bytes: Vec<u8>,
    remaining: usize,
    fail_flush: bool,
}
impl Write for ShortWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::from(io::ErrorKind::StorageFull));
        }
        let count = bytes.len().min(self.remaining).min(1);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.fail_flush {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            Ok(())
        }
    }
}

#[test]
fn output_transport_handles_every_partial_write_and_flush_failure() {
    let sample = "x\0é🦀\n";
    for end in 0..=sample.len() {
        let mut writer = ShortWriter {
            bytes: vec![],
            remaining: end,
            fail_flush: false,
        };
        let result = exit::CommandOutput {
            stdout: sample.into(),
            failure: None,
        }
        .write_to(&mut writer);
        assert_eq!(writer.bytes, sample.as_bytes()[..end]);
        if end < sample.len() {
            actionable(&result.expect_err("partial failure"));
        } else {
            result.expect("complete");
        }
    }
    for text in [String::new(), "x".into(), "x".repeat(65_536)] {
        let mut bytes = Vec::new();
        let failure = exit::Failure::no_result("nothing", "choose a target");
        let result = exit::CommandOutput {
            stdout: text.clone(),
            failure: Some(failure.clone()),
        }
        .write_to(&mut bytes);
        assert_eq!(result, Err(failure));
        assert_eq!(bytes, text.as_bytes());
    }
    let mut writer = ShortWriter {
        bytes: vec![],
        remaining: 1,
        fail_flush: true,
    };
    actionable(
        &exit::CommandOutput {
            stdout: "x".into(),
            failure: None,
        }
        .write_to(&mut writer)
        .expect_err("flush"),
    );
    assert_eq!(
        exit::write_failure(io::Error::from(io::ErrorKind::BrokenPipe)).code,
        0
    );
    let mut writer = ShortWriter {
        bytes: vec![],
        remaining: 0,
        fail_flush: true,
    };
    let mut tracked = exit::TrackedWriter::new(&mut writer);
    assert!(tracked.write_all(b"x").is_err());
    assert!(tracked.flush().is_err());
    assert_eq!(
        tracked.take_error().expect("first error").kind(),
        io::ErrorKind::StorageFull
    );
    assert!(tracked.take_error().is_none());
}

#[test]
fn failure_constructors_and_facade_mapping_have_stable_actionable_output() {
    for text in [
        String::new(),
        "x".into(),
        "\0é\n".into(),
        "x".repeat(65_536),
    ] {
        for (code, failure) in [
            (1, exit::Failure::no_result(&text, "choose a target")),
            (2, exit::Failure::usage(&text, "run help")),
            (3, exit::Failure::index(&text, "check the path")),
        ] {
            assert_eq!(failure.code, code);
            assert_eq!(failure, exit::Failure::new(code, &text, &failure.hint));
            assert_eq!(failure.render(), failure.clone().render());
            assert_eq!(failure.within("test").message, format!("test: {text}"));
        }
    }
    for error in [
        dctx::DctxError::SourceUnavailable,
        dctx::DctxError::PolicyDenied,
        dctx::DctxError::StaleContinuation,
        dctx::DctxError::io("x"),
    ] {
        actionable(&exit::from_facade(&error, "dctx --help"));
    }
}

#[test]
fn pack_all_source_prefixes_invalid_utf8_filters_and_output_parity() {
    let fixture = Fixture::new("pack-content");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    let input = args(&["pack", project.to_str().unwrap(), "--tree-stamps", "off"]);
    let empty = pack::output(&input).expect("empty page");
    assert_eq!(empty.failure.expect("no result").code, 1);
    let file = project.join("sample.rs");
    let sample = "fn sample() { /* é🦀 */ }\n".as_bytes();
    for end in 0..=sample.len() {
        fs::write(&file, &sample[..end]).expect("source prefix");
        let first = pack::output(&input).expect("page");
        let second = pack::output(&input).expect("repeat");
        assert_eq!(first.stdout, second.stdout, "byte offset {end}");
        assert_eq!(first.failure, second.failure);
        let mut stream = Vec::new();
        let result = pack::write_output(&input, &mut stream);
        assert_eq!(result.err(), first.failure);
        assert_eq!(stream, first.stdout.as_bytes());
    }
    fs::write(&file, b"fn sample() {}\n").expect("valid source");
    let left = args(&[
        "pack",
        project.to_str().unwrap(),
        "--tree-stamps",
        "off",
        "--include",
        "*.rs",
        "--exclude",
        "*.md",
    ]);
    let right = args(&[
        "pack",
        "--exclude",
        "*.md",
        "--include",
        "*.rs",
        "--include",
        "*.rs",
        "--tree-stamps=off",
        project.to_str().unwrap(),
    ]);
    let page = pack::output(&left).expect("filtered").stdout;
    assert_eq!(
        page,
        pack::output(&right)
            .expect("reordered and duplicate pattern")
            .stdout
    );
    let outcome = pack::execute(&left).expect("captured outcome");
    assert_eq!(outcome.files, 1);
    let mut normalized = outcome.body;
    if !normalized.ends_with(b"\n") {
        normalized.push(b'\n');
    }
    assert_eq!(normalized, page.as_bytes());
    let output = fixture.0.join("page.txt");
    let mut to_file = left.clone();
    to_file.extend(["--output".into(), output.display().to_string()]);
    for _ in 0..2 {
        let mut stdout = Vec::new();
        pack::write_output(&to_file, &mut stdout).expect("explicit output");
        assert!(stdout.is_empty());
        assert_eq!(fs::read(&output).expect("saved bytes"), page.as_bytes());
    }
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| assert_eq!(pack::output(&left).expect("concurrent pack").stdout, page));
        }
    });
}

#[test]
fn pack_rejects_nonfinite_sizes_and_empty_or_conflicting_options() {
    let fixture = Fixture::new("pack-options");
    for flag in ["--large", "--small"] {
        for value in ["", "x", "NaN", "inf", "-inf", "1e309", "-1"] {
            let input = args(&["pack", fixture.0.to_str().unwrap(), flag, value]);
            let failure = pack::output(&input).expect_err("bad size");
            assert_eq!(failure.code, 2);
            actionable(&failure);
        }
        for value in ["0", "1", "1.7976931348623157e308"] {
            assert!(
                pack::output(&args(&["pack", fixture.0.to_str().unwrap(), flag, value])).is_ok()
            );
        }
    }
    for suffix in [
        vec!["--ext", ""],
        vec!["--include", ""],
        vec!["--exclude", "!x"],
        vec!["--empty", "--non-empty"],
        vec!["--output", ""],
        vec!["--output", "a", "--output", "b"],
    ] {
        let mut input = args(&["pack", fixture.0.to_str().unwrap()]);
        input.extend(args(&suffix));
        assert_eq!(pack::output(&input).expect_err("invalid filter").code, 2);
    }
}

#[test]
fn source_file_limit_and_total_scan_limits_have_exact_boundaries() {
    let fixture = Fixture::new("source-limits");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    let file = project.join("large.txt");
    const CAP: usize = 8 * 1024 * 1024;
    fs::write(&file, vec![b'x'; CAP]).expect("maximum file");
    assert_eq!(
        host::read_included_file(&project, "large.txt")
            .expect("read")
            .expect("included")
            .len(),
        CAP
    );
    let input = args(&["pack", project.to_str().unwrap(), "--tree-stamps", "off"]);
    assert!(pack::output(&input).is_ok());
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .unwrap()
        .write_all(b"x")
        .unwrap();
    assert!(host::read_included_file(&project, "large.txt").is_err());
    let failure = pack::output(&input).expect_err("file too large");
    assert_eq!(failure.code, 3);
    actionable(&failure);
    fs::write(&file, b"12345678").expect("small source");
    for (limit, success) in [("8", true), ("7", false)] {
        let output = fixture
            .command()
            .args(["pack", "project", "--tree-stamps", "off"])
            .env("DCTX_MAX_SOURCE_BYTES", limit)
            .output()
            .expect("source limit");
        assert_eq!(
            output.status.success(),
            success,
            "limit {limit}: {output:?}"
        );
        if !success {
            assert!(String::from_utf8_lossy(&output.stderr).contains("DCTX_MAX_SOURCE_BYTES"));
        }
    }
}

#[test]
fn host_source_entry_points_refuse_missing_empty_and_non_directory_roots() {
    let fixture = Fixture::new("host-roots");
    let file = fixture.0.join("file");
    fs::write(&file, b"x").expect("file");
    for path in [PathBuf::new(), fixture.0.join("missing"), file] {
        assert!(host::open_dir(&path).is_err());
        assert!(host::open_mapped(&path).is_err());
        assert!(host::open_mapped_at(&path, None).is_err());
        assert!(host::open_mapped_query(&path, Some("x")).is_err());
        assert!(host::open_mapped_roots(&path, &[]).is_err());
        assert!(host::open_notice_query(&path, &[], Some("x"), &host::Imports::default()).is_err());
    }
    for relative in ["../escape", "a/../../escape", "a\0b"] {
        assert!(host::read_included_file(&fixture.0, relative).is_err());
    }
    for relative in [".env", ".git/config"] {
        assert_eq!(
            host::read_included_file(&fixture.0, relative).expect("denied"),
            None
        );
    }
    let page = "unchanged\0é";
    assert_eq!(
        host::outside_notice(
            &fixture.0.join("missing"),
            &[],
            "x",
            page.into(),
            false,
            &[]
        ),
        page
    );
}

#[test]
fn host_sessions_imports_and_marker_projection_are_reentrant() {
    let fixture = Fixture::new("host-sessions");
    fs::write(fixture.0.join("sample.rs"), b"pub fn sample() {}\n").expect("source");
    let imports = host::Imports::default();
    let (mut session, _) =
        host::open_notice_query(&fixture.0, &[], Some("sample.rs"), &imports).expect("session");
    let handle = session
        .snapshot(&dctx::SnapshotRequest::strict())
        .expect("snapshot");
    for name in ["", "x", "sample", "é\0"] {
        assert_eq!(
            host::indexed_name(&mut session, &handle, name, None),
            name == "sample"
        );
        assert!(imports
            .unresolved(&mut session, &handle, Some(name), None)
            .is_empty());
        assert!(imports.candidates(Some(name)).is_empty());
    }
    assert!(imports.candidates(None).is_empty());
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                assert!(imports.clone().candidates(None).is_empty());
                assert!(host::open_memory().is_ok());
                assert!(host::open_dir(&fixture.0).is_ok());
                assert!(host::open_mapped_at(&fixture.0, None).is_ok());
                assert!(host::open_mapped_query(&fixture.0, Some("sample.rs")).is_ok());
            });
        }
    });
    for content in [vec![], vec![b'x'], vec![0xff], vec![b'x'; 65_536]] {
        let mut node = dctx_core::datatypes::DirContextTreeNode::new(
            dctx_core::datatypes::NodeType::File,
            "sample.rs",
            PathBuf::new(),
        );
        node.file_content = Some(content.clone());
        node.content_size = content.len() as u64;
        let policy = dctx_core::selection::SelectionPolicy::default();
        assert_eq!(pack::legacy_markers(&node, &policy).content, Some(content));
        node.is_ignored = true;
        assert!(pack::legacy_markers(&node, &policy).content.is_none());
    }
}

#[test]
fn policy_file_size_boundary_and_ledger_truncation_preserve_stored_bytes() {
    let fixture = Fixture::new("policy-ledger");
    let root = fixture.0.join("cache");
    host::ensure_cache_dir(&root).expect("cache");
    host::configure(&root, Some(false), None, None).expect("policy");
    let path = root.join("policy.v1");
    let mut bytes = fs::read(&path).expect("policy bytes");
    let policy = host::load_policy(&root).expect("policy");
    bytes.resize(RPC_CAP, b'\n');
    fs::write(&path, &bytes).expect("max policy");
    assert_eq!(host::load_policy(&root).expect("at cap"), policy);
    bytes.push(b'\n');
    fs::write(&path, &bytes).expect("over cap");
    assert!(host::load_policy(&root).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::remove_file(&path).expect("remove policy");
    let life = dctx_adapter_cache::Lifecycle::product_default();
    host::store_ledger(&root, &life).expect("initial ledger");
    let ledger = root.join("ledger.v1");
    let encoded = fs::read(&ledger).expect("encoded ledger");
    for end in 0..encoded.len() {
        fs::write(&ledger, &encoded[..end]).expect("prefix");
        assert!(host::load_ledger(&root).is_err(), "offset {end}");
        assert_eq!(fs::read(&ledger).unwrap(), encoded[..end]);
    }
    fs::write(&ledger, &encoded).expect("restore ledger");
    assert!(host::load_ledger(&root).is_ok());
    assert!(host::store_ledger(&root, &life).is_err());
    assert_eq!(fs::read(&ledger).unwrap(), encoded);
}

#[test]
fn cache_configuration_rejects_invalid_public_api_values_before_writing() {
    let fixture = Fixture::new("configure-invalid");
    for (cap, days) in [(Some(0), None), (None, Some(0)), (Some(u64::MAX), None)] {
        let root = fixture.0.join(format!("case-{cap:?}-{days:?}"));
        assert!(
            host::configure(&root, Some(false), cap, days).is_err(),
            "cap={cap:?} days={days:?}"
        );
        assert!(!root.join("policy.v1").exists());
    }
}

#[test]
fn context_target_count_byte_bounds_and_option_reordering() {
    let fixture = Fixture::new("context-limits");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    fs::write(project.join("sample.rs"), b"pub fn sample() {}\n").expect("source");
    for count in [1, 64, 65] {
        let mut input = vec!["context", "--json", "--place", "project"];
        input.extend(std::iter::repeat_n("sample", count));
        let output = fixture.cli(&input);
        assert_eq!(
            output.status.code(),
            Some(if count <= 64 { 0 } else { 2 }),
            "count {count}: {output:?}"
        );
        same_process_result(&output, &fixture.cli(&input));
    }
    for size in [0, 1, 1024, 1025] {
        let target = "x".repeat(size);
        let output = fixture.cli(&["context", &target, "--place", "project", "--json"]);
        assert_eq!(
            output.status.code(),
            Some(if size == 0 || size > 1024 { 2 } else { 1 }),
            "size {size}: {output:?}"
        );
    }
    let first = fixture.cli(&[
        "context", "sample", "--json", "--budget", "100000", "--place", "project",
    ]);
    assert!(first.status.success(), "{first:?}");
    same_process_result(
        &first,
        &fixture.cli(&[
            "context", "--place", "project", "--budget", "100000", "--json", "sample",
        ]),
    );
    for budget in ["0", "99", "100001", "4294967296", "x", ""] {
        assert_eq!(
            fixture
                .cli(&["context", "sample", "--place", "project", "--budget", budget])
                .status
                .code(),
            Some(2)
        );
    }
}

#[test]
fn map_numeric_bounds_and_cursor_prefixes_are_usage_errors_without_panics() {
    let fixture = Fixture::new("map-limits");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    fs::write(project.join("sample.rs"), b"pub fn sample() {}\n").expect("source");
    for (verb, target, flag, maximum) in [
        ("map", "m:sample.rs", "--depth", 3),
        ("neighbors", "s:sample.rs#sample", "--depth", 4),
        ("find", "sample", "--limit", 40),
        ("explain", "s:sample.rs#sample", "--budget", 1000),
        ("read", "m:sample.rs", "--budget", 2000),
    ] {
        let maximum_text = maximum.to_string();
        let output = fixture.cli(&[
            "map",
            verb,
            target,
            "--place",
            "project",
            flag,
            &maximum_text,
            "--json",
        ]);
        assert!(
            output.status.success(),
            "{verb} {flag}={maximum}: {output:?}"
        );
        same_process_result(
            &output,
            &fixture.cli(&[
                "map",
                verb,
                "--json",
                flag,
                &maximum_text,
                "--place",
                "project",
                target,
            ]),
        );
        for value in [
            "0".to_owned(),
            (maximum + 1).to_string(),
            "18446744073709551616".into(),
            "x".into(),
            String::new(),
        ] {
            let output = fixture.cli(&["map", verb, target, "--place", "project", flag, &value]);
            assert_eq!(
                output.status.code(),
                Some(2),
                "{verb} {flag}={value}: {output:?}"
            );
            assert!(String::from_utf8_lossy(&output.stderr).contains("hint:"));
        }
    }
    let cursor = "v1|1|find|s:sample.rs#sample";
    for end in 0..=cursor.len() {
        let output = fixture.cli(&[
            "map",
            "find",
            "sample",
            "--place",
            "project",
            "--cursor",
            &cursor[..end],
        ]);
        assert_eq!(
            output.status.code(),
            Some(if end == cursor.len() { 0 } else { 2 }),
            "cursor offset {end}: {output:?}"
        );
        if end < cursor.len() {
            assert!(String::from_utf8_lossy(&output.stderr).contains("hint:"));
        }
    }
}

#[cfg(unix)]
#[test]
fn symlink_leaves_are_refused_by_reads_and_outputs_without_changing_targets() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("symlink-leaves");
    let source = fixture.0.join("source");
    let target = fixture.0.join("target");
    let link = fixture.0.join("link");
    fs::write(&source, TEMPLATE).expect("source");
    fs::write(&target, b"sentinel").expect("target");
    symlink(&target, &link).expect("link");
    assert!(host::read_explicit_file(&link, 1024).is_err());
    let build = args(&[
        "template",
        "build",
        source.to_str().unwrap(),
        "--output",
        link.to_str().unwrap(),
    ]);
    actionable(&template::execute(&build).expect_err("symlink output"));
    assert_eq!(fs::read(&target).unwrap(), b"sentinel");
    assert!(host::ensure_cache_dir(&link).is_err());
    assert!(host::load_policy(&link).is_err());
}

#[test]
fn public_string_errors_and_corrupt_policy_diagnostics_preserve_next_steps() {
    let fixture = Fixture::new("error-hints");
    assert!(cache::run_in(&[], Some(&fixture.0))
        .expect_err("cache usage")
        .contains("dctx cache --help"));
    assert!(pack::execute(&[])
        .expect_err("pack usage")
        .contains("dctx pack --help"));
    fs::write(fixture.0.join("policy.v1"), b"bad policy").expect("corrupt policy");
    let failure = cache::output_in(&args(&["cache", "config"]), Some(&fixture.0))
        .expect_err("policy failure");
    assert!(failure.hint.contains("remove policy.v1"));
    assert_eq!(
        fs::read(fixture.0.join("policy.v1")).unwrap(),
        b"bad policy"
    );
    let source = fixture.0.join("oversized");
    fs::File::create(&source)
        .unwrap()
        .set_len(RPC_CAP as u64 + 1)
        .unwrap();
    let failure = template::execute(&args(&["template", "build", source.to_str().unwrap()]))
        .expect_err("source cap");
    assert!(failure.hint.contains("1 MiB"));
    assert!(failure.hint.contains("reduce"));
}

#[cfg(unix)]
#[test]
fn doctor_deleted_working_directory_names_a_recovery_step() {
    let fixture = Fixture::new("doctor-deleted-cwd");
    let gone = fixture.0.join("gone");
    fs::create_dir(&gone).expect("working directory");
    let output = Command::new("sh")
        .env_clear()
        .current_dir(&gone)
        .args([
            "-c",
            "rmdir ../gone && exec \"$1\" doctor --json",
            "doctor-fixture",
            env!("CARGO_BIN_EXE_dctx-cli"),
        ])
        .output()
        .expect("deleted directory doctor");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("current directory"), "{stderr}");
    assert!(
        stderr.contains("hint: change to an existing directory"),
        "{stderr}"
    );
}

#[test]
fn auto_clean_literal_flag_survives_real_cli_dispatch() {
    let fixture = Fixture::new("literal-auto-flag");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    fs::write(
        project.join("sample.rs"),
        b"// --no-auto-clean\npub fn sample() {}\n",
    )
    .expect("source");
    let output = fixture.cli(&["map", "grep", "--place", "project", "--", "--no-auto-clean"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("--no-auto-clean"));
    same_process_result(
        &output,
        &fixture.cli(&["map", "grep", "--place", "project", "--", "--no-auto-clean"]),
    );
}

#[test]
fn over_limit_pack_preserves_output_and_view_exclusions_still_work() {
    let fixture = Fixture::new("over-limit-pack-output");
    let project = fixture.0.join("project");
    fs::create_dir(&project).expect("project");
    fs::File::create(project.join("large.txt"))
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    fs::write(project.join("sample.rs"), b"pub fn sample() {}\n").expect("source");
    let output = fixture.0.join("page.txt");
    fs::write(&output, b"keep previous page").expect("previous output");
    let input = args(&[
        "pack",
        project.to_str().unwrap(),
        "--tree-stamps",
        "off",
        "--output",
        output.to_str().unwrap(),
    ]);
    let mut stream = Vec::new();
    let failure = pack::write_output(&input, &mut stream).expect_err("oversized source");
    assert_eq!(failure.code, 3);
    assert!(failure.hint.contains("--exclude"));
    assert!(stream.is_empty());
    assert_eq!(fs::read(&output).unwrap(), b"keep previous page");
    assert!(pack::execute(&input)
        .expect_err("captured source cap")
        .contains("--exclude"));
    let mut filtered = input;
    filtered.extend(args(&["--exclude", "large.txt"]));
    pack::write_output(&filtered, &mut stream).expect("excluded oversized source");
    assert!(String::from_utf8(fs::read(&output).unwrap())
        .unwrap()
        .contains("pub fn sample"));
}

#[test]
fn disabled_maintenance_and_cache_session_operations_are_repeatable() {
    let fixture = Fixture::new("cache-maintenance");
    let missing = fixture.0.join("missing");
    assert_eq!(host::piggyback_at(&missing, false).expect("disabled"), 0);
    assert!(!missing.exists());
    host::configure(&fixture.0, Some(false), None, None).expect("disable");
    let before = fs::read(fixture.0.join("policy.v1")).unwrap();
    assert_eq!(
        host::piggyback_at(&fixture.0, true).expect("disabled policy"),
        0
    );
    assert_eq!(fs::read(fixture.0.join("policy.v1")).unwrap(), before);
    for call in [
        host::SessionCacheCall::Status,
        host::SessionCacheCall::Gc { apply: false },
        host::SessionCacheCall::Gc { apply: true },
        host::SessionCacheCall::Purge { apply: false },
        host::SessionCacheCall::Purge { apply: true },
    ] {
        assert!(host::session_cache(call).is_ok());
    }
    let mut life = dctx_adapter_cache::Lifecycle::product_default();
    life.auto_clean(
        dctx_adapter_cache::ClockSample {
            wall: dctx_core::ids::Timestamp(123),
            monotonic_ms: 123,
            boot_id: 1,
        },
        dctx_adapter_cache::AutoCleanPolicy::product_default(),
    )
    .expect("fixed maintenance clock");
    let root = fixture.0.join("recorded");
    host::ensure_cache_dir(&root).unwrap();
    host::store_ledger(&root, &life).unwrap();
    let report = host::auto_report(&root).expect("recorded report");
    assert_eq!(report.last_auto_clean_ms, Some(123));
    assert_eq!(report, host::auto_report(&root).expect("repeat report"));
}

#[test]
fn template_directory_entry_limit_includes_non_plan_entries() {
    let fixture = Fixture::new("template-entry-limit");
    let input = args(&["template", "list", "--dir", fixture.0.to_str().unwrap()]);
    let empty = template::execute(&input).expect("empty directory");
    assert!(empty.starts_with("no compiled plans"));
    // Empty directories exercise the real entry ceiling without compiling
    // 100,000 identical plans or allocating file bodies.
    for index in 0..100_000 {
        fs::create_dir(fixture.0.join(index.to_string())).expect("directory entry");
    }
    assert_eq!(template::execute(&input).expect("exact limit"), empty);
    fs::create_dir(fixture.0.join("overflow")).expect("overflow entry");
    let failure = template::execute(&input).expect_err("entry limit");
    assert_eq!(failure.code, 3);
    assert!(failure.message.contains("100000"));
    assert!(failure.hint.contains("--dir"));
}

#[test]
fn tui_stdin_entry_points_handle_eof_utf8_prefixes_and_large_lines() {
    // Re-execute only this test with piped stdio: no global stdin replacement,
    // process-global environment mutation, terminal setup or timing assertions.
    if let Ok(method) = std::env::var("DCTX_HOSTILE_TUI_PROBE") {
        assert!(tui::require_terminal().is_err());
        let result = match method.as_str() {
            "input" => tui::get_input("prompt"),
            "menu" => tui::Menu::new("menu").show().map(|key| key.to_string()),
            "message" => tui::show_message("message").map(|()| String::new()),
            "error" => tui::show_error("error").map(|()| String::new()),
            "run" => {
                tui::run();
                Ok(String::new())
            }
            _ => unreachable!("known probe"),
        };
        let value = match result {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error":format!("{:?}", error.kind())}),
        };
        let expected: Value = serde_json::from_str(
            &std::env::var("DCTX_HOSTILE_TUI_EXPECT").expect("expected result"),
        )
        .unwrap();
        assert_eq!(value, expected);
        println!("\nPROBE_RESULT={value}");
        return;
    }
    let fixture = Fixture::new("tui-stdin");
    let sample = " é🦀 \n".as_bytes();
    let mut corpus: Vec<Vec<u8>> = (0..=sample.len())
        .map(|end| sample[..end].to_vec())
        .collect();
    corpus.extend([vec![b'x'], vec![0xff], vec![b'x'; 65_536]]);
    for method in ["input", "menu", "message", "error", "run"] {
        for bytes in &corpus {
            let expected = if method == "run" {
                json!({"ok":""})
            } else if let Ok(text) = std::str::from_utf8(bytes) {
                let text = text.trim();
                let value = match method {
                    "input" => text.to_owned(),
                    "menu" => text.chars().next().unwrap_or('\0').to_string(),
                    _ => String::new(),
                };
                json!({"ok":value})
            } else {
                json!({"error":"InvalidData"})
            };
            let run = || {
                let mut command = Command::new(std::env::current_exe().expect("test executable"));
                command
                    .env_clear()
                    .current_dir(&fixture.0)
                    .args([
                        "--exact",
                        "tui_stdin_entry_points_handle_eof_utf8_prefixes_and_large_lines",
                        "--nocapture",
                    ])
                    .env("DCTX_HOSTILE_TUI_PROBE", method)
                    .env("DCTX_HOSTILE_TUI_EXPECT", expected.to_string())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                common::isolated_dirs(&mut command, &fixture.0.join("state"));
                let mut child = command.spawn().expect("stdin probe");
                let mut input = child.stdin.take().unwrap();
                let output = std::thread::scope(|scope| {
                    let writer = scope.spawn(move || input.write_all(bytes).expect("probe input"));
                    let output = child.wait_with_output().expect("probe exit");
                    writer.join().expect("input writer");
                    output
                });
                assert!(output.status.success(), "{method}: {output:?}");
                let stdout = String::from_utf8(output.stdout).unwrap();
                stdout
                    .lines()
                    .find(|line| line.starts_with("PROBE_RESULT="))
                    .expect("probe result")
                    .to_owned()
            };
            assert_eq!(run(), run());
        }
    }
}
