// SPDX-License-Identifier: Apache-2.0
//! Wire message envelope: decode a frame body into a bootstrap or session message and encode it back.
//! Implements wire.schema.json (bootstrap | request | response | event), exactly one of result/error,
//! no id on events, closed envelopes and per-message semantic checks. Depends on json and every record.
//! Used by session tracking, host/worker adapters and the corpus tests. Encoding is canonical JSON.

use crate::json::decode::{DecodeLimits, decode_object};
use crate::json::encode::to_canonical;
use crate::json::fields::{Fields, Obj, positive, string};
use crate::json::value::Json;
use crate::records::bootstrap::BootstrapMessage;
use crate::records::common::{ApiVersion, Extensions, decode_extensions, encode_extensions};
use crate::records::events::Event;
use crate::records::failure::WireError;
use crate::records::method::Method;
use crate::records::requests::Request;
use crate::records::results::ResultBody;
use crate::records::scalars::{RequestId, Token};
use crate::refusal::Refusal;

/// A response result whose exact shape depends on the pending request (W-RESULT).
/// Equality ignores object member order.
#[derive(Clone, Debug)]
pub struct PendingResult(Json);

impl PartialEq for PendingResult {
    fn eq(&self, other: &Self) -> bool {
        self.0.same_value(&other.0)
    }
}

impl Eq for PendingResult {}

impl PendingResult {
    /// Wraps a typed result for sending.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn from_body(body: &ResultBody) -> Self {
        Self(body.encode())
    }

    /// Resolves the result with the pending request's method.
    /// # Errors
    /// Refuses a result that does not satisfy the pending method's schema or semantic rules.
    pub fn resolve(&self, method: Method) -> Result<ResultBody, Refusal> {
        ResultBody::decode(method, &self.0)
    }

    /// The validated union value.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn as_json(&self) -> &Json {
        &self.0
    }
}

/// Exactly one of result or error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Successful result, resolved later by method.
    Result(PendingResult),
    /// Typed error.
    Error(Box<WireError>),
}

/// The body of a session envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionBody {
    /// A request with a sender-owned ID.
    Request {
        /// Request ID.
        id: RequestId,
        /// Typed request.
        request: Request,
    },
    /// A response to a pending request.
    Response {
        /// Pending request ID.
        id: RequestId,
        /// Result or error.
        outcome: Outcome,
    },
    /// A plugin event with an envelope sequence.
    Event {
        /// Envelope sequence (per sender/session, strictly increasing).
        seq: u64,
        /// Typed event.
        event: Event,
    },
}

/// A session envelope at version `[1,0]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMessage {
    /// Opaque session ID.
    pub session: Token,
    /// Envelope extensions (must be negotiated).
    pub extensions: Option<Extensions>,
    /// Body.
    pub body: SessionBody,
}

/// Any message carried in one frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireMessage {
    /// `wire: 1` bootstrap.
    Bootstrap(BootstrapMessage),
    /// `v: [1,0]` session envelope.
    Session(SessionMessage),
}

impl WireMessage {
    /// Decodes one frame body under `limits` through the decode, schema and semantic layers.
    /// # Errors
    /// Propagates JSON decode limits, schema violations, and protocol semantic refusals.
    pub fn decode(body: &[u8], limits: DecodeLimits) -> Result<Self, Refusal> {
        Self::from_json(&decode_object(body, limits)?)
    }

    /// Decodes an already strictly decoded value.
    /// # Errors
    /// Refuses bootstrap/session schema violations or invalid protocol field values.
    pub fn from_json(value: &Json) -> Result<Self, Refusal> {
        if value.get("wire").is_some() {
            return BootstrapMessage::decode(value).map(Self::Bootstrap);
        }
        decode_session(value).map(Self::Session)
    }

    /// Canonical JSON value.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn to_json(&self) -> Json {
        match self {
            Self::Bootstrap(message) => message.encode(),
            Self::Session(message) => encode_session(message),
        }
    }

    /// Canonical frame body bytes.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn encode(&self) -> Vec<u8> {
        to_canonical(&self.to_json())
    }
}

fn decode_session(value: &Json) -> Result<SessionMessage, Refusal> {
    let mut f = Fields::of(value)?;
    ApiVersion::decode(f.req("v")?)?;
    let session = Token::decode(f.req("session")?)?;
    let extensions = f.opt("extensions").map(decode_extensions).transpose()?;
    let body = match string(f.req("kind")?)? {
        "request" => {
            let id = RequestId::decode(f.req("id")?)?;
            let method =
                Method::decode(f.req("method")?).map_err(|_| Refusal::schema("unknown-method"))?;
            SessionBody::Request {
                id,
                request: Request::decode(method, f.req("params")?)?,
            }
        }
        "response" => {
            let id = RequestId::decode(f.req("id")?)?;
            let outcome = match (f.opt("result"), f.opt("error")) {
                (Some(result), None) => {
                    if !ResultBody::matches_any(result) {
                        return Err(Refusal::schema("unknown-result-shape"));
                    }
                    Outcome::Result(PendingResult(result.clone()))
                }
                (None, Some(error)) => Outcome::Error(Box::new(WireError::decode(error)?)),
                _ => return Err(Refusal::schema("response-needs-one-outcome")),
            };
            SessionBody::Response { id, outcome }
        }
        "event" => {
            let seq = positive(f.req("seq")?)?;
            let method = string(f.req("method")?)?;
            SessionBody::Event {
                seq,
                event: Event::decode(method, f.req("params")?)?,
            }
        }
        _ => return Err(Refusal::schema("unknown-kind")),
    };
    f.finish()?;
    Ok(SessionMessage {
        session,
        extensions,
        body,
    })
}

fn encode_session(message: &SessionMessage) -> Json {
    let base = Obj::new()
        .put("v", ApiVersion::V1_0.encode())
        .put("session", message.session.encode());
    let base = match &message.body {
        SessionBody::Request { id, request } => base
            .put("kind", Json::str("request"))
            .put("id", id.encode())
            .put("method", request.method().encode())
            .put("params", request.encode_params()),
        SessionBody::Response { id, outcome } => {
            let base = base
                .put("kind", Json::str("response"))
                .put("id", id.encode());
            match outcome {
                Outcome::Result(result) => base.put("result", result.0.clone()),
                Outcome::Error(error) => base.put("error", error.encode()),
            }
        }
        SessionBody::Event { seq, event } => base
            .put("kind", Json::str("event"))
            .put("seq", Json::uint(*seq))
            .put("method", Json::str(event.method()))
            .put("params", event.encode_params()),
    };
    base.maybe(
        "extensions",
        message.extensions.as_ref().map(encode_extensions),
    )
    .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refusal::Stage;

    fn decode(text: &str) -> Result<WireMessage, Refusal> {
        WireMessage::decode(text.as_bytes(), DecodeLimits::WIRE)
    }

    const READ: &str = r#"{"v":[1,0],"session":"s:1","kind":"request","id":"p:1","method":"file.read","params":{"resource":"file:ignore","offset":0,"length":3}}"#;

    #[test]
    fn request_envelope_round_trips_canonically() {
        let message = decode(READ).expect("valid");
        let canonical =
            to_canonical(&decode_object(READ.as_bytes(), DecodeLimits::WIRE).expect("json"));
        assert_eq!(message.encode(), canonical);
        assert!(
            String::from_utf8(canonical)
                .expect("utf8")
                .starts_with("{\"id\":\"p:1\",\"kind\":\"request\"")
        );
        assert_eq!(
            WireMessage::decode(&message.encode(), DecodeLimits::WIRE),
            Ok(message)
        );
    }

    #[test]
    fn closed_envelopes_refuse_extra_and_unknown_members() {
        let refusals = [
            READ.replace("}}", "},\"debug\":true}"),
            READ.replace("file.read", "network.fetch"),
            READ.replace("[1,0]", "[1,1]"),
            READ.replace("\"request\"", "\"notice\""),
            READ.replace("\"offset\":0", "\"offset\":-1"),
        ];
        for text in refusals {
            assert_eq!(decode(&text).unwrap_err().stage, Stage::Schema, "{text}");
        }
    }

    #[test]
    fn responses_need_exactly_one_outcome_and_events_no_id() {
        let neither = r#"{"v":[1,0],"session":"s:1","kind":"response","id":"p:1"}"#;
        assert_eq!(
            decode(neither).unwrap_err().reason,
            "response-needs-one-outcome"
        );
        let ready = r#"{"v":[1,0],"session":"s:1","kind":"response","id":"h:1","result":{"status":"ready"}}"#;
        let message = decode(ready).expect("valid");
        assert_eq!(
            decode(&String::from_utf8(message.encode()).expect("utf8")),
            Ok(message)
        );
        let unknown = ready.replace("ready", "done");
        assert_eq!(decode(&unknown).unwrap_err().reason, "unknown-result-shape");
        let event = r#"{"v":[1,0],"session":"s:1","kind":"event","id":"p:1","seq":1,"method":"cancelled","params":{"invocation":"op:1"}}"#;
        assert_eq!(decode(event).unwrap_err().reason, "unknown-field");
        assert!(decode(&event.replace("\"id\":\"p:1\",", "")).is_ok());
    }
}
