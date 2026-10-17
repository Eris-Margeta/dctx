//! Byte-level v1 marker rendering and the shipped default template.
//! Covers D3-03's opt-in legacy layouts without changing the body-only API.
//! Uses injected nodes and templates; SVG and pattern decisions have positive controls.

use dctx_core::FileHandlingDecision;
use dctx_core::diff::{ChangeType, DiffEntry, DiffReport};
use dctx_core::datatypes::NodeType;
use dctx_core::version::Version;
use dctx_template::{
    compile, render_legacy_diff, render_legacy_snapshot, render_markers, render_plan,
    AnswerRecord, GrantRule, HostCeiling, MarkerNode, Template,
};

const FIXTURE: &str = r#"
[VARIABLES]
attr_id = "ID"
attr_mod = "MOD"
attr_size = "SIZE"
[PARTIAL:TREE_ENTRY]
{{{node.type_char}}} {{{node.path}}} ({{attr_id}}:{{{node.id}}}, {{attr_mod}}:{{{node.mod}}}{{{node.size_part}}})
[PARTIAL:FILE_BLOCK]
<FILE ID="{{{file.id}}}">
{{{file.content}}}
</FILE ID="{{{file.id}}}">
[LAYOUT]
[DIRECTORY:{{{project}}}]
{{{comp.tree}}}
{{{comp.files}}}
"#;

const EXPECTED: &str = "\
[DIRECTORY:demo]
[D]  (ID:ROOT, MOD:9)
  [F] a.txt (ID:F001, MOD:5, SIZE:2)
<FILE ID=\"F001\">
hi
</FILE ID=\"F001\">";

fn fixture_template() -> Template {
    let mut template = Template::parse(FIXTURE);
    template.insert_variable("project", "demo");
    template
}

fn fixture_tree() -> MarkerNode {
    let mut root = MarkerNode::dir("", 9);
    root.children.push(MarkerNode::file("a.txt", b"hi", 5));
    root
}

#[test]
fn marker_fixture_matches_the_hand_computed_bytes() {
    let output = render_markers(&fixture_template(), &fixture_tree(), false);
    assert_eq!(output, EXPECTED);
}

#[test]
fn legacy_header_wraps_custom_template_and_version_without_changing_body() {
    let version = Version { major: 2, minor: 5 };
    let output = render_legacy_snapshot(&fixture_template(), &fixture_tree(), &version, false, true);
    assert_eq!(output, format!("[DCTX_LLM_CONTEXT_SNAPSHOT_V2.5]\n{EXPECTED}"));
}

#[test]
fn svg_setting_is_explicit_and_cannot_enable_pattern_omitted_content() {
    let mut root = MarkerNode::dir("", 1);
    let mut svg = MarkerNode::file("icon.svg", b"<svg/>", 2);
    svg.binary_label = Some("SVG file".to_string());
    root.children.push(svg);
    let template = fixture_template();
    let version = Version::new_v1();
    let ordinary = render_markers(&template, &root, false);
    assert!(ordinary.contains("[Binary file: SVG file, content omitted]"));
    let omitted = render_legacy_snapshot(&template, &root, &version, false, true);
    assert!(omitted.contains("[SVG file (omit_svg_content=true), content omitted]"));
    assert!(!omitted.contains("<svg/>"));
    let included = render_legacy_snapshot(&template, &root, &version, false, false);
    assert!(included.contains("<svg/>"));
    root.children[0].omit_content = true;
    let denied = render_legacy_snapshot(&template, &root, &version, false, false);
    assert!(denied.contains("[Content omitted by pattern rules]"));
    assert!(!denied.contains("<svg/>"));
}

#[test]
fn diff_ignores_removed_and_ignored_bytes_but_preserves_full_tree_ids() {
    let mut root = MarkerNode::dir("", 1);
    root.children.push(MarkerNode::file("removed", b"REMOVED", 2));
    let mut hidden = MarkerNode::file("ignored", b"PRIVATE", 2);
    hidden.is_ignored = true;
    root.children.push(hidden);
    root.children.push(MarkerNode::file("included", b"PUBLIC", 2));
    let mut diff = DiffReport::default();
    for (path, change_type) in [("included", ChangeType::Added), ("ignored", ChangeType::Modified), ("removed", ChangeType::Removed)] {
        diff.entries.push(DiffEntry { relative_path: path.to_string(), change_type, node_type: NodeType::File });
    }
    let output = render_legacy_diff(&fixture_template(), &root, &diff, &Version::new_v1(), &Version { major: 1, minor: 1 }, true);
    assert!(output.contains("[ADDED] included\n[MODIFIED] ignored\n[REMOVED] removed\n"));
    assert!(output.contains("<FILE ID=\"F003\">\nPUBLIC\n</FILE ID=\"F003\">\n"));
    assert!(!output.contains("PRIVATE"));
    assert!(!output.contains("\nREMOVED\n"));
    assert!(!output.contains("<FILE ID=\"F001\">"));
    assert!(!output.contains("<FILE ID=\"F002\">"));
}

#[test]
fn plan_render_matches_the_marker_fixture_for_a_flat_root() {
    let mut host = HostCeiling::base();
    host.grants.push(GrantRule { pattern: "**".to_string(), decision: FileHandlingDecision::Include });
    let plan = compile(FIXTURE, &host).expect("plan");
    let records = vec![
        AnswerRecord { path: String::new(), is_dir: true, depth: 0, size: 0, modified: 9, content: Vec::new() },
        AnswerRecord { path: "a.txt".to_string(), is_dir: false, depth: 1, size: 2, modified: 5, content: b"hi".to_vec() },
    ];
    let output = render_plan(&plan, "pack", &records, "demo", &host).expect("render");
    assert_eq!(output, EXPECTED);
}

#[test]
fn ignored_file_is_marked_and_its_bytes_are_absent() {
    let mut root = MarkerNode::dir("", 9);
    let mut hidden = MarkerNode::file("a.txt", b"SECRET", 5);
    hidden.is_ignored = true;
    root.children.push(hidden);
    let denied = render_markers(&fixture_template(), &root, false);
    assert!(denied.contains("a.txt [IGNORED]"));
    assert!(!denied.contains("SECRET"));
    assert!(!denied.contains("<FILE"));

    let mut shown = MarkerNode::dir("", 9);
    shown.children.push(MarkerNode::file("a.txt", b"SECRET", 5));
    let allowed = render_markers(&fixture_template(), &shown, false);
    assert!(allowed.contains("SECRET"));
}

#[test]
fn marker_tree_keeps_caller_child_order() {
    let mut root = MarkerNode::dir("", 1);
    root.children.push(MarkerNode::file("b.txt", b"B", 1));
    root.children.push(MarkerNode::file("a.txt", b"A", 1));
    let output = render_markers(&fixture_template(), &root, false);
    let b = output.find("b.txt").expect("b");
    let a = output.find("a.txt").expect("a");
    assert!(b < a);
}

#[test]
fn default_template_resolves_markers_and_renders_the_project() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/templates/default.dctx_template");
    let raw = std::fs::read_to_string(path).expect("default template");
    let mut template = Template::parse(&raw);
    let instructions = template.variables.get("instructions_text").expect("instructions").clone();
    assert!(instructions.contains("<DIRECTORY_TREE>"));
    assert!(instructions.contains("\\\""));
    let compiled = template.compile("default").expect("compile");
    assert!(compiled.partial_file_block.contains("<FILE_CONTENT_START"));
    assert!(!compiled.partial_file_block.contains("{{start_marker_open}}"));
    template.insert_variable("project", "demo");
    let mut root = MarkerNode::dir("", 1);
    root.children.push(MarkerNode::file("a.txt", b"hello", 2));
    let output = render_markers(&template, &root, false);
    assert!(output.contains("<INSTRUCTIONS>"));
    assert!(output.contains("</INSTRUCTIONS>"));
    assert!(output.contains("[DIRECTORY:demo]"));
    assert!(!output.contains("{MyProject}"));
    assert!(output.contains("hello"));
    let start = output
        .lines()
        .find(|line| line.contains("FILE_CONTENT_START") && line.contains("PATH="))
        .expect("file start");
    assert!(start.contains("LINES="));
    assert!(!start.contains("SIZE="));
}
