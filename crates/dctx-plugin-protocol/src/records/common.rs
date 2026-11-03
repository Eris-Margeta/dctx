// SPDX-License-Identifier: Apache-2.0
//! Shared records from common.schema.json: versions, ranges, hooks, limits, identity, waits, resources,
//! diagnostics, config values, extensions and command records.
//! Implements plugin-api common.schema.json `$defs` exactly (closed objects, bounded arrays/maps).
//! Depends on json, scalars and refusal; used by bootstrap, requests, results, events and invocation.

use std::collections::BTreeMap;

use crate::json::fields::{Fields, Obj, array, constant, constant_int, int_in, list, positive, string, uint};
use crate::json::value::Json;
use crate::records::scalars::{Name, RequestId, Role, Text, Token};
use crate::records::scalars::{Digest, PluginId, SemVer};
use crate::refusal::Refusal;

/// A protocol version `[major, minor]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    /// Major version.
    pub major: u64,
    /// Minor version.
    pub minor: u64,
}

impl ApiVersion {
    /// The only version frozen in v1.
    pub const V1_0: Self = Self { major: 1, minor: 0 };

    /// Decodes the schema `version` constant, which admits only `[1,0]`.
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let parts = list(value, 2, 2, uint)?;
        if parts == [1, 0] { Ok(Self::V1_0) } else { Err(Refusal::schema("unsupported-version")) }
    }

    pub(crate) fn encode(self) -> Json {
        Json::Array(vec![Json::uint(self.major), Json::uint(self.minor)])
    }
}

/// A supported version range for one major version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VersionRange {
    /// Major version (at least 1).
    pub major: u64,
    /// Lowest supported minor.
    pub min_minor: u64,
    /// Highest supported minor.
    pub max_minor: u64,
}

impl VersionRange {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let range = Self {
            major: positive(fields.req("major")?)?,
            min_minor: uint(fields.req("min_minor")?)?,
            max_minor: uint(fields.req("max_minor")?)?,
        };
        fields.finish()?;
        Ok(range)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("major", Json::uint(self.major))
            .put("min_minor", Json::uint(self.min_minor))
            .put("max_minor", Json::uint(self.max_minor))
            .build()
    }
}

wire_enum!(
    /// Lifecycle hooks a plugin can declare (`hook` enum).
    HookName {
        /// Before pack output is created.
        BeforePackOutputWritten => "before-pack-output-written",
        /// After pack output is committed.
        AfterPackOutputWritten => "after-pack-output-written",
        /// Before index content reads.
        BeforeIndex => "before-index",
        /// After complete index publication.
        AfterIndex => "after-index",
        /// After core cache GC.
        CacheGc => "cache-gc",
    }
);

/// `hook_decl`: a hook name at `hook_version` 1.
pub(crate) fn decode_hook_decl(value: &Json) -> Result<HookName, Refusal> {
    let mut fields = Fields::of(value)?;
    let name = HookName::decode(fields.req("name")?)?;
    constant_int(fields.req("version")?, 1)?;
    fields.finish()?;
    Ok(name)
}

#[allow(clippy::trivially_copy_pass_by_ref, reason = "This encoder is passed as a callback over borrowed hook declarations, matching the other record encoders.")]
pub(crate) fn encode_hook_decl(name: &HookName) -> Json {
    Obj::new().put("name", name.encode()).put("version", Json::Int(1)).build()
}

/// Resource ceilings (`limits`); every field is required.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    /// Frame ceiling, 1 through 1 MiB.
    pub frame_bytes: u64,
    /// Cumulative delivered content and staged bytes.
    pub read_bytes: u64,
    /// Admitted broker actions.
    pub actions: u64,
    /// Outstanding broker requests, 1 through 16.
    pub outstanding_requests: u64,
    /// Diagnostic bytes, at most 65536.
    pub diagnostics_bytes: u64,
    /// Raw chunk bytes, 1 through 65536.
    pub chunk_bytes: u64,
    /// Retained state bytes.
    pub state_bytes: u64,
    /// Live private upload bytes.
    pub upload_bytes: u64,
    /// Per-direction queue bytes.
    pub queue_bytes: u64,
    /// Worker memory ceiling.
    pub memory_bytes: u64,
    /// Caller-admitted work units.
    pub work_units: u64,
}

impl Limits {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        let limits = Self {
            frame_bytes: int_in(f.req("frame_bytes")?, 1, 1_048_576)?,
            read_bytes: uint(f.req("read_bytes")?)?,
            actions: uint(f.req("actions")?)?,
            outstanding_requests: int_in(f.req("outstanding_requests")?, 1, 16)?,
            diagnostics_bytes: int_in(f.req("diagnostics_bytes")?, 0, 65_536)?,
            chunk_bytes: int_in(f.req("chunk_bytes")?, 1, 65_536)?,
            state_bytes: uint(f.req("state_bytes")?)?,
            upload_bytes: uint(f.req("upload_bytes")?)?,
            queue_bytes: positive(f.req("queue_bytes")?)?,
            memory_bytes: positive(f.req("memory_bytes")?)?,
            work_units: positive(f.req("work_units")?)?,
        };
        f.finish()?;
        Ok(limits)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("frame_bytes", Json::uint(self.frame_bytes))
            .put("read_bytes", Json::uint(self.read_bytes))
            .put("actions", Json::uint(self.actions))
            .put("outstanding_requests", Json::uint(self.outstanding_requests))
            .put("diagnostics_bytes", Json::uint(self.diagnostics_bytes))
            .put("chunk_bytes", Json::uint(self.chunk_bytes))
            .put("state_bytes", Json::uint(self.state_bytes))
            .put("upload_bytes", Json::uint(self.upload_bytes))
            .put("queue_bytes", Json::uint(self.queue_bytes))
            .put("memory_bytes", Json::uint(self.memory_bytes))
            .put("work_units", Json::uint(self.work_units))
            .build()
    }
}

wire_enum!(
    /// Diagnostic severity.
    Severity {
        /// Informational.
        Info => "info",
        /// Warning.
        Warning => "warning",
        /// Error.
        Error => "error",
    }
);

/// A bounded diagnostic whose code must come from the manifest dictionary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Diagnostic code.
    pub code: Name,
    /// Severity.
    pub severity: Severity,
    /// Related resource handle, if any.
    pub resource_handle: Option<Token>,
}

impl Diagnostic {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let diagnostic = Self {
            code: Name::decode(fields.req("code")?)?,
            severity: Severity::decode(fields.req("severity")?)?,
            resource_handle: fields.opt("resource_handle").map(Token::decode).transpose()?,
        };
        fields.finish()?;
        Ok(diagnostic)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("code", self.code.encode())
            .put("severity", self.severity.encode())
            .maybe("resource_handle", self.resource_handle.as_ref().map(Token::encode))
            .build()
    }
}

/// Verified plugin identity (`identity`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// Plugin ID.
    pub id: PluginId,
    /// Plugin version.
    pub version: SemVer,
    /// Publisher key reference.
    pub publisher_key: Token,
    /// Artifact digest.
    pub artifact_digest: Digest,
}

impl Identity {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let identity = Self {
            id: PluginId::decode(fields.req("id")?)?,
            version: SemVer::decode(fields.req("version")?)?,
            publisher_key: Token::decode(fields.req("publisher_key")?)?,
            artifact_digest: Digest::decode(fields.req("artifact_digest")?)?,
        };
        fields.finish()?;
        Ok(identity)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("id", self.id.encode())
            .put("version", self.version.encode())
            .put("publisher_key", self.publisher_key.encode())
            .put("artifact_digest", self.artifact_digest.encode())
            .build()
    }
}

/// What a waiting worker depends on (`wait`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wait {
    /// An outstanding broker request.
    BrokerRequest(RequestId),
    /// A host-registered live dependency.
    Dependency(Token),
}

impl Wait {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let wait = if fields.has("broker_request") {
            Self::BrokerRequest(RequestId::decode(fields.req("broker_request")?)?)
        } else {
            Self::Dependency(Token::decode(fields.req("dependency")?)?)
        };
        fields.finish()?;
        Ok(wait)
    }

    pub(crate) fn encode(&self) -> Json {
        match self {
            Self::BrokerRequest(id) => Obj::new().put("broker_request", id.encode()).build(),
            Self::Dependency(token) => Obj::new().put("dependency", token.encode()).build(),
        }
    }
}

wire_enum!(
    /// Kind of a host-issued resource handle.
    ResourceKind {
        /// Project root.
        Project => "project",
        /// Named file.
        File => "file",
        /// Pack output.
        Output => "output",
        /// Index generation.
        Generation => "generation",
        /// Plugin state object.
        State => "state",
        /// Repository administrative resource.
        Repository => "repository",
    }
);

/// An opaque resource handle supplied by initialize (`resource`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// Opaque handle.
    pub handle: Token,
    /// Resource kind.
    pub kind: ResourceKind,
    /// Optional role.
    pub role: Option<Role>,
    /// Optional size.
    pub bytes: Option<u64>,
}

impl Resource {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let resource = Self {
            handle: Token::decode(fields.req("handle")?)?,
            kind: ResourceKind::decode(fields.req("kind")?)?,
            role: fields.opt("role").map(Role::decode).transpose()?,
            bytes: fields.opt("bytes").map(uint).transpose()?,
        };
        fields.finish()?;
        Ok(resource)
    }

    pub(crate) fn encode(&self) -> Json {
        Obj::new()
            .put("handle", self.handle.encode())
            .put("kind", self.kind.encode())
            .maybe("role", self.role.as_ref().map(Role::encode))
            .maybe("bytes", self.bytes.map(Json::uint))
            .build()
    }
}

/// `scalar`: boolean, safe unsigned integer or bounded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scalar {
    /// Boolean.
    Bool(bool),
    /// Unsigned integer.
    Uint(u64),
    /// Text up to 4096 bytes.
    Text(Text),
}

impl Scalar {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        match value {
            Json::Bool(flag) => Ok(Self::Bool(*flag)),
            Json::Int(_) => uint(value).map(Self::Uint),
            Json::Str(_) => Text::decode(value).map(Self::Text),
            _ => Err(Refusal::schema("expected-scalar")),
        }
    }

    pub(crate) fn encode(&self) -> Json {
        match self {
            Self::Bool(flag) => Json::Bool(*flag),
            Self::Uint(number) => Json::uint(*number),
            Self::Text(text) => text.encode(),
        }
    }
}

/// `config_value`: a scalar or a flat list of at most 64 scalars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigValue {
    /// One scalar.
    Scalar(Scalar),
    /// A flat list.
    List(Vec<Scalar>),
}

/// Validated config: at most 64 named values.
pub type Config = BTreeMap<Name, ConfigValue>;

/// Envelope extensions: at most 16 named scalars.
pub type Extensions = BTreeMap<Name, Scalar>;

fn decode_map<T>(value: &Json, max: usize, item: impl Fn(&Json) -> Result<T, Refusal>) -> Result<BTreeMap<Name, T>, Refusal> {
    let members = value.as_object().ok_or(Refusal::schema("expected-object"))?;
    if members.len() > max {
        return Err(Refusal::schema("too-many-properties"));
    }
    members.iter().map(|(key, entry)| Ok((Name::new(key)?, item(entry)?))).collect()
}

fn encode_map<T>(map: &BTreeMap<Name, T>, item: impl Fn(&T) -> Json) -> Json {
    Json::Object(map.iter().map(|(key, entry)| (key.as_str().to_owned(), item(entry))).collect())
}

pub(crate) fn decode_config(value: &Json) -> Result<Config, Refusal> {
    decode_map(value, 64, |entry| match entry {
        Json::Array(_) => list(entry, 0, 64, Scalar::decode).map(ConfigValue::List),
        _ => Scalar::decode(entry).map(ConfigValue::Scalar),
    })
}

pub(crate) fn encode_config(config: &Config) -> Json {
    encode_map(config, |entry| match entry {
        ConfigValue::Scalar(scalar) => scalar.encode(),
        ConfigValue::List(items) => array(items, Scalar::encode),
    })
}

pub(crate) fn decode_extensions(value: &Json) -> Result<Extensions, Refusal> {
    decode_map(value, 16, Scalar::decode)
}

pub(crate) fn encode_extensions(extensions: &Extensions) -> Json {
    encode_map(extensions, Scalar::encode)
}

/// A typed command output record (`command_record`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandRecord {
    /// Bounded text, permitted only when the manifest sets `allow_text`.
    Text(Text),
    /// A declared record with typed fields.
    Record {
        /// Declared record name.
        name: Name,
        /// Field values.
        fields: Config,
    },
}

impl CommandRecord {
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let kind = string(fields.req("type")?)?;
        let record = if kind == "text" {
            Self::Text(Text::decode(fields.req("text")?)?)
        } else {
            constant(fields.req("type")?, "record")?;
            Self::Record { name: Name::decode(fields.req("name")?)?, fields: decode_config(fields.req("fields")?)? }
        };
        fields.finish()?;
        Ok(record)
    }

    pub(crate) fn encode(&self) -> Json {
        match self {
            Self::Text(text) => Obj::new().put("type", Json::str("text")).put("text", text.encode()).build(),
            Self::Record { name, fields } => Obj::new()
                .put("type", Json::str("record"))
                .put("name", name.encode())
                .put("fields", encode_config(fields))
                .build(),
        }
    }
}

#[cfg(test)]
mod tests;
