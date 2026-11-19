// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;
use std::io::ErrorKind;

use dctx_core::FileHandlingDecision::{DebugVisible, Exclude, Include, OmitContent};
use dctx_core::datatypes::NodeType;
use dctx_core::diff::{ChangeType, DiffEntry, DiffReport};
use dctx_core::version::Version;
use dctx_template::{
    AnswerRecord, CompiledTemplate, ContextPlan, GrantRule, HostCeiling, MarkerNode, Template,
    TemplateError, compile, load, render_legacy_diff, render_legacy_snapshot, render_plan,
    to_bytes,
};

const V2: &str = "\
[DTS]
version = 2
[SELECT:all]
[BUDGET:answer]
[VIEW:pack]
selection = \"all\"
budget = \"answer\"
";

const OP_FIXTURE: &str = "\
[VARIABLES]
label = \"é\"
prefix = \"{{label}}!\"
[PARTIAL:label]
{{prefix}}{{{project}}}
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}}|{{{node.path}}}|{{{node.id}}}|{{{node.mod}}}|{{{node.size_part}}}
[PARTIAL:FILE_BLOCK]
{{{file.id}}}|{{{file.path}}}|{{{file.lines}}}|{{{file.size}}}|{{{file.content}}}
[LAYOUT]
begin:{{>label}}\n{{{comp.tree}}}\n{{{comp.files}}}:end
";

fn render(
    plan: &ContextPlan,
    records: &[AnswerRecord],
    project: &str,
    host: &HostCeiling,
) -> Result<String, TemplateError> {
    render_plan(plan, "pack", records, project, host)
}

fn include_all() -> HostCeiling {
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule {
        pattern: "*".into(),
        decision: Include,
    });
    host
}

fn file(path: &str, bytes: &[u8]) -> AnswerRecord {
    AnswerRecord {
        path: path.into(),
        is_dir: false,
        depth: 1,
        size: bytes.len() as u64,
        modified: 7,
        content: bytes.to_vec(),
    }
}

fn limit(what: &'static str, limit: u64) -> TemplateError {
    TemplateError::Limit { what, limit }
}

#[test]
fn repeated_lowering_produces_identical_opcodes_and_binary() {
    let host = include_all();
    let source = format!("{V2}{OP_FIXTURE}");
    let first = compile(&source, &host).unwrap();
    let second = compile(&source, &host).unwrap();
    assert_eq!(first.render, second.render);
    assert_eq!(to_bytes(&first).unwrap(), to_bytes(&second).unwrap());
}

#[test]
fn text_data_and_partial_opcodes_render_every_data_field() {
    let host = include_all();
    let plan = compile(&format!("{V2}{OP_FIXTURE}"), &host).unwrap();
    let mut opcodes: Vec<_> = plan
        .render
        .layouts
        .iter()
        .chain(&plan.render.partials)
        .flat_map(|code| code.instructions.iter().map(|inst| inst.opcode))
        .collect();
    opcodes.sort_unstable();
    opcodes.dedup();
    assert_eq!(opcodes, [1, 2, 3]);

    let plan = load(&to_bytes(&plan).unwrap(), &host).unwrap();
    let records = [file("x.txt", b"one\n{{prefix}}")];
    let expected =
        "begin:é!{{>label}}\n[F]|x.txt|F001|7|, SIZE:14\nF001|x.txt|2|14|one\n{{prefix}}:end";
    assert_eq!(
        render(&plan, &records, "{{>label}}", &host)
            .unwrap()
            .as_bytes(),
        expected.as_bytes()
    );
}

#[test]
fn output_byte_limit_is_inclusive_and_never_cuts_utf8() {
    let host = HostCeiling::base();
    let source = format!("{V2}[LAYOUT]\n{{{{{{project}}}}}}\n");
    let mut plan = compile(&source, &host).unwrap();
    plan.budgets[0].bytes = 4;
    assert_eq!(
        render(&plan, &[], "éé", &host).unwrap().as_bytes(),
        b"\xc3\xa9\xc3\xa9"
    );
    assert_eq!(
        render(&plan, &[], "éé!", &host),
        Err(TemplateError::BudgetTooSmall { minimum_bytes: 5 })
    );
}

#[test]
fn record_loop_refuses_the_first_record_over_its_input_limit() {
    const MAX_RECORDS: usize = 100_000;
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule {
        pattern: "last".into(),
        decision: Include,
    });
    let source = format!(
        "{V2}[PARTIAL:FILE_BLOCK]\n{{{{{{file.content}}}}}}\n[LAYOUT]\n{{{{{{comp.files}}}}}}\n"
    );
    let plan = compile(&source, &host).unwrap();
    let mut records = vec![file("", b""); MAX_RECORDS - 1];
    records.push(file("last", b"complete"));
    assert_eq!(render(&plan, &records, "", &host).unwrap(), "complete");
    records.push(file("", b""));
    assert_eq!(
        render(&plan, &records, "", &host),
        Err(limit("records", MAX_RECORDS as u64))
    );
}

#[test]
fn record_budget_marks_omitted_whole_records() {
    let host = include_all();
    let source = format!(
        "{V2}[PARTIAL:FILE_BLOCK]\n<{{{{{{file.content}}}}}}>\n[LAYOUT]\n{{{{{{comp.files}}}}}}\n"
    );
    let mut plan = compile(&source, &host).unwrap();
    plan.budgets[0].records = 1;
    let first = file("a", "é".as_bytes());
    assert_eq!(
        render(&plan, std::slice::from_ref(&first), "", &host).unwrap(),
        "<é>"
    );
    assert_eq!(
        render(&plan, &[first, file("b", b"two")], "", &host).unwrap(),
        "<é>[DCTX_CONTINUED]\n"
    );
}

fn include_chain(depth: usize, tail: &str) -> String {
    let mut source = format!("{V2}[LAYOUT]\n{{{{>p1}}}}\n");
    for level in (1..=depth).rev() {
        writeln!(source, "[PARTIAL:p{level}]").unwrap();
        if level == depth {
            writeln!(source, "{tail}").unwrap();
        } else {
            writeln!(source, "{{{{>p{}}}}}", level + 1).unwrap();
        }
    }
    source
}

#[test]
fn include_depth_limit_is_inclusive() {
    let host = HostCeiling::base();
    let at_limit = compile(&include_chain(32, "leaf"), &host).unwrap();
    assert_eq!(render(&at_limit, &[], "", &host).unwrap(), "leaf");
    let over_limit = compile(&include_chain(33, "leaf"), &host).unwrap();
    assert_eq!(
        render(&over_limit, &[], "", &host),
        Err(limit("include-depth", 32))
    );
}

#[test]
fn opcode_limit_is_inclusive() {
    const MAX_OPCODES: usize = 10_000;
    let host = HostCeiling::base();
    let source = |count| format!("{V2}[LAYOUT]\n{}", "{{{project}}}".repeat(count));
    let plan = compile(&source(MAX_OPCODES), &host).unwrap();
    assert_eq!(plan.render.instruction_count(), MAX_OPCODES);
    assert_eq!(
        render(&plan, &[], "x", &host).unwrap(),
        "x".repeat(MAX_OPCODES)
    );
    assert_eq!(
        compile(&source(MAX_OPCODES + 1), &host),
        Err(limit("instructions", MAX_OPCODES as u64))
    );
}

#[test]
fn hostile_source_returns_named_errors() {
    let host = HostCeiling::base();
    for (input, name) in [
        ("{{{project}}", "project"),
        ("{{project}}}", "project"),
        ("{{#files}}", "#files"),
        ("{{/files}}", "/files"),
        ("{{{unknown.directive}}}", "unknown.directive"),
    ] {
        assert_eq!(
            compile(&format!("{V2}[LAYOUT]\n{input}\n"), &host),
            Err(TemplateError::UnresolvedVariable { name: name.into() }),
            "{input}"
        );
    }
    assert_eq!(
        compile(&format!("{V2}[LAYOUT]\na\0b"), &host),
        Err(TemplateError::InvalidSyntax {
            line: 0,
            detail: "nul".into()
        })
    );
    assert_eq!(
        compile(&format!("{V2}[LAYOUT]\n{}", "x".repeat(1024 * 1024)), &host),
        Err(limit("source", 1024 * 1024))
    );
    assert_eq!(
        compile(
            &format!("{V2}[PARTIAL:self]\n{{{{>self}}}}\n[LAYOUT]\n{{{{>self}}}}"),
            &host
        ),
        Err(TemplateError::Cycle {
            kind: "partial",
            name: "self".into()
        })
    );
}

#[test]
fn deep_includes_refuse_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let host = HostCeiling::base();
            let plan = compile(&include_chain(10_000, ""), &host).unwrap();
            assert_eq!(plan.render.instruction_count(), 10_000);
            assert_eq!(
                render(&plan, &[], "", &host),
                Err(limit("include-depth", 32))
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn invalid_utf8_template_bytes_are_refused_by_the_byte_decoder() {
    let mut encoded = Vec::new();
    Template::parse("[LAYOUT]\nlayout")
        .compile("unique-name")
        .unwrap()
        .serialize(&mut encoded)
        .unwrap();
    assert!(CompiledTemplate::deserialize(&mut encoded.as_slice()).is_ok());
    let offset = encoded
        .windows(b"unique-name".len())
        .position(|bytes| bytes == b"unique-name")
        .unwrap();
    encoded[offset] = 0xff;
    let error = CompiledTemplate::deserialize(&mut encoded.as_slice()).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::InvalidData);
    assert!(error.to_string().contains("invalid utf-8"));
}

fn binary(path: &str, size: u64, modified: u64, label: &str) -> MarkerNode {
    let mut node = MarkerNode::file(path, b"", modified);
    node.content = None;
    node.content_size = size;
    node.binary_label = Some(label.into());
    node
}

fn snapshot_tree() -> MarkerNode {
    let mut git = MarkerNode::dir(".git", 1600);
    git.is_ignored = true;
    let mut config = MarkerNode::file(".git/config", b"sensitive", 1700);
    config.is_ignored = true;
    git.children.push(config);
    let mut omitted = MarkerNode::file("Cargo.lock", b"omitted", 1500);
    omitted.omit_content = true;
    let mut secret = MarkerNode::file("secret.env", b"SECRET=1\n", 1300);
    secret.is_ignored = true;
    let mut src = MarkerNode::dir("src", 1100);
    src.children
        .push(MarkerNode::file("src/main.rs", b"fn main() {}\n", 1200));
    let mut root = MarkerNode::dir("", 1000);
    root.children = vec![
        git,
        omitted,
        binary("logo.png", 7, 1400, "PNG image"),
        secret,
        src,
    ];
    root
}

fn diff_tree() -> (MarkerNode, DiffReport) {
    let mut root = MarkerNode::dir("", 1000);
    root.children = vec![
        MarkerNode::file("added.txt", b"NEW!", 2000),
        MarkerNode::file("changed.txt", b"New content", 2100),
        binary("icon.svg", 6, 2200, "SVG file"),
        MarkerNode::dir("new_dir", 2300),
        MarkerNode::file("unchanged.txt", b"Old content", 2400),
    ];
    let mut diff = DiffReport::default();
    for (path, change_type, node_type) in [
        ("added.txt", ChangeType::Added, NodeType::File),
        ("changed.txt", ChangeType::Modified, NodeType::File),
        ("deleted_file.txt", ChangeType::Removed, NodeType::File),
        ("new_dir", ChangeType::Added, NodeType::Directory),
        ("icon.svg", ChangeType::Modified, NodeType::File),
    ] {
        diff.entries.push(DiffEntry {
            change_type,
            node_type,
            relative_path: path.into(),
        });
    }
    (root, diff)
}

fn fixture_records(root: &MarkerNode) -> (Vec<AnswerRecord>, HostCeiling) {
    fn flatten(
        node: &MarkerNode,
        depth: u32,
        records: &mut Vec<AnswerRecord>,
        host: &mut HostCeiling,
    ) {
        let decision = if node.is_ignored {
            DebugVisible
        } else if node.omit_content || node.binary_label.is_some() {
            OmitContent
        } else {
            Include
        };
        // The root uses the wildcard grant because an empty grant pattern is invalid.
        if !node.relative_path.is_empty() {
            host.grants.push(GrantRule {
                pattern: node.relative_path.clone(),
                decision,
            });
        }
        records.push(AnswerRecord {
            path: node.relative_path.clone(),
            is_dir: node.is_dir,
            depth,
            size: node.content_size,
            modified: node.modified,
            content: node.content.clone().unwrap_or_default(),
        });
        for child in &node.children {
            flatten(child, depth + 1, records, host);
        }
    }
    let mut host = include_all();
    let mut records = Vec::new();
    flatten(root, 0, &mut records, &mut host);
    (records, host)
}

fn legacy_binary_label(output: &str, path: &str, label: &str) -> String {
    // AnswerRecord uses the generic omission reason. Decorate it for the v1 envelope.
    let field = format!("PATH=\"{path}\" LINES=\"0\">\n");
    let generic = format!("{field}[Content omitted by pattern rules]");
    assert_eq!(output.matches(&generic).count(), 1);
    output.replacen(&generic, &format!("{field}{label}"), 1)
}

#[test]
fn snapshot_and_tree_opcodes_match_all_four_legacy_fixtures() {
    let root = snapshot_tree();
    let version = Version::new_v1();
    let (records, host) = fixture_records(&root);
    for (source, snapshot, tree) in [
        (
            include_str!("../templates/default.dctx_template"),
            include_bytes!("fixtures/legacy-snapshot.txt").as_slice(),
            include_bytes!("fixtures/legacy-tree.txt").as_slice(),
        ),
        (
            include_str!("../templates/stamped.dctx_template"),
            include_bytes!("fixtures/legacy-snapshot-stamped.txt").as_slice(),
            include_bytes!("fixtures/legacy-tree-stamped.txt").as_slice(),
        ),
    ] {
        let mut legacy = Template::parse(source);
        legacy.insert_variable("project", "Corpus");
        let mut plan = compile(source, &host).unwrap();
        let full = render(&plan, &records, "Corpus", &host).unwrap();
        let full = legacy_binary_label(
            &full,
            "logo.png",
            "[Binary file: PNG image, content omitted]",
        );
        plan.render
            .partials
            .retain(|code| code.name != "FILE_BLOCK");
        let tree_body = render(&plan, &records, "Corpus", &host).unwrap();
        for (tree_only, body, expected) in [(false, full, snapshot), (true, tree_body, tree)] {
            let opcode_envelope = format!("[DCTX_LLM_CONTEXT_SNAPSHOT_{version}]\n{body}");
            let legacy_envelope = render_legacy_snapshot(&legacy, &root, &version, tree_only, true);
            assert_eq!(legacy_envelope.as_bytes(), expected);
            assert_eq!(opcode_envelope.as_bytes(), expected);
        }
    }
}

fn diff_summary(diff: &DiffReport, old: &Version, new: &Version) -> String {
    let mut summary = format!(
        "[DCTX_LLM_CONTEXT_DIFF_{new}]\nVersion Change: {old} -> {new}\n\n<CHANGES_SUMMARY>\n"
    );
    for entry in &diff.entries {
        let change = match entry.change_type {
            ChangeType::Added => "ADDED",
            ChangeType::Modified => "MODIFIED",
            ChangeType::Removed => "REMOVED",
        };
        let suffix = if entry.node_type == NodeType::Directory {
            "/"
        } else {
            ""
        };
        writeln!(summary, "[{change}] {}{suffix}", entry.relative_path).unwrap();
    }
    summary.push_str("</CHANGES_SUMMARY>\n\n");
    summary
}

#[test]
fn diff_tree_and_file_opcodes_match_both_legacy_diff_fixtures() {
    let (root, diff) = diff_tree();
    let (records, host) = fixture_records(&root);
    let mut changed_host = host.clone();
    // In this fixture the unchanged record follows all changed records, preserving their IDs.
    changed_host.grants.push(GrantRule {
        pattern: "unchanged.txt".into(),
        decision: Exclude,
    });
    let old = Version::new_v1();
    let new = Version { major: 1, minor: 1 };
    let summary = diff_summary(&diff, &old, &new);

    for (source, expected) in [
        (
            include_str!("../templates/default.dctx_template"),
            include_bytes!("fixtures/legacy-diff.txt").as_slice(),
        ),
        (
            include_str!("../templates/stamped.dctx_template"),
            include_bytes!("fixtures/legacy-diff-stamped.txt").as_slice(),
        ),
    ] {
        let legacy = render_legacy_diff(&Template::parse(source), &root, &diff, &old, &new, true);
        let (partials, _) = source.split_once("\n[LAYOUT]\n").unwrap();
        let render_component = |field, ceiling| {
            let plan = compile(
                &format!("{partials}\n[LAYOUT]\n{{{{{{{field}}}}}}}"),
                ceiling,
            )
            .unwrap();
            render(&plan, &records, "", ceiling).unwrap()
        };
        let tree = render_component("comp.tree", &host);
        let files = render_component("comp.files", &changed_host);
        let files = legacy_binary_label(
            &files,
            "icon.svg",
            "[SVG file, content omitted (omit_svg_content=true)]",
        );
        let output = format!(
            "{summary}<UPDATED_DIRECTORY_TREE>\n{tree}</UPDATED_DIRECTORY_TREE>\n\n{files}\n"
        );
        assert_eq!(legacy.as_bytes(), expected);
        assert_eq!(output.as_bytes(), expected);
    }
}

fn sample(state: &mut u64, upper: usize) -> usize {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    usize::try_from(*state % u64::try_from(upper).unwrap()).unwrap()
}

#[test]
fn seeded_templates_render_exact_bounded_output_or_return_typed_errors() {
    const SEED: u64 = 0x1118_9e37_79b9_7f4a;
    const CASES: usize = 5_000;
    const PARTS: [(&str, &str); 6] = [
        ("é", "é"),
        ("[literal]", "[literal]"),
        ("{{word}}", "λ"),
        ("{{{project}}}", "{{>leaf}}"),
        ("{{>leaf}}", "<λ:{{>leaf}}>"),
        (" / ", " / "),
    ];
    eprintln!("template seed {SEED:#018x}, {CASES} cases");
    let mut random = SEED;
    let host = HostCeiling::base();
    let mut outcomes = [0; 3];
    for case in 0..CASES {
        let budget = 2 + sample(&mut random, 150);
        let mut source = V2.replace(
            "[BUDGET:answer]",
            &format!("[BUDGET:answer]\nbytes = {budget}"),
        );
        writeln!(
            source,
            "[VARIABLES]\nword = \"λ\"\n[PARTIAL:leaf]\n<{{{{word}}}}:{{{{{{project}}}}}}>\n[LAYOUT]"
        ).unwrap();

        source.push('|');
        let mut expected = String::from("|");
        for _ in 0..sample(&mut random, 20) {
            let (fragment, bytes) = PARTS[sample(&mut random, PARTS.len())];
            source.push_str(fragment);
            expected.push_str(bytes);
        }
        let refusal = match sample(&mut random, 8) {
            0 => {
                source.push_str("{{{unknown}}}");
                Some(TemplateError::UnresolvedVariable {
                    name: "unknown".into(),
                })
            }
            1 => {
                source.push_str("{{>missing}}");
                Some(TemplateError::UnresolvedRef {
                    kind: "partial",
                    name: "missing".into(),
                })
            }
            _ => None,
        };
        source.push('|');
        expected.push('|');
        let context = || format!("seed {SEED:#018x}, case {case}, source {source:?}");
        if let Some(error) = refusal {
            assert_eq!(compile(&source, &host), Err(error), "{}", context());
            outcomes[0] += 1;
            continue;
        }

        let plan = compile(&source, &host).unwrap_or_else(|error| panic!("{}: {error}", context()));
        let result = render(&plan, &[], "{{>leaf}}", &host);
        if expected.len() <= budget {
            assert_eq!(result, Ok(expected), "{}", context());
            outcomes[1] += 1;
        } else {
            assert_eq!(
                result,
                Err(TemplateError::BudgetTooSmall {
                    minimum_bytes: expected.len() as u64
                }),
                "{}",
                context()
            );
            outcomes[2] += 1;
        }
    }
    assert!(
        outcomes.into_iter().all(|count| count > 0),
        "seed {SEED:#018x}: {outcomes:?}"
    );
}
