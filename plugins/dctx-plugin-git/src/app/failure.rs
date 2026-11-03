// SPDX-License-Identifier: Apache-2.0
//! Typed invoke failures: a host error passed through, or a plugin refusal with its known effect state.
//! Implements PLUGINS.md §3.2 error objects `{code,phase,retryable,effect_state,receipt_ids,details}` and the
//! S-REPLAY rule (partial/unknown effects are never retryable). Depends on dctx-plugin-protocol.
//! Used by every app module; `adapters::worker` converts it to the wire error.

use dctx_plugin_protocol::records::failure::ErrorDetails;
use dctx_plugin_protocol::records::scalars::{Name, Token};
use dctx_plugin_protocol::{EffectState, ErrorCode, Phase, Refusal, WireError};

use crate::ports::BrokerFailure;

/// An invoke that ends with an error response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Closed error code.
    pub code: ErrorCode,
    /// Phase.
    pub phase: Phase,
    /// Safe reason name.
    pub reason: &'static str,
    /// What is known about effects already requested.
    pub effect: EffectState,
    /// Receipts of committed effects.
    pub receipts: Vec<Token>,
    /// Exceeded limit name, if any.
    pub limit: Option<&'static str>,
    /// A host error to pass through unchanged (its own details and receipts).
    pub host: Option<Box<WireError>>,
}

impl Failure {
    /// A refusal before any effect.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn refused(code: ErrorCode, phase: Phase, reason: &'static str) -> Self {
        Self { code, phase, reason, effect: EffectState::None, receipts: Vec::new(), limit: None, host: None }
    }

    /// A refusal for an exceeded bound.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn limit(reason: &'static str, limit: &'static str) -> Self {
        Self { limit: Some(limit), ..Self::refused(ErrorCode::ResourceLimit, Phase::Invoke, reason) }
    }

    /// Converts a broker failure. `pending_mutation` marks a request whose effect may have happened;
    /// `receipts` are the effects already committed in this invocation.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn from_broker(failure: BrokerFailure, pending_mutation: bool, receipts: &[Token]) -> Self {
        let settled = if receipts.is_empty() { EffectState::None } else { EffectState::Committed };
        let uncertain = if pending_mutation { EffectState::Unknown } else { settled };
        let with = |code, reason, effect| Self {
            code,
            phase: Phase::Broker,
            reason,
            effect,
            receipts: receipts.to_vec(),
            limit: None,
            host: None,
        };
        match failure {
            BrokerFailure::Cancelled => Self { phase: Phase::Cancel, ..with(ErrorCode::Cancelled, "cancelled", uncertain) },
            BrokerFailure::Protocol(reason) if pending_mutation => with(ErrorCode::EffectUnknown, reason, uncertain),
            BrokerFailure::Protocol(reason) => with(ErrorCode::InvalidMessage, reason, settled),
            BrokerFailure::Refused(error) if receipts.is_empty() => {
                Self { host: Some(error.clone()), ..with(error.code, "host-refused", error.effect_state) }
            }
            BrokerFailure::Refused(error) => {
                let effect = match error.effect_state {
                    EffectState::None => EffectState::Committed,
                    other => other,
                };
                with(error.code, "host-refused", effect)
            }
        }
    }

    /// The wire error object; uncertain effects are never retryable.
    /// # Errors
    /// Refuses reason/limit names outside the protocol scalar format.
    pub fn to_wire(&self) -> Result<WireError, Refusal> {
        if let Some(host) = &self.host {
            return Ok(WireError { retryable: false, ..(**host).clone() });
        }
        Ok(WireError {
            code: self.code,
            phase: self.phase,
            retryable: false,
            effect_state: self.effect,
            receipt_ids: self.receipts.clone(),
            details: ErrorDetails {
                reason: Name::new(self.reason)?,
                resource_handle: None,
                limit: self.limit.map(Name::new).transpose()?,
                output_committed: None,
                reap_pending: None,
                exit_code: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str) -> Token {
        Token::new(text).expect("token")
    }

    #[test]
    fn effect_state_follows_receipts_and_pending_mutations() {
        let none = Failure::from_broker(BrokerFailure::Protocol("broken-link"), false, &[]);
        assert_eq!((none.code, none.effect), (ErrorCode::InvalidMessage, EffectState::None));
        let unknown = Failure::from_broker(BrokerFailure::Protocol("broken-link"), true, &[token("receipt:replace")]);
        assert_eq!((unknown.code, unknown.effect), (ErrorCode::EffectUnknown, EffectState::Unknown));
        let cancelled = Failure::from_broker(BrokerFailure::Cancelled, false, &[token("receipt:replace")]);
        assert_eq!((cancelled.code, cancelled.effect), (ErrorCode::Cancelled, EffectState::Committed));
        let wire = unknown.to_wire().expect("wire");
        assert!(!wire.retryable);
        assert_eq!(wire.receipt_ids, vec![token("receipt:replace")]);
    }

    #[test]
    fn host_refusals_pass_through_until_an_effect_committed() {
        let denied = WireError::refusal(ErrorCode::Denied, Phase::Broker, Name::new("selection-denied").expect("name"));
        let failure = Failure::from_broker(BrokerFailure::Refused(Box::new(denied.clone())), false, &[]);
        assert_eq!(failure.to_wire(), Ok(denied.clone()));
        let after = Failure::from_broker(BrokerFailure::Refused(Box::new(denied)), false, &[token("r:1")]);
        assert_eq!((after.code, after.effect), (ErrorCode::Denied, EffectState::Committed));
    }
}
