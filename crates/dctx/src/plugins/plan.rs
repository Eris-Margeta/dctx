// SPDX-License-Identifier: Apache-2.0
//! Map a compiled DTS plugin plan onto host grants, selection and hook order.
//! Implements PLUGINS.md §3.5 and §4.1. The template crate remains the planner.
//! Depends on `dctx-template` and `dctx-plugin-host`. [`super::boundary`] executes the result.
//! Invalid capability rows are dropped, which can only narrow the grant.

use dctx_plugin_host::{
    Authority, Binding, Decision, GrantLayers, HookEntry, HostRefusal, Origin, Selection,
};
use dctx_plugin_protocol::records::capability::{Capability as HostCap, OutputOperation};
use dctx_plugin_protocol::records::scalars::{Digest, PluginId, ProjectPath, Role, Token};
use dctx_plugin_protocol::{ErrorCode, Phase};
use dctx_template::{Capability, PluginBinding, PluginOrigin, PluginPlan, Resolution};

use super::ports::bad_token;
use super::{Boundary, PluginHost};

/// Enabled plugins on `boundary`, in the plan's order.
pub(crate) fn participants(plan: &PluginPlan, boundary: Boundary) -> Vec<&PluginBinding> {
    let hook = boundary.as_str();
    plan.order
        .iter()
        .filter_map(|id| plan.plugins.iter().find(|plugin| &plugin.id == id))
        .filter(|plugin| plugin.state == dctx_template::PluginState::Enabled)
        .filter(|plugin| {
            plugin
                .hooks
                .iter()
                .any(|decl| decl.name == hook && decl.version == 1)
        })
        .collect()
}

pub(crate) fn entries(plugins: &[&PluginBinding]) -> Result<Vec<HookEntry>, HostRefusal> {
    plugins
        .iter()
        .map(|plugin| {
            Ok(HookEntry {
                plugin: plugin_id(&plugin.id)?,
                required: plugin.required,
            })
        })
        .collect()
}

pub(crate) fn edges(plugins: &[&PluginBinding]) -> Result<Vec<(PluginId, PluginId)>, HostRefusal> {
    let mut out = Vec::new();
    for pair in plugins.windows(2) {
        out.push((plugin_id(&pair[0].id)?, plugin_id(&pair[1].id)?));
    }
    Ok(out)
}

pub(crate) type PolicySnapshot = (Binding, GrantLayers, Selection);
pub(crate) type AuthoritySource =
    dyn Fn(&str, &Token, u64) -> Result<PolicySnapshot, HostRefusal> + Send + Sync;

pub(crate) fn authority(
    host: &PluginHost,
    plugin: &PluginBinding,
    invocation: &Token,
    included: bool,
    epoch: u64,
) -> Result<Authority, HostRefusal> {
    let (binding, mut layers, selected) = (host.authority)(&plugin.id, invocation, epoch)?;
    let digest = plugin.digest.ok_or(HostRefusal::new(
        ErrorCode::IntegrityFailure,
        Phase::Verify,
        "artifact-digest",
    ))?;
    if binding.artifact != digest_text(digest)? {
        return Err(HostRefusal::new(
            ErrorCode::IntegrityFailure,
            Phase::Verify,
            "artifact-mismatch",
        ));
    }
    if binding.plugin != plugin_id(&plugin.id)? || &binding.invocation != invocation {
        return Err(HostRefusal::denied("binding-mismatch"));
    }
    if !host.confined() {
        layers.adapter.clear();
    }
    let selected = if included {
        selected
    } else {
        Selection::default()
    };
    let policy_epoch = binding.generation;
    let mut authority = Authority::new(layers, selected, binding, policy_epoch);
    authority.configure(&wire_caps(plugin));
    authority.initialize()?;
    Ok(authority)
}

pub(crate) fn fixture_authority(
    plugin: &str,
    invocation: &Token,
    epoch: u64,
    confined: bool,
) -> Result<PolicySnapshot, HostRefusal> {
    let fixture = dctx_template::bundled_git_plugin();
    if plugin != fixture.id {
        return Err(HostRefusal::denied("authority-missing"));
    }
    let digest = fixture.digest.ok_or(bad_token())?;
    let binding = Binding {
        plugin: plugin_id(plugin)?,
        artifact: digest_text(digest)?,
        invocation: invocation.clone(),
        root: Token::new("fixture:root").map_err(|_| bad_token())?,
        session: Token::new("fixture:session").map_err(|_| bad_token())?,
        namespace: Token::new("fixture:git").map_err(|_| bad_token())?,
        generation: epoch,
    };
    let caps: Vec<_> = fixture.records.iter().filter_map(wire_cap).collect();
    let layers = if confined {
        GrantLayers {
            manifest: caps.clone(),
            consent: caps.clone(),
            organization: caps.clone(),
            host: caps.clone(),
            adapter: caps,
            denies: Vec::new(),
        }
    } else {
        GrantLayers::default()
    };
    Ok((binding, layers, selection(true)?))
}

pub(crate) fn origin_of(origin: Option<PluginOrigin>) -> Origin {
    match origin {
        Some(PluginOrigin::Bundled) => Origin::BundledGit,
        Some(PluginOrigin::Override) => Origin::OfficialOverride,
        Some(PluginOrigin::Installed) | None => Origin::ThirdParty,
    }
}

pub(crate) fn launch_kind(origin: Origin) -> dctx_plugin_platform::LaunchKind {
    match origin {
        Origin::BundledGit => dctx_plugin_platform::LaunchKind::BundledGit,
        Origin::ThirdParty | Origin::OfficialOverride => {
            dctx_plugin_platform::LaunchKind::ThirdParty
        }
    }
}

pub(crate) fn selection(included: bool) -> Result<Selection, HostRefusal> {
    let path = ProjectPath::new(".gitignore").map_err(|_| bad_token())?;
    let decision = if included {
        Decision::Include
    } else {
        Decision::Exclude
    };
    Ok(Selection::new([(path, decision)]))
}

pub(crate) fn verified(plugin: &PluginBinding) -> bool {
    plugin.digest.is_some() && matches!(plugin.resolution, Resolution::Available)
}

fn wire_caps(plugin: &PluginBinding) -> Vec<HostCap> {
    plugin.allow.iter().filter_map(wire_cap).collect()
}

fn wire_cap(cap: &Capability) -> Option<HostCap> {
    match cap {
        Capability::FileRead { path, max_bytes } => Some(HostCap::FileRead {
            path: ProjectPath::new(path).ok()?,
            max_bytes: *max_bytes,
        }),
        Capability::FileWrite {
            path,
            create,
            max_growth_bytes,
            max_bytes,
        } => Some(HostCap::FileWrite {
            path: ProjectPath::new(path).ok()?,
            create: *create,
            max_growth_bytes: *max_growth_bytes,
            max_bytes: *max_bytes,
        }),
        Capability::Program {
            operation,
            resources,
            max_output_bytes,
        } => {
            if operation != "git.add-ignore"
                || !resources
                    .iter()
                    .any(|item| item == "git-admin:stage-ignore")
            {
                return None;
            }
            let max_output_bytes = (*max_output_bytes).clamp(1, 65_536);
            Some(HostCap::GitAddIgnore { max_output_bytes })
        }
        Capability::OutputsRead {
            roles,
            operations,
            max_bytes,
        } => {
            let roles = roles
                .iter()
                .filter_map(|role| Role::new(role).ok())
                .collect::<Vec<_>>();
            let operations = operations
                .iter()
                .filter_map(|op| match op.as_str() {
                    "pack" => Some(OutputOperation::Pack),
                    "index" => Some(OutputOperation::Index),
                    _ => None,
                })
                .collect::<Vec<_>>();
            if roles.is_empty() || operations.is_empty() {
                return None;
            }
            Some(HostCap::OutputsRead {
                roles,
                operations,
                max_bytes: *max_bytes,
            })
        }
        Capability::State {
            read,
            write,
            reclaim,
            max_bytes,
            source_content,
        } => Some(HostCap::State {
            read: *read,
            write: *write,
            reclaim: *reclaim,
            max_bytes: *max_bytes,
            source_content: *source_content,
        }),
    }
}

fn plugin_id(text: &str) -> Result<PluginId, HostRefusal> {
    PluginId::new(text)
        .map_err(|_| HostRefusal::new(ErrorCode::InvalidConfig, Phase::Resolve, "plugin-id"))
}

fn digest_text(bytes: [u8; 32]) -> Result<Digest, HostRefusal> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("sha256:");
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Digest::new(&text).map_err(|_| {
        HostRefusal::new(
            ErrorCode::IntegrityFailure,
            Phase::Verify,
            "artifact-digest",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_template::{HostCeiling, KnownPlugin, bundled_git_plugin, compile};

    fn plan(extra: &str) -> PluginPlan {
        let mut host = HostCeiling::base();
        let mut plugin = bundled_git_plugin();
        plugin.hooks = vec![
            "before-pack-output-written".to_string(),
            "cache-gc".to_string(),
        ];
        host.plugins.push(plugin);
        let mut other = KnownPlugin::named("ex/other", "1.0.0");
        other.hooks.push("before-pack-output-written".to_string());
        host.plugins.push(other);
        let source = format!(
            "\
[DTS]
version = 2
[SELECT:s]
roots = [\"src\"]
[INDEX:s]
parser = \"structural\"
[BUDGET:b]
records = 4
[VIEW:map]
selection = \"s\"
index = \"s\"
budget = \"b\"
{extra}"
        );
        compile(&source, &host).expect("plan").plugins
    }

    #[test]
    fn participants_follow_plan_order_and_skip_undeclared_hooks() {
        let compiled = plan(
            "\
[PLUGIN:ex/other]
version = \"1.0.0\"
hooks = [\"before-pack-output-written\"]
[PLUGIN:dctx/git]
version = \"1.2.0\"
digest = \"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\"
hooks = [\"before-pack-output-written\", \"cache-gc\"]
",
        );
        let before: Vec<&str> = participants(&compiled, Boundary::BeforePack)
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect();
        assert_eq!(before, ["dctx/git", "ex/other"]);
        let gc: Vec<&str> = participants(&compiled, Boundary::CacheGc)
            .iter()
            .map(|plugin| plugin.id.as_str())
            .collect();
        assert_eq!(gc, ["dctx/git"]);
        assert_eq!(
            participants(&compiled, Boundary::AfterIndex),
            [] as [&dctx_template::PluginBinding; 0]
        );
    }

    #[test]
    fn unknown_program_is_not_granted() {
        let cap = Capability::Program {
            operation: "git.status".to_string(),
            resources: vec!["git-admin:stage-ignore".to_string()],
            max_output_bytes: 10,
        };
        assert!(wire_cap(&cap).is_none());
    }
}
