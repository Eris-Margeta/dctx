// SPDX-License-Identifier: Apache-2.0
//! Published pack contract: schema validation, goldens, exact spans and deterministic bytes.

mod common;

use std::fs::{self, File, FileTimes};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new(source: &[u8]) -> Self {
        Self::with_files("repo", &[("src/lib.rs", source)])
    }

    fn with_files(name: &str, files: &[(&str, &[u8])]) -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let repo = temp.path().join(name);
        fs::create_dir_all(repo.join("src")).expect("source directory");
        for (path, source) in files {
            let file = repo.join(path);
            fs::write(&file, source).expect("source");
            // A fixed timestamp keeps metadata-bearing snapshots portable.
            File::options()
                .write(true)
                .open(file)
                .expect("file")
                .set_times(
                    FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
                )
                .expect("fixed mtime");
        }
        Self { temp, repo }
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
        command
            .env_clear()
            .current_dir(&self.repo)
            .args(args)
            .env("DCTX_AUTO_CLEAN", "0")
            .env("NO_COLOR", "1");
        common::isolated_dirs(&mut command, self.temp.path());
        command
            .env("DCTX_MAP_CACHE", self.temp.path().join("map-cache"))
            .env("DCTX_PLUGIN_ROOT", self.temp.path().join("plugins"))
            .env("PATH", self.temp.path().join("empty-path"));
        command.output().expect("CLI")
    }

    fn pack(&self, term: &str, budget: &str) -> Vec<u8> {
        self.run_success(&["pack", "--for", term, "--budget", budget, "--json"])
    }

    fn run_success(&self, args: &[&str]) -> Vec<u8> {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "CLI {args:?} exited with {} and stderr {}",
            output.status,
            quoted_output(&output.stderr)
        );
        output.stdout
    }
}

fn quoted_output(bytes: &[u8]) -> String {
    format!("{:?}", String::from_utf8_lossy(bytes))
}

fn validate(bytes: &[u8], valid: bool) {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut child = Command::new(python)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/validate_pack_schema.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("could not start the required {python} validator ({err:?})"));
    let written = child.stdin.take().expect("stdin").write_all(bytes);
    let output = child.wait_with_output().expect("validator");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr
            .lines()
            .any(|line| line == "ModuleNotFoundError: No module named 'jsonschema'"),
        "pack schema tests require the Python module `jsonschema`. From the repo root, run `{python} -m pip install -r scripts/requirements.txt`."
    );
    written.expect("JSON input");
    assert_eq!(
        output.status.success(),
        valid,
        "{}",
        quoted_output(&output.stderr)
    );
}

fn verify_spans(bytes: &[u8], repo: &Path) -> Value {
    validate(bytes, true);
    let document: Value = serde_json::from_slice(bytes).expect("JSON");
    for fragment in document["fragments"].as_array().expect("fragments") {
        let path = fragment["path"].as_str().expect("path");
        let source = fs::read(repo.join(path))
            .unwrap_or_else(|err| panic!("reading fragment source {path:?} failed with {err:?}"));
        let start = usize::try_from(fragment["byte_start"].as_u64().expect("start"))
            .expect("start fits usize");
        let end =
            usize::try_from(fragment["byte_end"].as_u64().expect("end")).expect("end fits usize");
        assert!(
            start < end && end <= source.len(),
            "invalid byte range [{start}, {end}) for {path:?} ({} bytes)",
            source.len()
        );
        let span = &source[start..end];
        let content = fragment["content"].as_str().expect("content");
        assert_eq!(span, content.as_bytes(), "source bytes for {path:?}");
        assert_eq!(
            fragment["line_start"],
            1 + source[..start].iter().filter(|&&b| b == b'\n').count(),
            "first line for {path:?}"
        );
        assert_eq!(
            fragment["line_end"],
            1 + source[..end - 1].iter().filter(|&&b| b == b'\n').count(),
            "last line for {path:?}"
        );
        let digest = dctx_hash::Sha256::digest(span).expect("sha256");
        let hex: String = digest
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(fragment["sha256"], hex, "SHA-256 for {path:?}");
        // The display page may add a final LF; the fragment must retain the exact source.
        assert!(
            document["text"].as_str().expect("text").contains(content),
            "display text is missing the fragment for {path:?}"
        );
    }
    document
}

#[test]
fn subprocess_diagnostics_quote_control_characters() {
    assert_eq!(
        quoted_output(b"first\n\x1b[31m\"second\""),
        r#""first\n\u{1b}[31m\"second\"""#
    );
}

#[test]
fn printed_schema_matches_both_shipped_copies_without_project_io() {
    let fixture = Fixture::new(b"");
    let output = fixture.run(&["pack", "--schema"]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        include_bytes!("../../../schemas/dctx.pack-1.schema.json")
    );
    assert_eq!(output.stdout, dctx_cli::pack::JSON_SCHEMA.as_bytes());
    assert!(!fixture.temp.path().join("map-cache").exists());
    assert!(!fixture.repo.join(".dctx").exists());
}

#[test]
fn golden_success_and_empty_pack_validate_and_repeat_identically() {
    let fixture = Fixture::new(b"// fixture\npub fn ping() {}\n");
    let first = fixture.pack("ping", "3000");
    assert_eq!(first, fixture.pack("ping", "3000"));
    verify_spans(&first, &fixture.repo);
    assert_eq!(first, include_bytes!("golden/pack_v1.json"));
    let empty = fixture.run(&["pack", "--for", "missing_symbol", "--json"]);
    assert_eq!(empty.status.code(), Some(1));
    validate(&empty.stdout, true);
    assert_eq!(empty.stdout, include_bytes!("golden/pack_v1_empty.json"));
    let again = fixture.run(&["pack", "--for", "missing_symbol", "--json"]);
    assert_eq!(empty.stdout, again.stdout);
}

#[test]
fn spans_preserve_unicode_crlf_and_a_missing_final_newline() {
    for source in [
        "// café 🦀\r\npub fn ping() {\r\n    let _s = \"ž\";\r\n}\r\n",
        "// café 🦀\npub fn ping() {\n    let _s = \"ž\";\n}",
    ] {
        let fixture = Fixture::new(source.as_bytes());
        let bytes = fixture.pack("ping", "3000");
        assert_eq!(bytes, fixture.pack("ping", "3000"));
        let doc = verify_spans(&bytes, &fixture.repo);
        let fragments = doc["fragments"].as_array().expect("fragments");
        assert_eq!(fragments.len(), 1);
        assert!(fragments[0]["byte_start"].as_u64().expect("start") > 0);
        assert_eq!(doc["commit"], Value::Null);
    }
}

#[test]
fn budget_truncation_keeps_exact_prefix_spans_and_no_display_markers() {
    let mut source = String::from("// prefix\npub fn ping() {\n");
    for n in 0..200 {
        source.push_str(&format!("    let _v{n} = {n};\n"));
    }
    source.push_str("}\n");
    let fixture = Fixture::new(source.as_bytes());
    let bytes = fixture.pack("ping", "300");
    let doc = verify_spans(&bytes, &fixture.repo);
    let fragment = &doc["fragments"][0];
    assert!(fragment["byte_end"].as_u64().expect("end") < source.len() as u64);
    let content = fragment["content"].as_str().expect("content");
    assert!(content.ends_with('\n'));
    assert!(!content.contains("DCTX_TRUNCATED") && !content.contains("DCTX_OMITTED"));
    assert_eq!(bytes, fixture.pack("ping", "300"));
}

#[test]
fn cross_file_citations() {
    let fixture = Fixture::with_files(
        "rust",
        &[
            (
                "Cargo.toml",
                include_bytes!("../../dctx-map-parse/tests/fixtures/rust/Cargo.toml"),
            ),
            (
                "src/lib.rs",
                include_bytes!("../../dctx-map-parse/tests/fixtures/rust/src/lib.rs"),
            ),
            (
                "src/support.rs",
                include_bytes!("../../dctx-map-parse/tests/fixtures/rust/src/support.rs"),
            ),
        ],
    );

    let trait_run = "s:src/lib.rs#Work::run";
    let declaration = fixture.run_success(&["map", "read", trait_run, "--json"]);
    let declaration: Value = serde_json::from_slice(&declaration).expect("JSON declaration");
    let records = declaration["records"].as_array().expect("read records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["id"], trait_run);
    assert_eq!(records[0]["path"], "src/lib.rs");
    assert_eq!(records[0]["lines"], json!([19, 19]));
    assert_eq!(records[0]["source_utf8"], "fn run(&self) -> u32;");

    let caller_id = "s:src/lib.rs#Widget::run";
    let full = [
        (caller_id, "src/lib.rs", [23, 25], [438, 483]),
        (
            "s:src/support.rs#helper",
            "src/support.rs",
            [8, 10],
            [235, 267],
        ),
    ];
    let signature = [(caller_id, "src/lib.rs", [23, 23], [438, 461])];

    // Selection budgets reduce source coverage while the hit count stays fixed.
    for (budget, expected) in [
        (3000, full.as_slice()),
        (228, signature.as_slice()),
        (100, &[]),
    ] {
        let bytes = fixture.pack("`run` `helper`", &budget.to_string());
        let doc = verify_spans(&bytes, &fixture.repo);
        assert_eq!(doc["budget"], budget);
        assert_eq!(doc["terms"], 2, "budget {budget}");
        assert_eq!(doc["hits"], 3, "budget {budget}");
        let fragments = doc["fragments"].as_array().expect("fragments");
        assert_eq!(fragments.len(), expected.len(), "budget {budget}");
        for &(target, path, [first, last], [start, end]) in expected {
            let fragment = fragments
                .iter()
                .find(|fragment| fragment["target"] == target)
                .unwrap_or_else(|| panic!("budget {budget} has no fragment for {target:?}"));
            assert_eq!(fragment["path"], path, "budget {budget}");
            assert_eq!(fragment["line_start"], first, "budget {budget}");
            assert_eq!(fragment["line_end"], last, "budget {budget}");
            assert_eq!(fragment["byte_start"], start, "budget {budget}");
            assert_eq!(fragment["byte_end"], end, "budget {budget}");
        }
        if budget == 3000 {
            assert!(
                doc["text"]
                    .as_str()
                    .expect("text")
                    .lines()
                    .any(|line| line.split('\t').next() == Some(trait_run)),
                "the competing trait declaration should appear in the find results"
            );
        }
    }
}

#[test]
fn schema_rejects_missing_fields_bad_hashes_and_unsupported_major() {
    let fixture = Fixture::new(b"pub fn ping() {}\n");
    let original: Value = serde_json::from_slice(&fixture.pack("ping", "3000")).expect("JSON");
    for field in ["schema", "snapshot", "fragments"] {
        let mut invalid = original.clone();
        invalid.as_object_mut().expect("object").remove(field);
        validate(&serde_json::to_vec(&invalid).expect("JSON"), false);
    }
    for (field, value) in [("schema", "dctx.pack/2"), ("schema_version", "2.0")] {
        let mut invalid = original.clone();
        invalid[field] = value.into();
        validate(&serde_json::to_vec(&invalid).expect("JSON"), false);
    }
    let mut invalid = original.clone();
    invalid["fragments"][0]["sha256"] = "bogus".into();
    validate(&serde_json::to_vec(&invalid).expect("JSON"), false);
    // Consumers of this major must tolerate additive metadata.
    let mut extended = original;
    extended["future_metadata"] = true.into();
    validate(&serde_json::to_vec(&extended).expect("JSON"), true);
}

#[test]
fn changed_source_changes_span_hash_and_snapshot_without_including_sensitive_files() {
    let fixture = Fixture::new(b"pub fn ping() { let _x = 1; }\n");
    fs::write(
        fixture.repo.join(".env"),
        "SECRET_PACK_CANARY=do-not-include\n",
    )
    .expect("secret");
    let first_bytes = fixture.pack("ping", "3000");
    let first = verify_spans(&first_bytes, &fixture.repo);
    fs::write(
        fixture.repo.join("src/lib.rs"),
        b"pub fn ping() { let _x = 2; }\n",
    )
    .expect("changed source");
    let second_bytes = fixture.pack("ping", "3000");
    let second = verify_spans(&second_bytes, &fixture.repo);
    assert_ne!(first["snapshot"], second["snapshot"]);
    assert_ne!(
        first["fragments"][0]["sha256"],
        second["fragments"][0]["sha256"]
    );
    assert!(!String::from_utf8_lossy(&first_bytes).contains("SECRET_PACK_CANARY"));
    assert!(!String::from_utf8_lossy(&second_bytes).contains("SECRET_PACK_CANARY"));
}
