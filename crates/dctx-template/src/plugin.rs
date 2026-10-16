//! Plugin catalog, typed grants and the compiled plugin plan.
//! Implements PLUGINS.md §3.5 and plugin-api/dts.md (exact locks, typed config).
//! Depends on [`crate::error`]. [`crate::plugin_plan`] checks a template against a host.
//! A template narrows this catalog. It cannot fetch, retarget or widen it.

use crate::error::TemplateError;

/// How much network a plugin group may use. Higher ranks are wider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    /// No network. Version 1 DTS requires this.
    None,
    /// Loopback only.
    Loopback,
    /// Outbound network.
    External,
}

impl NetworkPolicy {
    /// Width rank. A template may only move toward [`NetworkPolicy::None`].
    pub const fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Loopback => 1,
            Self::External => 2,
        }
    }

    /// Canonical token stored on a group.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Loopback => "loopback",
            Self::External => "external",
        }
    }
}

/// Whether a compiled plugin binding will run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// Present, allowed and not disabled.
    Enabled,
    /// The user disable or the template `enabled` list turned it off.
    Disabled,
    /// Optional plugin missing from the host catalog.
    Unavailable,
}

/// Where the host resolved the artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOrigin {
    /// Shipped inside the distribution.
    Bundled,
    /// Installed from a verified release.
    Installed,
    /// Explicit operator override of a bundled id.
    Override,
}

/// Why a compiled entry has no artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// The id is not in the host catalog.
    NotInstalled,
    /// The installed artifact does not satisfy the declaration.
    Incompatible,
    /// No build exists for this platform.
    UnsupportedPlatform,
    /// Verification failed and the host quarantined the artifact.
    Quarantined,
}

/// Lock row on a compiled entry. Unavailable rows carry no artifact identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Exact host lock.
    Available,
    /// Recorded absence. `required` is false and grants are empty.
    Unavailable {
        /// Why the host has no usable artifact.
        reason: UnavailableReason,
    },
}

/// A plugin config value after schema checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValue {
    /// Boolean.
    Bool(bool),
    /// Bounded integer.
    Int(u64),
    /// Bounded string.
    Str(String),
    /// Bounded list of strings.
    List(Vec<String>),
    /// One of the manifest's enum tokens, kept distinct from a free string.
    Enum(String),
}

/// One config assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEntry {
    /// Field name without the `config.` prefix.
    pub key: String,
    /// Checked value.
    pub value: ConfigValue,
}

/// Manifest field type. Values outside this shape are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// `true` or `false`.
    Bool,
    /// Integer in `min..=max`.
    Int {
        /// Inclusive minimum.
        min: u64,
        /// Inclusive maximum.
        max: u64,
    },
    /// One of `options`. Options may be bool, int or string scalars.
    Enum {
        /// Allowed scalars.
        options: Vec<ConfigValue>,
    },
    /// String up to `max_bytes`.
    Str {
        /// Maximum UTF-8 length.
        max_bytes: usize,
    },
    /// List of short strings.
    List {
        /// Most items.
        max_items: usize,
        /// Most bytes in one item.
        item_max_bytes: usize,
    },
}

/// One manifest field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    /// Field name.
    pub name: String,
    /// Accepted shape.
    pub kind: FieldKind,
    /// Manifest default. Omitted template keys use this after user settings.
    pub default_value: Option<ConfigValue>,
}

/// Typed broker ceiling copied from a verified manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// Read one literal project path.
    FileRead {
        /// Root-relative path.
        path: String,
        /// Most bytes.
        max_bytes: u64,
    },
    /// Write one literal project path.
    FileWrite {
        /// Root-relative path.
        path: String,
        /// Whether creation is in the ceiling. Git v1 keeps this false.
        create: bool,
        /// Most bytes the file may grow.
        max_growth_bytes: u64,
        /// Most bytes after the write.
        max_bytes: u64,
    },
    /// The sole v1 program recipe.
    Program {
        /// `git.add-ignore`.
        operation: String,
        /// Administrative resources. v1 is only `git-admin:stage-ignore`.
        resources: Vec<String>,
        /// Most captured output bytes.
        max_output_bytes: u64,
    },
    /// Read selected output roles. One record, not one per selector.
    OutputsRead {
        /// Selected roles.
        roles: Vec<String>,
        /// Selected operations (`pack` or `index`).
        operations: Vec<String>,
        /// Most bytes.
        max_bytes: u64,
    },
    /// The plugin's own state namespace.
    State {
        /// `state:read` selected.
        read: bool,
        /// `state:write` selected.
        write: bool,
        /// `state:reclaim` selected.
        reclaim: bool,
        /// Quota.
        max_bytes: u64,
        /// Manifest asked to retain source bytes. Still needs provenance consent.
        source_content: bool,
    },
}

/// Hook name plus payload version. v1 payloads are version 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookDecl {
    /// Manifest hook name.
    pub name: String,
    /// Payload version. This IR accepts only 1.
    pub version: u16,
}

/// A plugin the host already resolved. The template cannot fetch or retarget it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPlugin {
    /// Canonical id (`dctx/git`).
    pub id: String,
    /// Exact resolved semver.
    pub version: String,
    /// Artifact digest, 32 bytes, when the host has locked one.
    pub digest: Option<[u8; 32]>,
    /// Manifest digest, 32 bytes.
    pub manifest_digest: Option<[u8; 32]>,
    /// Publisher token. Empty when the test catalog has no publisher.
    pub publisher_key: String,
    /// How the host obtained the artifact.
    pub origin: PluginOrigin,
    /// Hooks the manifest supports.
    pub hooks: Vec<String>,
    /// Command alias, or empty.
    pub alias: String,
    /// Capability selectors the manifest declares. This is the policy ceiling.
    pub capabilities: Vec<String>,
    /// Typed records. Empty means selectors synthesize the v1 default limits.
    pub records: Vec<Capability>,
    /// Config fields the manifest defines.
    pub fields: Vec<FieldSpec>,
}

impl KnownPlugin {
    /// Catalog row with no digests and an installed origin.
    pub fn named(id: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            digest: None,
            manifest_digest: None,
            publisher_key: String::new(),
            origin: PluginOrigin::Installed,
            hooks: Vec::new(),
            alias: String::new(),
            capabilities: Vec::new(),
            records: Vec::new(),
            fields: Vec::new(),
        }
    }
}

/// One plugin group (`[PLUGINS:name]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginGroup {
    /// Organization name.
    pub name: String,
    /// Network ceiling after narrowing. v1 is [`NetworkPolicy::None`].
    pub network: NetworkPolicy,
    /// Enabled ids in source order.
    pub enabled: Vec<String>,
    /// Explicit order edges in source order.
    pub order: Vec<String>,
}

/// One compiled plugin binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBinding {
    /// Plugin id.
    pub id: String,
    /// Resolved version, or empty when unavailable.
    pub version: String,
    /// Artifact digest from the host lock.
    pub digest: Option<[u8; 32]>,
    /// Manifest digest from the host lock.
    pub manifest_digest: Option<[u8; 32]>,
    /// Publisher from the host lock, or empty when unavailable.
    pub publisher_key: String,
    /// Origin from the host lock. Absent when unavailable.
    pub origin: Option<PluginOrigin>,
    /// Available lock or recorded absence.
    pub resolution: Resolution,
    /// The template marked the plugin required. Unavailable entries are false.
    pub required: bool,
    /// Enablement after user disable and catalog lookup.
    pub state: PluginState,
    /// Command alias, or empty.
    pub alias: String,
    /// Effective hooks. Empty unless [`PluginState::Enabled`].
    pub hooks: Vec<HookDecl>,
    /// Effective typed grants. Empty unless [`PluginState::Enabled`].
    pub allow: Vec<Capability>,
    /// Effective config in key order. Empty unless [`PluginState::Enabled`].
    pub config: Vec<ConfigEntry>,
    /// Plugins that must run before this one.
    pub before: Vec<String>,
    /// Plugins that must run after this one.
    pub after: Vec<String>,
}

/// Caller and organization resource ceiling copied into the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Largest accepted frame.
    pub frame_bytes: u64,
    /// Most bytes read through the broker.
    pub read_bytes: u64,
    /// Most broker actions.
    pub actions: u64,
    /// Most outstanding broker requests.
    pub outstanding_requests: u64,
    /// Diagnostics ring.
    pub diagnostics_bytes: u64,
    /// Largest raw chunk.
    pub chunk_bytes: u64,
    /// Plugin state quota.
    pub state_bytes: u64,
    /// Temporary upload quota.
    pub upload_bytes: u64,
    /// Queue bytes.
    pub queue_bytes: u64,
    /// Memory budget.
    pub memory_bytes: u64,
    /// Work counter.
    pub work_units: u64,
}

impl ResourceLimits {
    /// Defaults from the v1 git manifest and dts plan fixture.
    pub const fn standard() -> Self {
        Self {
            frame_bytes: 1_048_576,
            read_bytes: 8_388_608,
            actions: 128,
            outstanding_requests: 16,
            diagnostics_bytes: 65_536,
            chunk_bytes: 65_536,
            state_bytes: 65_536,
            upload_bytes: 1_052_672,
            queue_bytes: 2_097_152,
            memory_bytes: 67_108_864,
            work_units: 1_000_000,
        }
    }

    /// Every field is at most `ceiling`.
    pub fn within(&self, ceiling: &Self) -> Result<(), TemplateError> {
        within(self.frame_bytes, ceiling.frame_bytes, "frame_bytes")?;
        within(self.read_bytes, ceiling.read_bytes, "read_bytes")?;
        within(self.actions, ceiling.actions, "actions")?;
        within(self.outstanding_requests, ceiling.outstanding_requests, "outstanding_requests")?;
        within(self.diagnostics_bytes, ceiling.diagnostics_bytes, "diagnostics_bytes")?;
        within(self.chunk_bytes, ceiling.chunk_bytes, "chunk_bytes")?;
        within(self.state_bytes, ceiling.state_bytes, "state_bytes")?;
        within(self.upload_bytes, ceiling.upload_bytes, "upload_bytes")?;
        within(self.queue_bytes, ceiling.queue_bytes, "queue_bytes")?;
        within(self.memory_bytes, ceiling.memory_bytes, "memory_bytes")?;
        within(self.work_units, ceiling.work_units, "work_units")
    }
}

fn within(requested: u64, ceiling: u64, field: &'static str) -> Result<(), TemplateError> {
    if requested > ceiling {
        Err(TemplateError::BudgetAboveCeiling { field, requested, ceiling })
    } else {
        Ok(())
    }
}

/// Liveness observation policy. The template cannot set it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Liveness {
    /// Interactive continue/cancel. No inactivity kill.
    Interactive {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
    },
    /// Unattended. Unresolved suspicion fails closed.
    UnattendedFailClosed {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
    },
    /// Unattended with a bounded last-resort inactivity window.
    UnattendedBounded {
        /// Observation cadence, in milliseconds.
        sample_ms: u64,
        /// Reap grace after cancel, in milliseconds.
        cancel_grace_ms: u64,
        /// Inactivity observation window, in milliseconds.
        inactivity_ms: u64,
    },
}

impl Liveness {
    /// Unattended fail-closed defaults from the dts plan fixture.
    pub const fn fail_closed() -> Self {
        Self::UnattendedFailClosed { sample_ms: 1000, cancel_grace_ms: 5000 }
    }
}

/// Plugin section of a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginPlan {
    /// PLUGINS section name, or empty when the source has none.
    pub organization: String,
    /// At most one group.
    pub groups: Vec<PluginGroup>,
    /// Bindings in id order.
    pub plugins: Vec<PluginBinding>,
    /// Enabled plugins in topological order. Canonical id breaks ties.
    pub order: Vec<String>,
    /// Admitted resource ceiling.
    pub limits: ResourceLimits,
    /// Admitted liveness policy.
    pub liveness: Liveness,
}

/// Built-in command words a plugin alias cannot take (PLUGINS.md §3.5).
pub const RESERVED_ALIASES: &[&str] = &["plugin", "pack", "map", "cache", "template", "help", "version"];

/// The bundled `dctx/git` manifest from the plugin-api fixtures. The host still opts in.
pub fn bundled_git_plugin() -> KnownPlugin {
    KnownPlugin {
        id: "dctx/git".to_string(),
        version: "1.0.0".to_string(),
        digest: Some(hex32("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")),
        manifest_digest: Some(hex32("bf74b6b3ff523cb08461bff8bbaa48dba16d91026d17ecd59e683beb165cbf9c")),
        publisher_key: "fixture-key-1".to_string(),
        origin: PluginOrigin::Bundled,
        hooks: vec!["before-pack-output-written".to_string()],
        alias: "git".to_string(),
        capabilities: vec![
            "file.read:.gitignore".to_string(),
            "file.write:.gitignore".to_string(),
            "program:git.add-ignore".to_string(),
            "resource:git-admin:stage-ignore".to_string(),
        ],
        records: vec![
            Capability::FileRead { path: ".gitignore".to_string(), max_bytes: 1_048_576 },
            Capability::FileWrite {
                path: ".gitignore".to_string(),
                create: false,
                max_growth_bytes: 4096,
                max_bytes: 1_052_672,
            },
            Capability::Program {
                operation: "git.add-ignore".to_string(),
                resources: vec!["git-admin:stage-ignore".to_string()],
                max_output_bytes: 65_536,
            },
        ],
        fields: vec![
            FieldSpec {
                name: "stage".to_string(),
                kind: FieldKind::Bool,
                default_value: Some(ConfigValue::Bool(true)),
            },
            FieldSpec {
                name: "create_missing".to_string(),
                kind: FieldKind::Enum { options: vec![ConfigValue::Bool(false)] },
                default_value: Some(ConfigValue::Bool(false)),
            },
            FieldSpec {
                name: "marker".to_string(),
                kind: FieldKind::Enum { options: vec![ConfigValue::Str("host-version".to_string())] },
                default_value: Some(ConfigValue::Str("host-version".to_string())),
            },
        ],
    }
}

fn hex32(text: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let bytes = text.as_bytes();
    for index in 0..32 {
        let hi = hex_val(bytes[index * 2]);
        let lo = hex_val(bytes[index * 2 + 1]);
        out[index] = (hi << 4) | lo;
    }
    out
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        _ => 0,
    }
}
