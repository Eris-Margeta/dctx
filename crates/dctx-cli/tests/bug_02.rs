// SPDX-License-Identifier: Apache-2.0
//! DCX-BUG-02: template syntax in source bytes and paths is always literal.
#![forbid(unsafe_code)]

mod common;

use std::{
    fs,
    io::Write as _,
    path::Path,
    process::{Command, Output, Stdio},
};

use dctx_core::{
    datatypes::NodeType,
    diff::{ChangeType, DiffEntry, DiffReport},
    version::Version,
    FileHandlingDecision,
};
use dctx_plugin_protocol::{
    json::decode::{decode_object, DecodeLimits},
    Json,
};
use dctx_template::{
    compile, from_bytes, render_legacy_diff, render_legacy_snapshot, render_markers, render_plan,
    to_bytes, AnswerRecord, CompiledTemplate, GrantRule, HostCeiling, MarkerNode, TreeStamps,
};

const BODY: &str = " \t{{name}}\r\n{{start_marker_open}}\n{{ unknown }}\n{{{triple}}}\n\\{{escaped}}\n{{{comp.tree}}} {{{comp.files}}} {{{project}}}\n{{{file.id}}} {{{file.path}}} {{{file.content}}} {{{node.mod}}}\nUnicode: café\n \t";
const FILE_PATH: &str = "src/{{name}}-{{start_marker_open}}-{{{node.id}}}-{{{file.content}}}.md";
const PROJECT: &str = "{{start_marker_open}}-{{{comp.files}}}";

fn json(text: &str) -> Json {
    decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("JSON object")
}

fn assert_body(page: &str, path: &str, body: &str) {
    let attribute = format!(" PATH=\"{path}\"");
    let (_, block) = page.split_once(&attribute).expect("literal file path");
    let (_, content) = block.split_once(">\n").expect("file start");
    let (actual, _) = content
        .split_once("\n</FILE_CONTENT_END ID=")
        .expect("file end");
    assert_eq!(actual.as_bytes(), body.as_bytes(), "{path}");
}

fn template_source(stamps: TreeStamps) -> String {
    let source = match stamps {
        TreeStamps::Off => include_str!("../../dctx-template/templates/default.dctx_template"),
        TreeStamps::On => include_str!("../../dctx-template/templates/stamped.dctx_template"),
    };
    source.replacen(
        "[VARIABLES]",
        "[VARIABLES]\nname = \"REPLACED\"\nescaped = \"REPLACED\"",
        1,
    )
}

#[test]
fn parsed_compiled_snapshot_and_diff_preserve_bodies_and_paths() {
    let mut root = MarkerNode::dir("", 0);
    root.children
        .push(MarkerNode::file(FILE_PATH, BODY.as_bytes(), 1));
    let version = Version::new_v1();
    let mut diff = DiffReport::default();
    diff.entries.push(DiffEntry {
        relative_path: FILE_PATH.to_string(),
        change_type: ChangeType::Modified,
        node_type: NodeType::File,
    });
    for stamps in [TreeStamps::Off, TreeStamps::On] {
        let mut template = dctx_template::Template::parse(&template_source(stamps));
        template.insert_variable("project", PROJECT);
        let compiled = template.compile("literal").expect("compile v1");
        let mut bytes = Vec::new();
        compiled.serialize(&mut bytes).expect("serialize");
        let loaded = CompiledTemplate::deserialize(&mut bytes.as_slice()).expect("load");
        for template in [template, loaded.to_template()] {
            let page = render_legacy_snapshot(&template, &root, &version, false, true);
            assert_body(&page, FILE_PATH, BODY);
            assert!(page.contains(&format!("[DIRECTORY:{PROJECT}]")));
            assert!(page.contains(&format!("[F] {FILE_PATH} (ID:F001")));
            assert!(
                !page.contains("{{attr_id}}"),
                "structure must still resolve"
            );
            assert_body(&render_markers(&template, &root, false), FILE_PATH, BODY);
            assert_eq!(
                template
                    .legacy_snapshot_within(&root, &version, false, true, page.len() as u64)
                    .expect("exact budget"),
                page
            );
            let changed = render_legacy_diff(&template, &root, &diff, &version, &version, true);
            assert_body(&changed, FILE_PATH, BODY);
            assert_eq!(
                template
                    .legacy_diff_within(
                        &root,
                        &diff,
                        &version,
                        &version,
                        true,
                        changed.len() as u64
                    )
                    .expect("exact diff budget"),
                changed
            );
        }
    }
}

#[test]
fn bytecode_text_and_json_preserve_bodies_and_paths_for_both_templates() {
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule {
        pattern: "src/**".to_string(),
        decision: FileHandlingDecision::Include,
    });
    let record = AnswerRecord {
        path: FILE_PATH.to_string(),
        is_dir: false,
        depth: 1,
        size: BODY.len() as u64,
        modified: 1,
        content: BODY.as_bytes().to_vec(),
    };
    for stamps in [TreeStamps::Off, TreeStamps::On] {
        let plan = compile(&template_source(stamps), &host).expect("compile");
        let mut plan = from_bytes(&to_bytes(&plan).expect("encode")).expect("decode");
        let page = render_plan(&plan, "pack", std::slice::from_ref(&record), PROJECT, &host)
            .expect("text");
        assert_body(&page, FILE_PATH, BODY);
        assert!(page.contains(&format!("[DIRECTORY:{PROJECT}]")));
        assert!(page.contains(&format!("[F] {FILE_PATH} (ID:F001")));
        plan.views[0].layout_label = "JSON".to_string();
        let page = render_plan(&plan, "pack", std::slice::from_ref(&record), PROJECT, &host)
            .expect("JSON");
        let decoded = json(&page);
        let records = decoded
            .get("records")
            .and_then(Json::as_array)
            .expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].get("path").and_then(Json::as_str),
            Some(FILE_PATH)
        );
        assert_eq!(
            records[0]
                .get("content")
                .and_then(Json::as_str)
                .expect("body")
                .as_bytes(),
            BODY.as_bytes()
        );
        assert_eq!(
            decoded.get("truncated").and_then(Json::as_bool),
            Some(false)
        );
    }
}

fn command(root: &Path, place: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-cli"));
    command
        .current_dir(place)
        .env_clear()
        .env("DCTX_AUTO_CLEAN", "0")
        .env("DCTX_CACHE_DIR", root.join("cache"));
    common::isolated_dirs(&mut command, root);
    command
}

/// A `dctx pack` page: stdout is exactly the page, the status line is the only stderr line.
fn ok_pack(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = String::from_utf8(output.stderr).expect("UTF-8 status");
    assert!(
        status.starts_with("pack bytes=") && status.lines().count() == 1,
        "{status}"
    );
    let page = String::from_utf8(output.stdout).expect("UTF-8 page");
    assert!(
        page.ends_with('\n') && !page.contains("pack bytes="),
        "{page}"
    );
    page
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 page")
}

#[test]
fn cli_pack_preserves_literal_source_and_the_original_reported_files() {
    let state = tempfile::tempdir().expect("state");
    let place = state.path().join(PROJECT);
    fs::create_dir_all(place.join("src")).expect("source directory");
    fs::create_dir(place.join("docs")).expect("docs");
    fs::write(place.join(FILE_PATH), BODY).expect("source");
    let originals = [
        (
            "docs/ADR-003-Templating-Engine.md",
            include_str!("../../../docs/ADR-003-Templating-Engine.md"),
        ),
        (
            "default.dctx_template",
            include_str!("../../dctx-template/templates/default.dctx_template"),
        ),
        (
            "stamped.dctx_template",
            include_str!("../../dctx-template/templates/stamped.dctx_template"),
        ),
    ];
    for (path, body) in originals {
        fs::write(place.join(path), body).expect("original source");
    }
    for stamps in ["--tree-stamps=off", "--tree-stamps=on"] {
        let page = ok_pack(
            command(state.path(), &place)
                .args(["pack", ".", stamps])
                .output()
                .expect("pack"),
        );
        assert_body(&page, FILE_PATH, BODY);
        for (path, body) in originals {
            assert_body(&page, path, body);
        }
        assert!(page.contains(&format!("[DIRECTORY:{PROJECT}]")));
        // CLI --json is intentionally a summary; source JSON is covered above.
        let page = ok_pack(
            command(state.path(), &place)
                .args(["pack", ".", stamps, "--json"])
                .output()
                .expect("pack summary"),
        );
        // stdout is exactly one JSON document.
        let report = json(page.strip_suffix('\n').expect("one line"));
        assert_eq!(report.get("files").and_then(Json::as_int), Some(4));
    }
}

#[test]
fn map_read_pack_for_and_mcp_rpc_keep_literal_source() {
    let state = tempfile::tempdir().expect("state");
    let place = state.path().join("repo");
    fs::create_dir(&place).expect("repo");
    let source = format!("pub fn literal_beacon() -> &'static str {{ r###\"{BODY}\"### }}\n");
    fs::write(place.join("literal.rs"), &source).expect("source");
    let page = ok(command(state.path(), &place)
        .args(["map", "read", "literal.rs"])
        .output()
        .expect("read"));
    assert!(page.contains(BODY), "{page}");
    let page = ok(command(state.path(), &place)
        .args(["map", "read", "literal.rs", "--json"])
        .output()
        .expect("JSON read"));
    let decoded = json(&page);
    let records = decoded
        .get("records")
        .and_then(Json::as_array)
        .expect("records");
    assert_eq!(
        records[0].get("source_utf8").and_then(Json::as_str),
        Some(source.as_str())
    );
    let page = ok(command(state.path(), &place)
        .args(["pack", "--for", "literal.rs", "--budget", "100000"])
        .output()
        .expect("pack for"));
    assert!(page.contains(BODY), "{page}");
    for args in [vec!["mcp"], vec!["serve"], vec!["serve", "--mcp"]] {
        let mut child = command(state.path(), &place)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("MCP");
        child.stdin.take().expect("stdin").write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{\"id\":\"m:literal.rs\"}}}\n").expect("RPC request");
        let reply = json(&ok(child.wait_with_output().expect("RPC response")));
        let result = reply.get("result").expect("result");
        assert!(result.get("isError").is_none(), "{reply:?}");
        let content = result
            .get("content")
            .and_then(Json::as_array)
            .expect("content");
        let page = content[0].get("text").and_then(Json::as_str).expect("text");
        assert!(page.contains(BODY), "{page}");
    }
}
