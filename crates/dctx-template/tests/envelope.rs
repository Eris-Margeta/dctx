// SPDX-License-Identifier: Apache-2.0
//! Whole-record envelopes, continuation, JSON escaping and denied bytes.

use dctx_core::FileHandlingDecision;
use dctx_template::{AnswerRecord, GrantRule, HostCeiling, TemplateError, compile, render_plan};

fn layout() -> &'static str {
    "\
[PARTIAL:TREE_ENTRY]
{{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.files}}}
"
}

fn host_src() -> HostCeiling {
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule {
        pattern: "src/**".to_string(),
        decision: FileHandlingDecision::Include,
    });
    host
}

fn file(path: &str, content: &str, depth: u32) -> AnswerRecord {
    AnswerRecord {
        path: path.to_string(),
        is_dir: false,
        depth,
        size: content.len() as u64,
        modified: 1,
        content: content.as_bytes().to_vec(),
    }
}

#[test]
fn denied_bytes_stay_out_and_the_allowed_twin_is_emitted() {
    let host = host_src();
    let plan = compile(layout(), &host).expect("v1");
    let records = vec![
        file("secret.env", "SECRET", 1),
        file("src/main.rs", "VISIBLE", 1),
    ];
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("render");
    assert!(output.contains("VISIBLE"));
    assert!(!output.contains("SECRET"));
    assert!(!output.contains("secret.env"));
}

#[test]
fn a_template_exclude_drops_a_path_the_host_allows() {
    let source = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
exclude = [\"src/secret.rs\"]
[INDEX:s]
parser = \"opaque\"
[BUDGET:b]
records = 10
[VIEW:pack]
selection = \"s\"
budget = \"b\"
[PARTIAL:TREE_ENTRY]
{{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.files}}}
";
    let host = host_src();
    let plan = compile(source, &host).expect("plan");
    let records = vec![
        file("src/secret.rs", "SECRET", 1),
        file("src/ok.rs", "OKAY", 1),
    ];
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("render");
    assert!(output.contains("OKAY"));
    assert!(!output.contains("SECRET"));
}

#[test]
fn records_are_whole_and_sorted_and_depth_drops_the_deeper_one() {
    let source = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"opaque\"
[BUDGET:b]
records = 1
[VIEW:pack]
selection = \"s\"
budget = \"b\"
[PARTIAL:TREE_ENTRY]
{{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.files}}}
";
    let host = host_src();
    let plan = compile(source, &host).expect("one");
    let records = vec![file("src/b.txt", "BBBB", 1), file("src/a.txt", "AAAA", 1)];
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("render");
    assert!(output.contains("AAAA"));
    assert!(!output.contains("BBBB"));
    assert!(output.contains("[DCTX_CONTINUED]\n"));
    let a = output.find("AAAA").expect("a");
    assert!(output.find("src/b.txt").is_none());
    assert!(a < output.find("[DCTX_CONTINUED]").expect("mark"));

    let both = source.replace("records = 1", "records = 2");
    let plan = compile(&both, &host).expect("two");
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("both");
    assert!(output.contains("AAAA"));
    assert!(output.contains("BBBB"));
    assert!(!output.contains("[DCTX_CONTINUED]"));
    assert!(output.find("AAAA").expect("a") < output.find("BBBB").expect("b"));

    let deep = both.replace("records = 2", "records = 2\ndepth = 1");
    let plan = compile(&deep, &host).expect("depth");
    let records = vec![
        file("src/keep.rs", "KEEP", 1),
        file("src/deep.rs", "DEEP", 2),
    ];
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("shallow");
    assert!(output.contains("KEEP"));
    assert!(!output.contains("DEEP"));
    assert!(!output.contains("[DCTX_CONTINUED]"));
}

#[test]
fn an_envelope_that_cannot_fit_is_budget_too_small() {
    let source = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"opaque\"
[BUDGET:b]
bytes = 0
records = 4
[VIEW:pack]
selection = \"s\"
budget = \"b\"
[PARTIAL:FILE_BLOCK]
{{{file.content}}}
[LAYOUT]
{{{comp.files}}}
";
    let host = host_src();
    let plan = compile(source, &host).expect("plan");
    let err = render_plan(
        &plan,
        "pack",
        &[file("src/a.txt", "HELLO", 1)],
        "demo",
        &host,
    )
    .expect_err("small");
    assert!(matches!(err, TemplateError::BudgetTooSmall { minimum_bytes } if minimum_bytes > 0));
}

#[test]
#[allow(
    clippy::similar_names,
    reason = "plan and plain distinguish the compiled plan from its plain rendered output in this escaping comparison."
)]
fn json_counts_escaped_utf8_and_quotes() {
    let wide = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"opaque\"
[BUDGET:b]
records = 4
[VIEW:pack]
selection = \"s\"
budget = \"b\"
layout = \"JSON\"
";
    let host = host_src();
    let plan = compile(wide, &host).expect("wide");
    let plain =
        render_plan(&plan, "pack", &[file("src/a.txt", "e", 1)], "demo", &host).expect("plain");
    let unicode =
        render_plan(&plan, "pack", &[file("src/a.txt", "é", 1)], "demo", &host).expect("unicode");
    let quoted = render_plan(
        &plan,
        "pack",
        &[file("src/a.txt", "a\"b", 1)],
        "demo",
        &host,
    )
    .expect("quoted");
    assert!(unicode.len() > plain.len());
    assert!(quoted.contains("\\\""));
    assert!(plain.contains('e'));

    let tight = wide.replace(
        "[BUDGET:b]\nrecords = 4",
        &format!("[BUDGET:b]\nbytes = {}\nrecords = 4", plain.len()),
    );
    let plan = compile(&tight, &host).expect("tight");
    let ok = render_plan(&plan, "pack", &[file("src/a.txt", "e", 1)], "demo", &host).expect("fits");
    assert!(ok.contains('e'));
    let missed = render_plan(
        &plan,
        "pack",
        &[file("src/a.txt", "a\"b", 1)],
        "demo",
        &host,
    );
    match missed {
        Ok(text) => assert!(!text.contains("a\\\"b") && !text.contains("a\"b")),
        Err(TemplateError::BudgetTooSmall { .. }) => {}
        Err(other) => panic!("unexpected {other}"),
    }
    let missed = render_plan(&plan, "pack", &[file("src/a.txt", "é", 1)], "demo", &host);
    match missed {
        Ok(text) => assert!(!text.contains('é')),
        Err(TemplateError::BudgetTooSmall { .. }) => {}
        Err(other) => panic!("unexpected {other}"),
    }
}

#[test]
fn plan_render_preserves_backslashes_and_the_end_marker() {
    let host = host_src();
    let plan = compile(layout(), &host).expect("plan");
    let output = render_plan(
        &plan,
        "pack",
        &[file("src/a.txt", "a\\b</FILE>c", 1)],
        "demo",
        &host,
    )
    .expect("render");
    assert!(output.contains("a\\b</FILE>c"));
}
