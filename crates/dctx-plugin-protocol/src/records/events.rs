// SPDX-License-Identifier: Apache-2.0
//! Plugin events: progress samples and the cancelled acknowledgement (no response is ever sent).
//! Implements events.schema.json; sequencing rules live in `session::progress` (W-PROGRESS).
//! Depends on json, common, scalars, refusal; used by envelope and session.

use crate::json::fields::{Fields, Obj, positive, uint};
use crate::json::value::Json;
use crate::records::common::Wait;
use crate::records::scalars::{Name, Token};
use crate::refusal::Refusal;

wire_enum!(
    /// Unit of a progress count; stable within a phase.
    ProgressUnit {
        /// Bytes.
        Bytes => "bytes",
        /// Items.
        Items => "items",
        /// Broker actions.
        Actions => "actions",
        /// Caller work units.
        WorkUnits => "work-units",
    }
);

/// One sanitized progress sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Active invocation.
    pub invocation: Token,
    /// Per-invocation sequence (starts at 1, strictly increasing).
    pub seq: u64,
    /// Opaque bounded phase code.
    pub phase: Name,
    /// Completed count (monotone within the phase).
    pub completed: u64,
    /// Total, when known (stable within the phase).
    pub total: Option<u64>,
    /// Unit (stable within the phase).
    pub unit: ProgressUnit,
    /// Declared wait.
    pub wait: Option<Wait>,
}

/// A typed event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// `progress`.
    Progress(Progress),
    /// `cancelled`.
    Cancelled {
        /// Cancelled invocation.
        invocation: Token,
        /// Last committed receipt reference.
        last_receipt: Option<Token>,
    },
}

impl Event {
    /// Wire method name.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn method(&self) -> &'static str {
        match self {
            Self::Progress(_) => "progress",
            Self::Cancelled { .. } => "cancelled",
        }
    }

    /// Invocation the event belongs to.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn invocation(&self) -> &Token {
        match self {
            Self::Progress(progress) => &progress.invocation,
            Self::Cancelled { invocation, .. } => invocation,
        }
    }

    pub(crate) fn decode(method: &str, params: &Json) -> Result<Self, Refusal> {
        let mut f = Fields::of(params)?;
        let event = match method {
            "progress" => Self::Progress(Progress {
                invocation: Token::decode(f.req("invocation")?)?,
                seq: positive(f.req("seq")?)?,
                phase: Name::decode(f.req("phase")?)?,
                completed: uint(f.req("completed")?)?,
                total: f.opt("total").map(uint).transpose()?,
                unit: ProgressUnit::decode(f.req("unit")?)?,
                wait: f.opt("wait").map(Wait::decode).transpose()?,
            }),
            "cancelled" => Self::Cancelled {
                invocation: Token::decode(f.req("invocation")?)?,
                last_receipt: f.opt("last_receipt").map(Token::decode).transpose()?,
            },
            _ => return Err(Refusal::schema("unknown-event")),
        };
        f.finish()?;
        Ok(event)
    }

    pub(crate) fn encode_params(&self) -> Json {
        match self {
            Self::Progress(p) => Obj::new()
                .put("invocation", p.invocation.encode())
                .put("seq", Json::uint(p.seq))
                .put("phase", p.phase.encode())
                .put("completed", Json::uint(p.completed))
                .maybe("total", p.total.map(Json::uint))
                .put("unit", p.unit.encode())
                .maybe("wait", p.wait.as_ref().map(Wait::encode))
                .build(),
            Self::Cancelled {
                invocation,
                last_receipt,
            } => Obj::new()
                .put("invocation", invocation.encode())
                .maybe("last_receipt", last_receipt.as_ref().map(Token::encode))
                .build(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    #[test]
    fn events_round_trip_and_refuse_unknown_kinds() {
        let text = r#"{"invocation":"op:1","seq":1,"phase":"read-ignore","completed":3,"total":3,"unit":"bytes","wait":{"dependency":"d:1"}}"#;
        let params = decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json");
        let event = Event::decode("progress", &params).expect("valid");
        assert_eq!(event.method(), "progress");
        assert_eq!(event.invocation().as_str(), "op:1");
        assert_eq!(Event::decode("progress", &event.encode_params()), Ok(event));
        assert!(Event::decode("heartbeat", &params).is_err());
        let zero = decode_object(
            text.replace("\"seq\":1", "\"seq\":0").as_bytes(),
            DecodeLimits::WIRE,
        )
        .expect("json");
        assert!(Event::decode("progress", &zero).is_err());
    }
}
