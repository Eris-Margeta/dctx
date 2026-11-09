// SPDX-License-Identifier: Apache-2.0
//! Plugin plan compile, load and the refusals around a plan that is accepted.

use std::collections::BTreeSet;

use dctx_template::{
    Capability, ConfigEntry, ConfigValue, ContextPlan, FieldKind, FieldSpec, HookDecl, HostCeiling,
    KnownPlugin, Liveness, PluginOrigin, PluginState, Resolution, ResourceLimits, TemplateError,
    UnavailableReason, bundled_git_plugin, compile, from_bytes, load, render_plan, to_bytes,
};

fn v2(extra: &str) -> String {
    format!(
        "\
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
{extra}"
    )
}

fn git_source() -> String {
    v2(
        "\
[PLUGIN:dctx/git]
version = \"1.3.0\"
digest = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"
hooks = [\"before-pack-output-written\"]
required = true
allow = [\"file.read:.gitignore\", \"file.write:.gitignore\", \"program:git.add-ignore\", \"resource:git-admin:stage-ignore\"]
config.stage = true
",
    )
}

fn git_host() -> HostCeiling {
    let mut host = HostCeiling::base();
    host.plugins.push(bundled_git_plugin());
    host
}

fn stage(plan: &ContextPlan) -> Option<&ConfigValue> {
    plan.plugins
        .plugins
        .iter()
        .find(|plugin| plugin.id == "dctx/git")?
        .config
        .iter()
        .find(|entry| entry.key == "stage")
        .map(|entry| &entry.value)
}

#[test]
fn compiled_git_plan_roundtrips_and_load_rechecks_the_host() {
    let host = git_host();
    let catalog = bundled_git_plugin();
    let plan = compile(&git_source(), &host).expect("compile");
    let binding = &plan.plugins.plugins[0];
    assert_eq!(binding.state, PluginState::Enabled);
    assert_eq!(binding.alias, "git");
    assert_eq!(binding.resolution, Resolution::Available);
    assert_eq!(binding.origin, Some(PluginOrigin::Bundled));
    assert_eq!(binding.publisher_key, catalog.publisher_key);
    assert_eq!(binding.digest, catalog.digest);
    assert_eq!(binding.manifest_digest, catalog.manifest_digest);
    assert_eq!(
        binding.hooks,
        vec![HookDecl {
            name: "before-pack-output-written".to_string(),
            version: 1
        }]
    );
    assert!(
        binding
            .allow
            .iter()
            .any(|cap| matches!(cap, Capability::FileRead { path, .. } if path == ".gitignore"))
    );
    assert!(binding.allow.iter().any(
        |cap| matches!(cap, Capability::Program { operation, .. } if operation == "git.add-ignore")
    ));
    assert_eq!(plan.plugins.limits, ResourceLimits::standard());
    assert_eq!(plan.plugins.liveness, Liveness::fail_closed());
    assert_eq!(plan.plugins.order, vec!["dctx/git".to_string()]);

    let bytes = to_bytes(&plan).expect("encode");
    assert_eq!(from_bytes(&bytes).expect("structural"), plan);
    assert_eq!(load(&bytes, &host).expect("policy"), plan);

    let mut denied = host.clone();
    denied.denied_capabilities.insert(
        "*".to_string(),
        BTreeSet::from(["file.write:.gitignore".to_string()]),
    );
    assert!(matches!(
        load(&bytes, &denied),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
    let mut disabled = host.clone();
    disabled.disabled_plugins.insert("dctx/git".to_string());
    assert!(matches!(
        load(&bytes, &disabled),
        Err(TemplateError::UserDisabled { .. })
    ));
    let mut other_key = host.clone();
    other_key.plugins[0].publisher_key = "other-key".to_string();
    assert!(matches!(
        load(&bytes, &other_key),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
    let mut other_manifest = host.clone();
    other_manifest.plugins[0].manifest_digest = Some([0x22; 32]);
    let err = load(&bytes, &other_manifest).expect_err("manifest");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("plugin digest"))
    );
    let mut live = host.clone();
    live.liveness = Liveness::Interactive {
        sample_ms: 1000,
        cancel_grace_ms: 5000,
    };
    assert!(matches!(
        load(&bytes, &live),
        Err(TemplateError::GrantExpansion { kind: "liveness" })
    ));
}

#[test]
fn order_is_deterministic_and_cycles_aliases_and_unknowns_are_refused() {
    let mut host = HostCeiling::base();
    host.plugins.push(KnownPlugin::named("ex/b", "1.0.0"));
    host.plugins.push(KnownPlugin::named("ex/a", "1.0.0"));
    let reverse = v2("\
[PLUGIN:ex/b]
version = \"1.0.0\"
[PLUGIN:ex/a]
version = \"1.0.0\"
");
    let forward = v2("\
[PLUGIN:ex/a]
version = \"1.0.0\"
[PLUGIN:ex/b]
version = \"1.0.0\"
");
    let tied = compile(&reverse, &host).expect("tie");
    assert_eq!(
        tied.plugins.order,
        vec!["ex/a".to_string(), "ex/b".to_string()]
    );
    assert_eq!(
        compile(&forward, &host).expect("forward").plugins.order,
        tied.plugins.order
    );

    let forced = v2("\
[PLUGINS:eng]
network = \"none\"
enabled = [\"ex/a\", \"ex/b\"]
order = [\"ex/b\", \"ex/a\"]
[PLUGIN:ex/a]
version = \"1.0.0\"
alias = \"alpha\"
[PLUGIN:ex/b]
version = \"1.0.0\"
alias = \"beta\"
");
    let ordered = compile(&forced, &host).expect("order");
    assert_eq!(ordered.plugins.organization, "eng");
    assert_eq!(
        ordered.plugins.order,
        vec!["ex/b".to_string(), "ex/a".to_string()]
    );

    let dup = forced.replace("alias = \"beta\"", "alias = \"alpha\"");
    assert!(matches!(
        compile(&dup, &host),
        Err(TemplateError::Duplicate { kind: "alias", .. })
    ));
    let cycle = v2("\
[PLUGIN:ex/a]
version = \"1.0.0\"
before = [\"ex/a\"]
");
    assert!(matches!(
        compile(&cycle, &host),
        Err(TemplateError::Cycle { kind: "plugin", .. })
    ));
    let unknown_ref = v2("\
[PLUGIN:ex/a]
version = \"1.0.0\"
before = [\"ex/missing\"]
");
    assert!(matches!(
        compile(&unknown_ref, &host),
        Err(TemplateError::UnresolvedRef { kind: "plugin", .. })
    ));

    let skipped = v2("\
[PLUGIN:ex/a]
version = \"1.0.0\"
after = [\"ex/gone\"]
[PLUGIN:ex/gone]
required = false
");
    let plan = compile(&skipped, &host).expect("optional predecessor");
    assert_eq!(plan.plugins.order, vec!["ex/a".to_string()]);
    let gone = plan
        .plugins
        .plugins
        .iter()
        .find(|plugin| plugin.id == "ex/gone")
        .expect("gone");
    assert_eq!(gone.state, PluginState::Unavailable);
    assert_eq!(
        gone.resolution,
        Resolution::Unavailable {
            reason: UnavailableReason::NotInstalled
        }
    );
    assert_eq!(gone.allow, [] as [dctx_template::Capability; 0]);
    assert_eq!(gone.hooks, [] as [dctx_template::HookDecl; 0]);
    assert_eq!(gone.config, [] as [dctx_template::ConfigEntry; 0]);
    assert!(gone.digest.is_none());

    let nope = git_source().replace(
        "config.stage = true",
        "config.stage = true\nconfig.nope = true",
    );
    assert!(matches!(
        compile(&nope, &git_host()),
        Err(TemplateError::UnknownField { .. })
    ));
    let token = git_source().replace("config.stage = true", "config.stage = true\ntoken = \"x\"");
    assert!(matches!(
        compile(&token, &git_host()),
        Err(TemplateError::InvalidSyntax { .. })
    ));
    let range = git_source().replace("version = \"1.3.0\"", "version = \"^1.3.0\"");
    let err = compile(&range, &git_host()).expect_err("range");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("version"))
    );
    let unmet = git_source().replace("version = \"1.3.0\"", "version = \"9.9.9\"");
    let err = compile(&unmet, &git_host()).expect_err("unmet");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("version"))
    );
    let bare = git_source().replace("version = \"1.3.0\"", "version = \"1\"");
    let err = compile(&bare, &git_host()).expect_err("not semver");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("version"))
    );
}

#[test]
fn disable_and_the_policy_ceiling_outrank_the_template() {
    let mut host = git_host();
    host.disabled_plugins.insert("dctx/git".to_string());
    assert!(matches!(
        compile(&git_source(), &host),
        Err(TemplateError::UserDisabled { .. })
    ));
    let optional = v2("\
[PLUGINS:eng]
network = \"none\"
enabled = [\"dctx/git\"]
[PLUGIN:dctx/git]
version = \"1.3.0\"
required = false
config.stage = true
");
    let disabled = compile(&optional, &host).expect("sticky disable");
    assert_eq!(disabled.plugins.plugins[0].state, PluginState::Disabled);
    assert_eq!(
        disabled.plugins.plugins[0].allow,
        [] as [dctx_template::Capability; 0]
    );
    assert_eq!(
        disabled.plugins.plugins[0].config,
        [] as [dctx_template::ConfigEntry; 0]
    );
    assert_eq!(disabled.plugins.order, [] as [std::string::String; 0]);
    assert_eq!(
        disabled.plugins.plugins[0].digest,
        bundled_git_plugin().digest
    );

    host.disabled_plugins.clear();
    host.denied_capabilities.insert(
        "dctx/git".to_string(),
        BTreeSet::from(["file.write:.gitignore".to_string()]),
    );
    assert!(matches!(
        compile(&git_source(), &host),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
    let read_only = git_source().replace(
        "allow = [\"file.read:.gitignore\", \"file.write:.gitignore\", \"program:git.add-ignore\", \"resource:git-admin:stage-ignore\"]",
        "allow = [\"file.read:.gitignore\"]",
    );
    let narrowed = compile(&read_only, &host).expect("read only");
    assert!(
        narrowed.plugins.plugins[0]
            .allow
            .iter()
            .all(|cap| matches!(cap, Capability::FileRead { .. }))
    );
    let lone = git_source().replace(
        "allow = [\"file.read:.gitignore\", \"file.write:.gitignore\", \"program:git.add-ignore\", \"resource:git-admin:stage-ignore\"]",
        "allow = [\"program:git.add-ignore\"]",
    );
    host.denied_capabilities.clear();
    assert!(matches!(
        compile(&lone, &host),
        Err(TemplateError::GrantExpansion { kind: "plugin" })
    ));
    assert!(compile(&git_source(), &host).is_ok());
}

#[test]
fn config_precedence_does_not_move_map_facts() {
    let mut host = git_host();
    host.user_config.insert(
        "dctx/git".to_string(),
        vec![ConfigEntry {
            key: "stage".to_string(),
            value: ConfigValue::Bool(false),
        }],
    );
    let project = compile(&git_source(), &host).expect("project wins");
    assert_eq!(stage(&project), Some(&ConfigValue::Bool(true)));
    let user_only =
        compile(&git_source().replace("config.stage = true\n", ""), &host).expect("user");
    assert_eq!(stage(&user_only), Some(&ConfigValue::Bool(false)));
    let defaults = compile(
        &git_source().replace("config.stage = true\n", ""),
        &git_host(),
    )
    .expect("default");
    assert_eq!(stage(&defaults), Some(&ConfigValue::Bool(true)));

    let quiet = compile(
        &git_source().replace("config.stage = true", "config.stage = false"),
        &git_host(),
    )
    .expect("quiet");
    let loud = compile(&git_source(), &git_host()).expect("loud");
    assert_eq!(loud.map_facts(), quiet.map_facts());
    assert_ne!(loud.plugin_digest, quiet.plugin_digest);
    assert_ne!(loud.plan_digest, quiet.plan_digest);

    let interpolated = git_source().replace("config.stage = true", "config.stage = \"${STAGE}\"");
    let err = compile(&interpolated, &git_host()).expect_err("interpolation");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("interpolation"))
    );
    let mut noted = KnownPlugin::named("ex/tool", "1.0.0");
    noted.fields.push(FieldSpec {
        name: "note".to_string(),
        kind: FieldKind::Str { max_bytes: 64 },
        default_value: None,
    });
    let mut tool_host = HostCeiling::base();
    tool_host.plugins.push(noted);
    let path = v2("\
[PLUGIN:ex/tool]
version = \"1.0.0\"
config.note = \"/bin/git\"
");
    let err = compile(&path, &tool_host).expect_err("path");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("executable path"))
    );
    let plain = path.replace("config.note = \"/bin/git\"", "config.note = \"keep\"");
    assert_eq!(
        compile(&plain, &tool_host).expect("note").plugins.plugins[0].config[0].value,
        ConfigValue::Str("keep".to_string())
    );
}

#[test]
fn v1_rendering_ignores_the_distribution_plugin_plan() {
    let source = "\
[VARIABLES]
name = \"demo\"
[LAYOUT]
Hello {{name}}
";
    let bare_host = HostCeiling::base();
    let bare = compile(source, &bare_host).expect("library");
    assert_eq!(
        bare.plugins.plugins,
        [] as [dctx_template::PluginBinding; 0]
    );
    let mut plugin_host = git_host();
    let with = compile(source, &plugin_host).expect("distribution");
    assert_eq!(with.plugins.plugins[0].state, PluginState::Enabled);
    let bare_text = render_plan(&bare, "pack", &[], "demo", &bare_host).expect("bare render");
    let with_text = render_plan(&with, "pack", &[], "demo", &plugin_host).expect("plugin render");
    assert_eq!(bare_text, with_text);
    assert_eq!(bare.map_facts(), with.map_facts());
    assert_ne!(bare.plugin_digest, with.plugin_digest);

    plugin_host.disabled_plugins.insert("dctx/git".to_string());
    let off = compile(source, &plugin_host).expect("user disable");
    assert_eq!(off.plugins.plugins[0].state, PluginState::Disabled);
    assert_eq!(
        off.plugins.plugins[0].allow,
        [] as [dctx_template::Capability; 0]
    );
    assert_eq!(
        render_plan(&off, "pack", &[], "demo", &plugin_host).expect("off"),
        bare_text
    );

    plugin_host.disabled_plugins.clear();
    plugin_host.denied_capabilities.insert(
        "*".to_string(),
        BTreeSet::from(["file.write:.gitignore".to_string()]),
    );
    let narrowed = compile(source, &plugin_host).expect("ceiling");
    assert!(
        narrowed.plugins.plugins[0]
            .allow
            .iter()
            .any(|cap| matches!(cap, Capability::FileRead { .. }))
    );
    assert!(
        narrowed.plugins.plugins[0]
            .allow
            .iter()
            .all(|cap| !matches!(cap, Capability::FileWrite { .. }))
    );
    assert_eq!(
        render_plan(&narrowed, "pack", &[], "demo", &plugin_host).expect("narrowed"),
        bare_text
    );
}

#[test]
fn a_missing_optional_plugin_stays_unavailable_until_the_catalog_grows() {
    let source = v2("\
[PLUGIN:dctx/git]
required = false
");
    let host = HostCeiling::base();
    let plan = compile(&source, &host).expect("optional");
    assert_eq!(plan.plugins.plugins[0].state, PluginState::Unavailable);
    assert_eq!(
        plan.plugins.plugins[0].allow,
        [] as [dctx_template::Capability; 0]
    );
    let bytes = to_bytes(&plan).expect("encode");
    assert_eq!(load(&bytes, &host).expect("still missing"), plan);
    let err = load(&bytes, &git_host()).expect_err("now installed");
    assert!(
        matches!(err, TemplateError::InvalidSyntax { detail, .. } if detail.contains("plugin lock"))
    );
}
