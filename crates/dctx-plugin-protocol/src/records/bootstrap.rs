// SPDX-License-Identifier: Apache-2.0
//! Bootstrap envelope (`wire: 1`): the host hello and the worker's selection or refusal.
//! Implements bootstrap.schema.json and wire.md §Bootstrap and lifecycle; negotiation rules are in negotiate.
//! Depends on json, common, failure, scalars, refusal; used by envelope, negotiate and session.

use crate::json::fields::{Fields, Obj, array, constant, constant_int, list, unique_list};
use crate::json::value::Json;
use crate::records::common::{ApiVersion, HookName, Identity, Limits, VersionRange, decode_hook_decl, encode_hook_decl};
use crate::records::failure::WireError;
use crate::records::scalars::{Name, RequestId};
use crate::refusal::Refusal;

/// The host's single `hello` offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    /// Supported version ranges (1 through 8).
    pub versions: Vec<VersionRange>,
    /// Offered APIs (unique).
    pub apis: Vec<Name>,
    /// Offered hooks (unique).
    pub hooks: Vec<HookName>,
    /// Host ceilings.
    pub limits: Limits,
    /// Offered extensions (unique; the v1 base registry is empty).
    pub extensions: Vec<Name>,
}

/// The worker's selection in reply to hello.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelloSelected {
    /// Chosen version.
    pub version: ApiVersion,
    /// Claimed identity (must match the install record).
    pub identity: Identity,
    /// APIs the worker cannot run without.
    pub required_apis: Vec<Name>,
    /// APIs the worker uses when offered.
    pub optional_apis: Vec<Name>,
    /// Selected extensions.
    pub extensions: Vec<Name>,
}

/// A bootstrap message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapMessage {
    /// Host hello request.
    Hello {
        /// Host request ID.
        id: RequestId,
        /// Offer.
        hello: Box<Hello>,
    },
    /// Worker selection.
    Selected {
        /// Echoed hello ID.
        id: RequestId,
        /// Selection.
        selected: Box<HelloSelected>,
    },
    /// Worker refusal (for example `UnsupportedVersion`).
    Refused {
        /// Echoed hello ID.
        id: RequestId,
        /// Error.
        error: Box<WireError>,
    },
}

impl BootstrapMessage {
    /// Decodes a bootstrap object (the caller has seen the `wire` key).
    pub(crate) fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(value)?;
        constant_int(f.req("wire")?, 1)?;
        let id = RequestId::decode(f.req("id")?)?;
        let message = if f.req("kind")?.as_str() == Some("request") {
            constant(f.req("method")?, "hello")?;
            Self::Hello { id, hello: Box::new(decode_hello(f.req("params")?)?) }
        } else {
            constant(f.req("kind")?, "response")?;
            match (f.opt("result"), f.opt("error")) {
                (Some(result), None) => Self::Selected { id, selected: Box::new(decode_selected(result)?) },
                (None, Some(error)) => Self::Refused { id, error: Box::new(WireError::decode(error)?) },
                _ => return Err(Refusal::schema("response-needs-one-outcome")),
            }
        };
        f.finish()?;
        Ok(message)
    }

    pub(crate) fn encode(&self) -> Json {
        let base = Obj::new().put("wire", Json::Int(1));
        let object = match self {
            Self::Hello { id, hello } => base
                .put("kind", Json::str("request"))
                .put("id", id.encode())
                .put("method", Json::str("hello"))
                .put("params", encode_hello(hello)),
            Self::Selected { id, selected } => base
                .put("kind", Json::str("response"))
                .put("id", id.encode())
                .put("result", encode_selected(selected)),
            Self::Refused { id, error } => {
                base.put("kind", Json::str("response")).put("id", id.encode()).put("error", error.encode())
            }
        };
        object.build()
    }
}

fn decode_hello(value: &Json) -> Result<Hello, Refusal> {
    let mut f = Fields::of(value)?;
    constant(f.req("encoding")?, "json")?;
    let hello = Hello {
        versions: list(f.req("versions")?, 1, 8, VersionRange::decode)?,
        apis: unique_list(f.req("apis")?, 0, 64, Name::decode)?,
        hooks: unique_list(f.req("hooks")?, 0, 5, decode_hook_decl)?,
        limits: Limits::decode(f.req("limits")?)?,
        extensions: unique_list(f.req("extensions")?, 0, 16, Name::decode)?,
    };
    f.finish()?;
    Ok(hello)
}

fn encode_hello(hello: &Hello) -> Json {
    Obj::new()
        .put("versions", array(&hello.versions, VersionRange::encode))
        .put("encoding", Json::str("json"))
        .put("apis", array(&hello.apis, Name::encode))
        .put("hooks", array(&hello.hooks, encode_hook_decl))
        .put("limits", hello.limits.encode())
        .put("extensions", array(&hello.extensions, Name::encode))
        .build()
}

fn decode_selected(value: &Json) -> Result<HelloSelected, Refusal> {
    let mut f = Fields::of(value)?;
    let selected = HelloSelected {
        version: ApiVersion::decode(f.req("version")?)?,
        identity: Identity::decode(f.req("identity")?)?,
        required_apis: unique_list(f.req("required_apis")?, 0, 64, Name::decode)?,
        optional_apis: unique_list(f.req("optional_apis")?, 0, 64, Name::decode)?,
        extensions: unique_list(f.req("extensions")?, 0, 16, Name::decode)?,
    };
    f.finish()?;
    Ok(selected)
}

fn encode_selected(selected: &HelloSelected) -> Json {
    Obj::new()
        .put("version", selected.version.encode())
        .put("identity", selected.identity.encode())
        .put("required_apis", array(&selected.required_apis, Name::encode))
        .put("optional_apis", array(&selected.optional_apis, Name::encode))
        .put("extensions", array(&selected.extensions, Name::encode))
        .build()
}
