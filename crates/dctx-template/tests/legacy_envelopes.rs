//! Pinned legacy formatter envelopes and compiled-template bytes (DX-D3-01).
//! Fixtures are the bytes the legacy formatter and `CompiledTemplate::serialize` produce
//! for a fixed tree and the `pin` template. This file calls only `dctx_template`.
//! Depends on dctx-template. Run by `cargo test -p dctx-template --test legacy_envelopes`.
//! D3-03 closes the envelope gaps without modifying the pinned fixtures.

use std::io::Cursor;

use dctx_core::datatypes::NodeType;
use dctx_core::diff::{ChangeType, DiffEntry, DiffReport};
use dctx_core::version::Version;
use dctx_template::{
    render_legacy_diff, render_legacy_snapshot, render_markers, CompiledTemplate, MarkerNode, Template, TreeStamps,
};

const SNAPSHOT: &str = include_str!("fixtures/legacy-snapshot.txt");
const TREE: &str = include_str!("fixtures/legacy-tree.txt");
const DIFF: &str = include_str!("fixtures/legacy-diff.txt");
const SNAPSHOT_STAMPED: &str = include_str!("fixtures/legacy-snapshot-stamped.txt");
const TREE_STAMPED: &str = include_str!("fixtures/legacy-tree-stamped.txt");
const DIFF_STAMPED: &str = include_str!("fixtures/legacy-diff-stamped.txt");
const HEX: &str = include_str!("fixtures/legacy-template.hex");
const HEADER: &str = "[DCTX_LLM_CONTEXT_SNAPSHOT_V1]";

const PIN: &str = "\
[VARIABLES]
zeta = \"Z\"
alpha = \"A\"
[PARTIAL:TREE_ENTRY]
{{alpha}} {{{node.path}}}
[PARTIAL:FILE_BLOCK]
{{zeta}} {{{file.content}}}
[LAYOUT]
{{alpha}}-{{zeta}}
{{{comp.tree}}}
";

/// The default template: tree lines carry no `MOD:` timestamp.
fn template() -> Template {
    template_with(TreeStamps::Off)
}

/// The stamped built-in: the 1.0.0 text, `MOD:` on every tree line.
fn stamped_template() -> Template {
    template_with(TreeStamps::On)
}

fn template_with(stamps: TreeStamps) -> Template {
    let mut parsed = stamps.template();
    parsed.insert_variable("project", "Corpus");
    parsed
}

fn omitted(path: &str, size: u64, modified: u64) -> MarkerNode {
    let mut node = MarkerNode::file(path, &[], modified);
    node.content = None;
    node.content_size = size;
    node.omit_content = true;
    node
}

fn binary(path: &str, size: u64, modified: u64, label: &str) -> MarkerNode {
    let mut node = MarkerNode::file(path, &[], modified);
    node.content = None;
    node.content_size = size;
    node.binary_label = Some(label.to_string());
    node
}

fn ignored(path: &str, bytes: &[u8], modified: u64) -> MarkerNode {
    let mut node = MarkerNode::file(path, bytes, modified);
    node.is_ignored = true;
    node
}

fn snapshot_tree() -> MarkerNode {
    let mut git = MarkerNode::dir(".git", 1600);
    git.is_ignored = true;
    git.children.push(ignored(".git/config", b"sensitive", 1700));
    let mut src = MarkerNode::dir("src", 1100);
    src.children.push(MarkerNode::file("src/main.rs", b"fn main() {}\n", 1200));
    let mut root = MarkerNode::dir("", 1000);
    root.children.push(git);
    root.children.push(omitted("Cargo.lock", 7, 1500));
    root.children.push(binary("logo.png", 7, 1400, "PNG image"));
    root.children.push(ignored("secret.env", b"SECRET=1\n", 1300));
    root.children.push(src);
    root
}

fn body(fixture: &str) -> &str {
    fixture.strip_prefix(HEADER).expect("header").strip_prefix('\n').expect("newline")
}

fn decode_hex(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines() {
        let code = line.split('#').next().unwrap_or("");
        for token in code.split_whitespace() {
            out.push(u8::from_str_radix(token, 16).unwrap_or_else(|_| panic!("bad hex {token}")));
        }
    }
    out
}

fn hex_sections() -> (Vec<u8>, Vec<u8>) {
    let mut sorted = String::new();
    let mut unsorted = String::new();
    let mut which = "";
    for line in HEX.lines() {
        match line.trim() {
            "SORTED" => which = "sorted",
            "UNSORTED" => which = "unsorted",
            _ => match which {
                "sorted" => {
                    sorted.push_str(line);
                    sorted.push('\n');
                }
                "unsorted" => {
                    unsorted.push_str(line);
                    unsorted.push('\n');
                }
                _ => {}
            },
        }
    }
    let sorted = decode_hex(&sorted);
    let unsorted = decode_hex(&unsorted);
    assert!(!sorted.is_empty() && sorted != unsorted);
    (sorted, unsorted)
}

#[test]
fn snapshot_body_matches_legacy_layout() {
    let got = render_markers(&template(), &snapshot_tree(), false);
    assert_eq!(got, body(SNAPSHOT));
    assert!(got.contains("[DIRECTORY:Corpus]"));
    assert!(!got.contains("{Corpus}"));
    assert!(got.contains("fn main() {}"));
    assert!(got.contains("secret.env [IGNORED]"));
    assert!(got.contains("    [F] src/main.rs"));
    assert!(got.contains(".git [IGNORED]"));
    assert!(got.contains(".git/config [IGNORED]"));
    assert!(got.contains("ID:F007"));
    assert!(got.contains("[Content omitted by pattern rules]"));
    assert!(got.contains("[Binary file: PNG image, content omitted]"));
    assert!(!got.contains("SECRET=1"));
    assert!(!got.contains("sensitive"));
}

#[test]
fn snapshot_envelope_matches_legacy_bytes() {
    assert_eq!(render_legacy_snapshot(&template(), &snapshot_tree(), &Version::new_v1(), false, true), SNAPSHOT);
}

#[test]
fn tree_only_body_matches_legacy_layout() {
    let got = render_markers(&template(), &snapshot_tree(), true);
    assert_eq!(got, body(TREE));
    assert!(got.contains("src/main.rs"));
    assert!(!got.contains("fn main() {}"));
    assert!(!got.contains("SECRET=1"));
    assert!(!got.contains("LINES="));
}

#[test]
fn tree_only_envelope_matches_legacy_bytes() {
    assert_eq!(render_legacy_snapshot(&template(), &snapshot_tree(), &Version::new_v1(), true, true), TREE);
}

#[test]
fn stamped_snapshot_and_tree_match_the_1_0_0_bytes() {
    let tree = snapshot_tree();
    let version = Version::new_v1();
    assert_eq!(render_legacy_snapshot(&stamped_template(), &tree, &version, false, true), SNAPSHOT_STAMPED);
    assert_eq!(render_legacy_snapshot(&stamped_template(), &tree, &version, true, true), TREE_STAMPED);
}

#[test]
fn default_output_has_no_timestamps_and_ignores_them() {
    let first = render_legacy_snapshot(&template(), &snapshot_tree(), &Version::new_v1(), false, true);
    assert!(!first.contains("MOD"), "default snapshot must not mention MOD");
    let mut touched = snapshot_tree();
    touched.modified += 1;
    for child in &mut touched.children {
        child.modified += 7;
    }
    assert_eq!(render_legacy_snapshot(&template(), &touched, &Version::new_v1(), false, true), first);
    let stamped = render_legacy_snapshot(&stamped_template(), &touched, &Version::new_v1(), false, true);
    assert_ne!(stamped, SNAPSHOT_STAMPED, "the stamped text does follow the timestamps");
}

#[test]
fn the_two_default_template_copies_are_identical_and_mod_free() {
    let template_copy = include_str!("../templates/default.dctx_template");
    let core_copy = include_str!("../../dctx-core/templates/default.dctx_template");
    assert_eq!(template_copy, core_copy, "dctx-template and dctx-core ship one default template");
    assert!(!template_copy.contains("MOD:TIMESTAMP") && !template_copy.contains(", {{attr_mod}}:"));
    let stamped = include_str!("../templates/stamped.dctx_template");
    assert!(stamped.contains("{{attr_mod}}:TIMESTAMP") && stamped.contains("{{attr_mod}}:{{{node.mod}}}"));
}

#[test]
fn tree_stamps_values_parse_strictly() {
    assert_eq!(TreeStamps::parse("on"), Ok(TreeStamps::On));
    assert_eq!(TreeStamps::parse("off"), Ok(TreeStamps::Off));
    assert!(TreeStamps::parse("ON").is_err() && TreeStamps::parse("").is_err());
}

#[test]
fn custom_templates_keep_the_mod_placeholder() {
    let source = "[VARIABLES]\nattr_mod = \"MOD\"\n[PARTIAL:TREE_ENTRY]\n{{{node.path}}} {{attr_mod}}:{{{node.mod}}}\n[PARTIAL:FILE_BLOCK]\n{{{file.content}}}\n[LAYOUT]\n{{{comp.tree}}}\n";
    let out = render_markers(&Template::parse(source), &snapshot_tree(), true);
    assert!(out.contains("Cargo.lock MOD:1500"), "{out}");
}

#[test]
fn diff_fixture_pins_the_legacy_diff_envelope() {
    assert!(DIFF.starts_with("[DCTX_LLM_CONTEXT_DIFF_V1.1]\n"));
    assert!(DIFF.contains("Version Change: V1 -> V1.1\n"));
    assert!(DIFF.contains("[ADDED] added.txt\n[MODIFIED] changed.txt\n"));
    assert!(DIFF.contains("[REMOVED] deleted_file.txt\n[ADDED] new_dir/\n[MODIFIED] icon.svg\n"));
    let glued = "  [F] unchanged.txt (ID:F005)</UPDATED_DIRECTORY_TREE>\n";
    assert!(DIFF.contains(glued), "legacy diff glues the closing tag to the last tree line");
    let stamped = "  [F] unchanged.txt (ID:F005, MOD:2400, SIZE:11)</UPDATED_DIRECTORY_TREE>\n";
    assert!(DIFF_STAMPED.contains(stamped), "the stamped diff keeps the 1.0.0 line");
    assert!(!DIFF.contains("MOD:") && !DIFF.contains("SIZE:"));
    assert!(DIFF.contains("[SVG file, content omitted (omit_svg_content=true)]"));
    assert!(DIFF.contains("NEW!"));
    assert!(DIFF.contains("New content"));
    assert!(!DIFF.contains("Old content"));
    assert!(DIFF.contains("unchanged.txt"));
    assert!(DIFF.ends_with('\n'));
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
        diff.entries.push(DiffEntry { change_type, node_type, relative_path: path.to_string() });
    }
    (root, diff)
}

fn render_diff(template: &Template) -> String {
    let (root, diff) = diff_tree();
    render_legacy_diff(template, &root, &diff, &Version::new_v1(), &Version { major: 1, minor: 1 }, true)
}

#[test]
fn diff_envelope_matches_legacy_bytes() {
    assert_eq!(render_diff(&template()), DIFF);
}

#[test]
fn stamped_diff_envelope_matches_the_1_0_0_bytes() {
    assert_eq!(render_diff(&stamped_template()), DIFF_STAMPED);
}


#[test]
fn svg_snapshot_sentence_matches_legacy() {
    let mut root = MarkerNode::dir("", 1);
    root.children.push(binary("icon.svg", 6, 2, "SVG file"));
    let got = render_legacy_snapshot(&template(), &root, &Version::new_v1(), false, true);
    assert!(got.contains("[SVG file (omit_svg_content=true), content omitted]"));
    assert!(!got.contains("<svg/>"));
}

#[test]
fn compiled_bytes_match_sorted_fixture_and_accept_legacy_order() {
    let (sorted, unsorted) = hex_sections();
    let compiled = Template::parse(PIN).compile("pin").expect("compile");
    let mut encoded = Vec::new();
    compiled.serialize(&mut encoded).expect("serialize");
    assert_eq!(encoded, sorted);
    let loaded = CompiledTemplate::deserialize(&mut Cursor::new(&sorted)).expect("sorted");
    let legacy = CompiledTemplate::deserialize(&mut Cursor::new(&unsorted)).expect("unsorted");
    assert_eq!(loaded, compiled);
    assert_eq!(legacy, compiled);
    let mut again = Vec::new();
    legacy.serialize(&mut again).expect("re-encode");
    assert_eq!(again, sorted);
    assert_ne!(unsorted, sorted);
}

#[test]
fn malformed_compiled_bytes_are_refused() {
    let (sorted, _) = hex_sections();
    let ok = CompiledTemplate::deserialize(&mut Cursor::new(&sorted)).expect("valid bytes decode");
    assert_eq!(ok.name, "pin");
    assert!(CompiledTemplate::deserialize(&mut Cursor::new(&b""[..])).is_err());
    assert!(CompiledTemplate::deserialize(&mut Cursor::new(&b"DCTXTMPX"[..])).is_err());
    assert!(CompiledTemplate::deserialize(&mut Cursor::new(&sorted[..10])).is_err());
    let mut bad_version = sorted.clone();
    bad_version[8] = 99;
    assert!(CompiledTemplate::deserialize(&mut Cursor::new(&bad_version)).is_err());
    let mut bad_utf8 = b"DCTXTMPL".to_vec();
    bad_utf8.extend_from_slice(&1u32.to_le_bytes());
    bad_utf8.extend_from_slice(&1u32.to_le_bytes());
    bad_utf8.push(0xFF);
    assert!(CompiledTemplate::deserialize(&mut Cursor::new(&bad_utf8)).is_err());
}
