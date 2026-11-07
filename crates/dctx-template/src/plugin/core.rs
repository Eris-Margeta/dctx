// SPDX-License-Identifier: Apache-2.0
//! Plugin catalog value types, policy ranks and group shape.
//! Implements PLUGINS.md §3.5 and plugin-api/dts.md (exact locks, typed config).
//! Depends on [`super::super::error`]; [`crate::plugin_plan`] checks a template against a host.
//! A template narrows this catalog. It cannot fetch, retarget or widen it.

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
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Loopback => 1,
            Self::External => 2,
        }
    }

    /// Canonical token stored on a group.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
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
