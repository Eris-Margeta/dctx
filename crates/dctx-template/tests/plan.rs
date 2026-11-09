// SPDX-License-Identifier: Apache-2.0
//! V2 `ContextPlan` compile, canonical bytes and domain digests.

use dctx_core::FileHandlingDecision;
use dctx_template::{HostCeiling, PluginState, bundled_git_plugin, compile, from_bytes, to_bytes};

const ENGINEERING: &str = r#"
[DTS]
version = 2

[SELECT:engineering]
roots = ["src", "crates", "docs"]
languages = ["rust", "typescript", "markdown"]
deny = ["**/.env*", "**/credentials/**"]

[INDEX:engineering]
facts = ["symbols", "imports", "calls", "headings"]
parser = "structural"

[BUDGET:agent]
estimated_tokens = 500
hard_estimated_tokens = 2000
records = 40
depth = 2

[VIEW:map]
selection = "engineering"
budget = "agent"
layout = "AGENT_LINES"

[VIEW:pack]
selection = "engineering"
budget = "agent"
layout = "SNAPSHOT"
"#;

#[test]
fn engineering_plan_compiles_and_roundtrips() {
    let host = HostCeiling::base();
    let plan = compile(ENGINEERING, &host).expect("compile");
    assert_eq!(plan.source_version, 2);
    assert_eq!(plan.selections[0].name, "engineering");
    assert_eq!(plan.selections[0].roots, vec!["src", "crates", "docs"]);
    assert!(plan.selections[0].rules.iter().any(|rule| {
        rule.decision == FileHandlingDecision::Exclude && rule.pattern == "**/.env*"
    }));
    assert_eq!(plan.indexes[0].parser, "structural");
    assert_eq!(
        plan.indexes[0].facts,
        vec!["symbols", "imports", "calls", "headings"]
    );
    assert_eq!(plan.budgets[0].estimated_tokens, 500);
    assert_eq!(plan.budgets[0].hard_estimated_tokens, 2000);
    assert_eq!(plan.budgets[0].records, 40);
    assert_eq!(plan.budgets[0].depth, 2);
    assert_eq!(plan.views.len(), 2);
    assert_eq!(plan.views[0].name, "map");
    assert_eq!(plan.views[0].layout_label, "AGENT_LINES");
    assert_eq!(plan.views[1].layout_label, "SNAPSHOT");
    let bytes = to_bytes(&plan).expect("encode");
    assert_eq!(from_bytes(&bytes).expect("decode"), plan);
    let again = compile(ENGINEERING, &host).expect("again");
    assert_eq!(to_bytes(&again).expect("encode"), bytes);
    assert_eq!(again.selection_digest, plan.selection_digest);
    assert_ne!(plan.selection_digest, plan.extraction_digest);
}

#[test]
fn reordered_sections_are_byte_identical_and_rule_order_is_not() {
    let host = HostCeiling::base();
    let first = "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
include = [\"src/a.rs\"]
deny = [\"src/b.rs\"]
[BUDGET:b]
records = 4
[INDEX:s]
parser = \"opaque\"
[VIEW:pack]
selection = \"s\"
budget = \"b\"
";
    let reordered = "\
[DTS]
version = 2
[BUDGET:b]
records = 4
[VIEW:pack]
selection = \"s\"
budget = \"b\"
[INDEX:s]
parser = \"opaque\"
[SELECT:s]
roots = [\"src\"]
include = [\"src/a.rs\"]
deny = [\"src/b.rs\"]
";
    let left = compile(first, &host).expect("first");
    let right = compile(reordered, &host).expect("reordered");
    assert_eq!(
        to_bytes(&left).expect("left"),
        to_bytes(&right).expect("right")
    );

    let swapped = first.replace(
        "include = [\"src/a.rs\"]\ndeny = [\"src/b.rs\"]",
        "deny = [\"src/b.rs\"]\ninclude = [\"src/a.rs\"]",
    );
    let other = compile(&swapped, &host).expect("swapped");
    assert_ne!(left.selection_digest, other.selection_digest);
}

#[test]
fn render_edit_and_plugin_edit_touch_only_their_digests() {
    let host = HostCeiling::base();
    let bare = compile(ENGINEERING, &host).expect("bare");
    let mut with_layout = ENGINEERING.to_string();
    with_layout.push_str("\n[LAYOUT]\nhello {{{comp.tree}}}\n");
    let rendered = compile(&with_layout, &host).expect("layout");
    assert_eq!(bare.selection_digest, rendered.selection_digest);
    assert_eq!(bare.extraction_digest, rendered.extraction_digest);
    assert_ne!(bare.render_digest, rendered.render_digest);
    assert_ne!(bare.plan_digest, rendered.plan_digest);

    let mut plugin_host = HostCeiling::base();
    plugin_host.plugins.push(bundled_git_plugin());
    let plugin = "\
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
[PLUGIN:dctx/git]
version = \"1.3.0\"
hooks = [\"before-pack-output-written\"]
required = true
allow = [\"file.read:.gitignore\"]
config.stage = true
";
    let enabled = compile(plugin, &plugin_host).expect("plugin");
    assert_eq!(enabled.plugins.plugins[0].state, PluginState::Enabled);
    assert_eq!(enabled.plugins.order, vec!["dctx/git".to_string()]);
    let quiet = plugin.replace("config.stage = true", "config.stage = false");
    let disabled_stage = compile(&quiet, &plugin_host).expect("stage");
    assert_eq!(enabled.selection_digest, disabled_stage.selection_digest);
    assert_eq!(enabled.extraction_digest, disabled_stage.extraction_digest);
    assert_ne!(enabled.plugin_digest, disabled_stage.plugin_digest);
    assert_ne!(enabled.plan_digest, disabled_stage.plan_digest);
}
