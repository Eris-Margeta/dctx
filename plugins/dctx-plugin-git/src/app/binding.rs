//! Session binding from `initialize`: the fixed v1 config, grants by kind, and the ignore/repository handles.
//! Implements plugin-api git.md "Config cannot expand these fixed v1 values" (G-PROFILE), PLUGINS.md §3.2
//! "a plugin may refuse an unsupported configuration but may not reinterpret it", and per-invocation grant
//! sets (only handles listed by `invoke` at its policy epoch are in force). Used by `app::protect/hook`.

use dctx_plugin_protocol::records::capability::Capability;
use dctx_plugin_protocol::records::common::{Config, ConfigValue, HookName, ResourceKind, Scalar};
use dctx_plugin_protocol::records::requests::Initialize;
use dctx_plugin_protocol::records::scalars::Token;
use dctx_plugin_protocol::{ErrorCode, Phase};

use crate::app::failure::Failure;
use crate::domain::text::MAX_IGNORE_BYTES;

/// Write growth ceiling of the v1 profile.
pub const MAX_GROWTH_BYTES: u64 = 4096;
/// Replacement size ceiling of the v1 profile.
pub const MAX_REPLACEMENT_BYTES: u64 = 1_052_672;

/// Validated plugin config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GitConfig {
    /// Request staging after a committed edit.
    pub stage: bool,
}

/// One bound grant.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bound {
    handle: Token,
    epoch: u64,
    limit: GrantLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GrantLimit {
    Read { max_bytes: u64 },
    Write { max_growth: u64, max_bytes: u64 },
    Program,
}

/// Grants in force for one invocation (ceilings already narrowed to the v1 profile).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    /// `file.read` byte ceiling.
    pub read: Option<u64>,
    /// `file.write` (growth, size) ceilings.
    pub write: Option<(u64, u64)>,
    /// `program.run git.add-ignore` granted.
    pub program: bool,
}

/// A repository handle and whether its layout is the supported ordinary root repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    /// Opaque administrative handle.
    pub handle: Token,
    /// Ordinary repository at the target root (no role, or role `ordinary`).
    pub ordinary: bool,
}

/// Everything `initialize` bound for this session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    grants: Vec<Bound>,
    /// Root `.gitignore` handle (absent when the file does not exist).
    pub ignore: Option<Token>,
    /// Nested rule files the host declared relevant to planned outputs.
    pub nested: Vec<Token>,
    /// Repository bindings.
    pub repositories: Vec<Repository>,
    /// Whether the host offered `program.run`.
    pub program_api: bool,
    /// Session config.
    pub config: GitConfig,
}

fn invalid(reason: &'static str) -> Failure {
    Failure::refused(ErrorCode::InvalidConfig, Phase::Initialize, reason)
}

/// Validates config: known keys only, fixed values, defaults for absent keys.
/// # Errors
/// Refuses unknown keys or values outside the frozen Git configuration profile.
pub fn parse_config(config: &Config) -> Result<GitConfig, Failure> {
    let mut parsed = GitConfig { stage: true };
    for (key, value) in config {
        match (key.as_str(), value) {
            ("stage", ConfigValue::Scalar(Scalar::Bool(stage))) => parsed.stage = *stage,
            ("create_missing", ConfigValue::Scalar(Scalar::Bool(false))) => {}
            ("marker", ConfigValue::Scalar(Scalar::Text(text))) if text.as_str() == "host-version" => {}
            ("stage" | "create_missing" | "marker", _) => return Err(invalid("config-value-outside-profile")),
            _ => return Err(invalid("unknown-config-key")),
        }
    }
    Ok(parsed)
}

/// Binds an initialize request.
/// # Errors
/// Refuses undeclared hooks, unsupported grants, duplicate ignore resources, or invalid configuration.
pub fn bind(init: &Initialize, program_api: bool) -> Result<Binding, Failure> {
    if init.hooks.iter().any(|hook| *hook != HookName::BeforePackOutputWritten) {
        return Err(Failure::refused(ErrorCode::UnsupportedCapability, Phase::Initialize, "undeclared-hook"));
    }
    let mut grants = Vec::new();
    for grant in &init.grants {
        let limit = match &grant.capability {
            Capability::FileRead { path, max_bytes } if path.as_str() == ".gitignore" => {
                GrantLimit::Read { max_bytes: (*max_bytes).min(MAX_IGNORE_BYTES) }
            }
            Capability::FileWrite { create: true, .. } => return Err(invalid("create-outside-profile")),
            Capability::FileWrite { path, max_growth_bytes, max_bytes, .. } if path.as_str() == ".gitignore" => {
                GrantLimit::Write {
                    max_growth: (*max_growth_bytes).min(MAX_GROWTH_BYTES),
                    max_bytes: (*max_bytes).min(MAX_REPLACEMENT_BYTES),
                }
            }
            Capability::GitAddIgnore { .. } => GrantLimit::Program,
            _ => return Err(invalid("capability-outside-profile")),
        };
        grants.push(Bound { handle: grant.handle.clone(), epoch: grant.policy_epoch, limit });
    }
    let mut ignore = None;
    let mut nested = Vec::new();
    let mut repositories = Vec::new();
    for resource in &init.resources {
        let role = resource.role.as_ref().map(dctx_plugin_protocol::records::scalars::Role::as_str);
        match (resource.kind, role) {
            (ResourceKind::File, None | Some("ignore")) if ignore.is_none() => ignore = Some(resource.handle.clone()),
            (ResourceKind::File, None | Some("ignore")) => return Err(invalid("duplicate-ignore-resource")),
            (ResourceKind::File, Some("nested-ignore")) => nested.push(resource.handle.clone()),
            (ResourceKind::Repository, role) => repositories.push(Repository {
                handle: resource.handle.clone(),
                ordinary: matches!(role, None | Some("ordinary")),
            }),
            _ => {}
        }
    }
    Ok(Binding { grants, ignore, nested, repositories, program_api, config: parse_config(&init.config)? })
}

impl Binding {
    /// Grants listed by an invocation at its policy epoch; anything else is not in force.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn grants(&self, listed: &[Token], epoch: u64) -> Grants {
        let mut grants = Grants::default();
        for bound in self.grants.iter().filter(|bound| bound.epoch == epoch && listed.contains(&bound.handle)) {
            match bound.limit {
                GrantLimit::Read { max_bytes } => grants.read = Some(max_bytes),
                GrantLimit::Write { max_growth, max_bytes } => grants.write = Some((max_growth, max_bytes)),
                GrantLimit::Program => grants.program = self.program_api,
            }
        }
        grants
    }

    /// The bound repository with this handle.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn repository(&self, handle: &Token) -> Option<&Repository> {
        self.repositories.iter().find(|repository| &repository.handle == handle)
    }
}
