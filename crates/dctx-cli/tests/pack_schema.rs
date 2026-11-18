// SPDX-License-Identifier: Apache-2.0
//! Published pack contract: schema validation, goldens, exact spans and deterministic bytes.

mod common;

use std::fs::{self, File, FileTimes};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, UNIX_EPOCH};

use serde_json::Value;

struct Fixture {
    temp: tempfile::TempDir,
    repo: PathBuf,
}

impl Fixture {
    fn new(source: &[u8]) -> Self {
        let temp = tempfile::tempdir().expect("fixture");
        let repo = temp.path().join("repo");
        fs::create_dir_all(repo.join("src")).expect("source directory");
        let file = repo.join("src/lib.rs");
        fs::write(&file, source).expect("source");
        // A fixed input timestamp makes the engine's metadata-bearing snapshot golden portable.
        File::options()
            .write(true)
            .open(file)
            .expect("file")
            .set_times(
                FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            )
            .expect("fixed mtime");
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
        let output = self.run(&["pack", "--for", term, "--budget", budget, "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
}

fn validate(bytes: &[u8], valid: bool) {
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut child = Command::new(python)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/validate_pack_schema.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("pack schema tests require {python}: {err}"));
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
    assert_eq!(output.status.success(), valid, "{stderr}");
}

fn verify_spans(bytes: &[u8], repo: &Path) -> Value {
    validate(bytes, true);
    let document: Value = serde_json::from_slice(bytes).expect("JSON");
    for fragment in document["fragments"].as_array().expect("fragments") {
        let source =
            fs::read(repo.join(fragment["path"].as_str().expect("path"))).expect("source bytes");
        let start = fragment["byte_start"].as_u64().expect("start") as usize;
        let end = fragment["byte_end"].as_u64().expect("end") as usize;
        assert!(end > start);
        let span = &source[start..end];
        assert_eq!(
            span,
            fragment["content"].as_str().expect("content").as_bytes()
        );
        assert_eq!(
            fragment["line_start"],
            1 + source[..start].iter().filter(|&&b| b == b'\n').count()
        );
        assert_eq!(
            fragment["line_end"],
            1 + source[..end - 1].iter().filter(|&&b| b == b'\n').count()
        );
        let digest = dctx_hash::Sha256::digest(span).expect("sha256");
        let hex: String = digest
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(fragment["sha256"], hex);
        // The display page may add a final LF; the fragment must retain the exact source.
        assert!(document["text"]
            .as_str()
            .expect("text")
            .contains(fragment["content"].as_str().expect("content")));
    }
    document
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
