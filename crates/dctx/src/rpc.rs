// SPDX-License-Identifier: Apache-2.0
//! Versioned engine RPC wire types. Output is opaque: transports never parse engine pages.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Protocol semver, independent of the engine release.
pub const PROTOCOL_VERSION: &str = "1.0.0";

/// A caller-owned correlation token. Reuse is allowed; ordering disambiguates replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Id {
    /// UTF-8 string token.
    String(String),
    /// Signed 64-bit integer token.
    Integer(i64),
}

/// Engine operations available over RPC. Arguments are the corresponding CLI words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// Required handshake, with no arguments.
    Hello,
    /// `dctx pack`, including `--for`.
    Pack,
    /// Alias for `dctx pack --for`; arguments include `--for TEXT`.
    Context,
    /// `dctx map map`.
    Map,
    /// `dctx map find`.
    Find,
    /// `dctx map read`.
    Read,
    /// `dctx map skeleton`.
    Skeleton,
    /// `dctx map explain`.
    Explain,
    /// `dctx map neighbors`.
    Neighbors,
    /// `dctx map grep`.
    Grep,
    /// `dctx map hubs`.
    Hubs,
    /// `dctx map path`.
    Path,
    /// `dctx trace`.
    Trace,
    /// `dctx impact`.
    Impact,
    /// `dctx languages`.
    Languages,
    /// `dctx cache status`.
    CacheStatus,
}

impl Verb {
    /// Complete advertised verb list, including the handshake.
    pub const ALL: [Self; 16] = [
        Self::Hello,
        Self::Pack,
        Self::Context,
        Self::Map,
        Self::Find,
        Self::Read,
        Self::Skeleton,
        Self::Explain,
        Self::Neighbors,
        Self::Grep,
        Self::Hubs,
        Self::Path,
        Self::Trace,
        Self::Impact,
        Self::Languages,
        Self::CacheStatus,
    ];

    /// CLI spelling of the operation (cache status is a two-word command).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hello => "hello",
            Self::Pack => "pack",
            Self::Context => "context",
            Self::Map => "map",
            Self::Find => "find",
            Self::Read => "read",
            Self::Skeleton => "skeleton",
            Self::Explain => "explain",
            Self::Neighbors => "neighbors",
            Self::Grep => "grep",
            Self::Hubs => "hubs",
            Self::Path => "path",
            Self::Trace => "trace",
            Self::Impact => "impact",
            Self::Languages => "languages",
            Self::CacheStatus => "cache_status",
        }
    }
}

/// One LF-delimited request. Unknown object fields are ignored for forward compatibility.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Request {
    /// Required correlation token, echoed verbatim in the reply.
    pub id: Id,
    /// Engine operation; the first accepted request must be `hello`.
    pub verb: Verb,
    /// CLI arguments after the operation, without the implicit `--json`.
    /// Positional values and flags are passed unchanged; `--json` may also be explicit.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Handshake answer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Hello {
    /// Independent engine protocol semver.
    pub protocol_version: String,
    /// Supported operations.
    pub verbs: Vec<Verb>,
}

/// Successful engine result. No page is decoded or re-encoded by this envelope.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ResultBody {
    /// Handshake metadata.
    Hello(Hello),
    /// Exact CLI stdout, including its trailing newline(s).
    Output {
        /// Engine-produced UTF-8 bytes as a JSON string.
        output: String,
    },
}

/// Stable CLI error taxonomy, including output emitted before an exit-1 outcome.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Error {
    /// 1 = no result, 2 = usage/protocol refusal, 3 = index/I/O error.
    #[schemars(range(min = 1, max = 3))]
    pub code: i32,
    /// Human-readable diagnostic; clients must branch on `code`, not this text.
    pub message: String,
    /// Actionable CLI hint.
    pub hint: String,
    /// Exact stdout preceding the error, empty for failures before rendering.
    pub output: String,
}

/// Exactly one result or error.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Outcome {
    /// A successful operation.
    Success {
        /// Handshake metadata or opaque engine output.
        result: ResultBody,
    },
    /// A failed operation, using the CLI taxonomy.
    Failure {
        /// Original failure and any already-emitted page.
        error: Error,
    },
}

/// One LF-delimited response, flushed before reading the next request.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Response {
    /// Echoed request token; null when the request token could not be decoded.
    #[schemars(required)]
    pub id: Option<Id>,
    /// Engine release that produced this reply, including protocol errors.
    pub engine_version: String,
    /// Exactly one result or error.
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// All messages in a v1 session, used as the schema generation root.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(
    dead_code,
    reason = "Schema root only; actual messages use Request and Response."
)]
enum Message {
    Request(Request),
    Response(Response),
}

/// Generates the committed request/response JSON Schema from the wire types.
/// # Errors
/// Returns a serialization error if the schema cannot be encoded.
pub fn schema() -> Result<String, serde_json::Error> {
    let schema = schemars::schema_for!(Message);
    serde_json::to_string_pretty(&schema).map(|text| text + "\n")
}
