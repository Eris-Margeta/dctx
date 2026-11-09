// SPDX-License-Identifier: Apache-2.0
//! Refusals and the neighboring case that is accepted.

use dctx_core::FileHandlingDecision;
use dctx_template::{
    GrantRule, HostCeiling, KnownPlugin, PluginState, Selection, Template, TemplateError,
    authorize_path, bundled_git_plugin, compile, narrow_decision,
};

fn v2(body: &str) -> String {
    format!("[DTS]\nversion = 2\n{body}")
}

fn skeleton(extra: &str) -> String {
    v2(&format!(
        r#"
[SELECT:s]
roots = ["src"]
[INDEX:s]
parser = "opaque"
[BUDGET:b]
records = 4
[VIEW:pack]
selection = "s"
budget = "b"
{extra}
"#
    ))
}

fn plugin(id: &str, version: &str) -> KnownPlugin {
    let mut known = KnownPlugin::named(id, version);
    known.hooks.push("before-pack-output-written".to_string());
    known.capabilities.push("file.read:.gitignore".to_string());
    known
}

#[test]
fn unresolved_variable_is_refused_and_a_defined_marker_compiles() {
    let missing = "\
[VARIABLES]
defined = \"yes\"
[LAYOUT]
{{undefined_var}}
";
    let err = compile(missing, &HostCeiling::base()).expect_err("missing");
    assert!(matches!(err, TemplateError::UnresolvedVariable { .. }));
    let present = "\
[VARIABLES]
defined = \"yes\"
[LAYOUT]
{{defined}}
";
    assert!(compile(present, &HostCeiling::base()).is_ok());
}

#[test]
fn variable_cycles_are_refused_and_a_chain_resolves() {
    let cyclic = "\
[VARIABLES]
a = \"{{b}}\"
b = \"{{a}}\"
[LAYOUT]
{{a}}
";
    let template = Template::parse(cyclic);
    assert!(
        template
            .validate()
            .iter()
            .any(|err| matches!(err, dctx_template::TemplateValidationError::Cycle(_)))
    );
    let err = compile(cyclic, &HostCeiling::base()).expect_err("cycle");
    assert!(matches!(
        err,
        TemplateError::Cycle {
            kind: "variable",
            ..
        }
    ));
    let chain = Template::parse(
        "\
[VARIABLES]
a = \"A\"
b = \"{{a}}B\"
c = \"{{b}}C\"
[LAYOUT]
{{c}}
",
    );
    assert_eq!(chain.variables.get("c").map(String::as_str), Some("ABC"));
    assert_eq!(chain.cycles, [] as [std::string::String; 0]);
}

#[test]
fn expressions_are_not_evaluated() {
    let expr = "\
[VARIABLES]
[LAYOUT]
{{1+1}}
";
    assert!(compile(expr, &HostCeiling::base()).is_err());
    let named = Template::parse(
        "\
[VARIABLES]
name = \"2\"
[LAYOUT]
{{name}}
",
    );
    assert_eq!(named.compile("t").expect("name").layout, "2");
    let literal = Template::parse(
        "\
[VARIABLES]
expr = \"1+1\"
[LAYOUT]
{{expr}}
",
    );
    assert_eq!(literal.compile("t").expect("expr").layout, "1+1");
}

#[test]
fn unknown_parser_and_fact_are_refused() {
    let host = HostCeiling::base();
    let tree = skeleton("").replace("parser = \"opaque\"", "parser = \"tree-sitter\"");
    let err = compile(&tree, &host).expect_err("tree-sitter");
    assert!(
        matches!(err, TemplateError::UnknownCapability { name } if name.contains("tree-sitter"))
    );
    let mut granted = host.clone();
    granted
        .capabilities
        .insert("parser:tree-sitter".to_string());
    assert!(compile(&tree, &granted).is_ok());
    let fact = skeleton("").replace(
        "parser = \"opaque\"",
        "facts = [\"bytecode\"]\nparser = \"opaque\"",
    );
    let err = compile(&fact, &host).expect_err("fact");
    assert!(matches!(err, TemplateError::UnknownCapability { name } if name.contains("bytecode")));
    let symbols = skeleton("").replace(
        "parser = \"opaque\"",
        "facts = [\"symbols\"]\nparser = \"opaque\"",
    );
    assert!(compile(&symbols, &host).is_ok());
}

#[test]
fn budgets_above_the_host_ceiling_are_refused() {
    let mut host = HostCeiling::base();
    host.max_hard_estimated_tokens = 2000;
    host.max_estimated_tokens = 2000;
    let over = skeleton("").replace(
        "[BUDGET:b]\nrecords = 4",
        "[BUDGET:b]\nhard_estimated_tokens = 2001\nrecords = 4",
    );
    let err = compile(&over, &host).expect_err("over");
    assert!(matches!(
        err,
        TemplateError::BudgetAboveCeiling {
            field: "hard_estimated_tokens",
            requested: 2001,
            ceiling: 2000
        }
    ));
    let exact = over.replace("2001", "2000");
    assert!(compile(&exact, &host).is_ok());

    host.max_bytes = 100;
    let bytes = skeleton("").replace("records = 4", "bytes = 101\nrecords = 4");
    let err = compile(&bytes, &host).expect_err("bytes");
    assert!(matches!(
        err,
        TemplateError::BudgetAboveCeiling {
            field: "bytes",
            requested: 101,
            ..
        }
    ));
    assert!(compile(&bytes.replace("101", "100"), &host).is_ok());
}

#[test]
fn version_is_not_inferred_and_bad_inputs_are_refused() {
    let select_only = "\
[SELECT:engineering]
roots = [\"src\"]
";
    let err = compile(select_only, &HostCeiling::base()).expect_err("v1");
    assert!(matches!(err, TemplateError::EmptySection { .. }));
    let declared = skeleton("");
    let plan = compile(&declared, &HostCeiling::base()).expect("v2");
    assert_eq!(plan.source_version, 2);
    assert_eq!(plan.selections[0].roots, vec!["src".to_string()]);

    let variable = "\
[VARIABLES]
version = 2
[LAYOUT]
ok
";
    assert_eq!(
        compile(variable, &HostCeiling::base())
            .expect("var")
            .source_version,
        1
    );
    let missing = compile("[DTS]\n[LAYOUT]\nok\n", &HostCeiling::base()).expect_err("dts");
    assert!(
        matches!(missing, TemplateError::InvalidSyntax { detail, .. } if detail.contains("version"))
    );
    let future = compile("[DTS]\nversion = 9\n", &HostCeiling::base()).expect_err("version");
    assert!(matches!(
        future,
        TemplateError::UnsupportedVersion { found: 9 }
    ));
    let nul = compile("ok\0", &HostCeiling::base()).expect_err("nul");
    assert!(matches!(nul, TemplateError::InvalidSyntax { detail, .. } if detail.contains("nul")));
    assert!(compile("[LAYOUT]\nok\n", &HostCeiling::base()).is_ok());
}

#[test]
fn host_size_caps_are_refused_when_narrowed_below_the_plan() {
    let source = skeleton("[LAYOUT]\nhello {{{comp.tree}}}\n");
    let mut host = HostCeiling::base();
    host.max_sections = 6;
    assert!(matches!(
        compile(&source, &host),
        Err(TemplateError::Limit {
            what: "sections",
            ..
        })
    ));
    host.max_sections = 64;
    host.max_plan_bytes = 32;
    assert!(matches!(
        compile(&source, &host),
        Err(TemplateError::Limit { what: "plan", .. })
    ));
    host.max_plan_bytes = dctx_template::HostCeiling::base().max_plan_bytes;
    host.max_instructions = 1;
    assert!(matches!(
        compile(&source, &host),
        Err(TemplateError::Limit {
            what: "instructions",
            ..
        })
    ));
    host.max_instructions = 10_000;
    assert!(compile(&source, &host).is_ok());
}

#[test]
fn permissions_only_narrow() {
    let empty = Selection {
        name: String::new(),
        roots: Vec::new(),
        languages: Vec::new(),
        kinds: Vec::new(),
        rules: Vec::new(),
    };
    let mut host = HostCeiling::base();
    let err = authorize_path(&host, &empty, "src/main.rs").expect_err("deny");
    assert!(matches!(
        err,
        TemplateError::GrantExpansion { kind: "decision" }
    ));
    host.grants.push(GrantRule {
        pattern: "src/**".to_string(),
        decision: FileHandlingDecision::Include,
    });
    assert_eq!(
        authorize_path(&host, &empty, "src/main.rs").expect("allow"),
        FileHandlingDecision::Include
    );
    assert!(matches!(
        narrow_decision(FileHandlingDecision::Include, FileHandlingDecision::Exclude),
        Ok(FileHandlingDecision::Exclude)
    ));
    assert!(matches!(
        narrow_decision(FileHandlingDecision::Exclude, FileHandlingDecision::Include),
        Err(TemplateError::GrantExpansion { kind: "decision" })
    ));
    assert!(matches!(
        narrow_decision(
            FileHandlingDecision::OmitContent,
            FileHandlingDecision::Include
        ),
        Err(TemplateError::GrantExpansion { kind: "decision" })
    ));
    assert!(matches!(
        narrow_decision(
            FileHandlingDecision::Include,
            FileHandlingDecision::OmitContent
        ),
        Ok(FileHandlingDecision::OmitContent)
    ));
    assert!(matches!(
        narrow_decision(
            FileHandlingDecision::DebugVisible,
            FileHandlingDecision::OmitContent
        ),
        Err(TemplateError::GrantExpansion { kind: "decision" })
    ));
    assert!(matches!(
        narrow_decision(
            FileHandlingDecision::OmitContent,
            FileHandlingDecision::DebugVisible
        ),
        Ok(FileHandlingDecision::DebugVisible)
    ));
}

#[test]
fn partial_cycles_are_refused_and_a_real_include_compiles() {
    let cycle = skeleton(
        "\
[PARTIAL:A]
{{>B}}
[PARTIAL:B]
{{>A}}
[LAYOUT]
{{>A}}
",
    );
    let err = compile(&cycle, &HostCeiling::base()).expect_err("cycle");
    assert!(matches!(
        err,
        TemplateError::Cycle {
            kind: "partial",
            ..
        }
    ));
    let ok = skeleton(
        "\
[PARTIAL:A]
from-a
[LAYOUT]
{{>A}}
",
    );
    assert!(compile(&ok, &HostCeiling::base()).is_ok());
    let unknown = skeleton(
        "\
[LAYOUT]
{{{not_a_field}}}
",
    );
    assert!(matches!(
        compile(&unknown, &HostCeiling::base()),
        Err(TemplateError::UnresolvedVariable { .. })
    ));
}

#[test]
fn plugin_declarations_narrow_the_catalog() {
    let mut host = HostCeiling::base();
    host.plugins.push(bundled_git_plugin());
    let good = skeleton(concat!(
        r#"
[PLUGIN:dctx/git]
version = ""#,
        env!("CARGO_PKG_VERSION"),
        r#""
hooks = ["before-pack-output-written"]
required = true
allow = ["file.read:.gitignore", "file.write:.gitignore", "program:git.add-ignore", "resource:git-admin:stage-ignore"]
config.stage = true
"#
    ));
    let plan = compile(&good, &host).expect("git");
    assert_eq!(plan.plugins.plugins[0].state, PluginState::Enabled);
    assert_eq!(plan.plugins.plugins[0].alias, "git");
    assert_eq!(plan.plugins.order, vec!["dctx/git".to_string()]);

    let subset = good.replace(
        "allow = [\"file.read:.gitignore\", \"file.write:.gitignore\", \"program:git.add-ignore\", \"resource:git-admin:stage-ignore\"]",
        "allow = [\"file.read:.gitignore\"]",
    );
    assert!(compile(&subset, &host).is_ok());
    let extra = good.replace("file.read:.gitignore", "file.read:secret");
    assert!(matches!(
        compile(&extra, &host),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
    let nope = good.replace("config.stage = true", "config.nope = true");
    assert!(matches!(
        compile(&nope, &host),
        Err(TemplateError::UnknownField { .. })
    ));
    let hook = good.replace("before-pack-output-written", "not-a-hook");
    assert!(matches!(
        compile(&hook, &host),
        Err(TemplateError::UnknownCapability { .. })
    ));

    let missing = skeleton(concat!(
        "[PLUGIN:dctx/git]\nversion = \"",
        env!("CARGO_PKG_VERSION"),
        "\"\nrequired = true\n"
    ));
    assert!(matches!(
        compile(&missing, &HostCeiling::base()),
        Err(TemplateError::RequiredPluginMissing { .. })
    ));
    let optional = skeleton("[PLUGIN:dctx/git]\nrequired = false\n");
    let unavailable = compile(&optional, &HostCeiling::base()).expect("optional");
    assert_eq!(
        unavailable.plugins.plugins[0].state,
        PluginState::Unavailable
    );

    host.disabled_plugins.insert("dctx/git".to_string());
    assert!(matches!(
        compile(&good, &host),
        Err(TemplateError::UserDisabled { .. })
    ));
    let optional_off = good.replace("required = true", "required = false");
    let disabled = compile(&optional_off, &host).expect("disabled");
    assert_eq!(disabled.plugins.plugins[0].state, PluginState::Disabled);

    let reserved = good.replace("required = true", "required = true\nalias = \"pack\"");
    host.disabled_plugins.clear();
    assert!(matches!(
        compile(&reserved, &host),
        Err(TemplateError::ReservedAlias { .. })
    ));
    let aliased = good.replace("required = true", "required = true\nalias = \"git\"");
    assert!(compile(&aliased, &host).is_ok());

    let bad_version = good.replace(env!("CARGO_PKG_VERSION"), "9.9.9");
    let err = compile(&bad_version, &host).expect_err("version");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("version"))
    );
    let mut pinned = bundled_git_plugin();
    pinned.digest = Some([0x11; 32]);
    host.plugins = vec![pinned];
    let digest = good.replace(
        "required = true",
        "required = true\ndigest = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
    );
    let err = compile(&digest, &host).expect_err("digest");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("plugin digest"))
    );
}

#[test]
fn plugin_aliases_orders_and_network_are_checked() {
    let mut host = HostCeiling::base();
    host.plugins.push(plugin("ex/a", "1.0.0"));
    host.plugins.push(plugin("ex/b", "1.0.0"));
    let dup = skeleton(
        r#"
[PLUGIN:ex/a]
version = "1.0.0"
alias = "tool"
[PLUGIN:ex/b]
version = "1.0.0"
alias = "tool"
"#,
    );
    assert!(matches!(
        compile(&dup, &host),
        Err(TemplateError::Duplicate { kind: "alias", .. })
    ));

    let cycle = skeleton(
        r#"
[PLUGIN:ex/a]
version = "1.0.0"
before = ["ex/b"]
[PLUGIN:ex/b]
version = "1.0.0"
before = ["ex/a"]
"#,
    );
    assert!(matches!(
        compile(&cycle, &host),
        Err(TemplateError::Cycle { kind: "plugin", .. })
    ));

    let after = skeleton(
        r#"
[PLUGIN:ex/a]
version = "1.0.0"
after = ["ex/b"]
[PLUGIN:ex/b]
version = "1.0.0"
"#,
    );
    let plan = compile(&after, &host).expect("after");
    assert_eq!(
        plan.plugins.order,
        vec!["ex/a".to_string(), "ex/b".to_string()]
    );

    let ordered = skeleton(
        r#"
[PLUGINS:g]
order = ["ex/a", "ex/b"]
enabled = ["ex/a", "ex/b"]
[PLUGIN:ex/a]
version = "1.0.0"
[PLUGIN:ex/b]
version = "1.0.0"
"#,
    );
    let plan = compile(&ordered, &host).expect("order");
    assert_eq!(
        plan.plugins.order,
        vec!["ex/a".to_string(), "ex/b".to_string()]
    );

    host.plugins.push(bundled_git_plugin());
    let external = skeleton(concat!(
        r#"
[PLUGINS:main]
network = "external"
enabled = ["dctx/git"]
[PLUGIN:dctx/git]
version = ""#,
        env!("CARGO_PKG_VERSION"),
        r#""
required = true
"#
    ));
    assert!(matches!(
        compile(&external, &host),
        Err(TemplateError::GrantExpansion { kind: "network" })
    ));
    let none = external.replace("external", "none");
    assert!(compile(&none, &host).is_ok());
}
