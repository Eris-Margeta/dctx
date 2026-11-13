// SPDX-License-Identifier: Apache-2.0
//! Host refusal: the single typed error every broker, ledger, budget and lifecycle rule returns.
//! Implements the PLUGINS.md §3.2 error object fields (`code`, `phase`, `effect_state`, `receipt_ids`) and
//! the S-REPLAY rule that partial/unknown effects are never retryable. Depends on dctx-plugin-protocol.
//! Used by every module; `to_wire` builds the protocol `WireError` sent to the worker.

use std::fmt;

use dctx_plugin_protocol::records::failure::ErrorDetails;
use dctx_plugin_protocol::records::scalars::{Name, Token};
use dctx_plugin_protocol::{EffectState, ErrorCode, Phase, Refusal, WireError};

/// A typed host refusal carrying only static safe reason names, never paths or source bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRefusal {
    /// Closed wire error code.
    pub code: ErrorCode,
    /// Lifecycle phase that refused.
    pub phase: Phase,
    /// What is known about the effect.
    pub effect_state: EffectState,
    /// Linked host receipts (pending intent or committed effect).
    pub receipt: Option<Token>,
    /// Safe kebab-case reason.
    pub reason: &'static str,
}

impl HostRefusal {
    /// A refusal with no effect.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn new(code: ErrorCode, phase: Phase, reason: &'static str) -> Self {
        Self {
            code,
            phase,
            effect_state: EffectState::None,
            receipt: None,
            reason,
        }
    }

    /// Broker-phase `Denied`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn denied(reason: &'static str) -> Self {
        Self::new(ErrorCode::Denied, Phase::Broker, reason)
    }

    /// Broker-phase `ResourceLimit`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn limit(reason: &'static str) -> Self {
        Self::new(ErrorCode::ResourceLimit, Phase::Broker, reason)
    }

    /// Broker-phase `Conflict`.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn conflict(reason: &'static str) -> Self {
        Self::new(ErrorCode::Conflict, Phase::Broker, reason)
    }

    /// Broker-phase `AuditUnavailable` with no effect.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub const fn audit(reason: &'static str) -> Self {
        Self::new(ErrorCode::AuditUnavailable, Phase::Broker, reason)
    }

    /// `EffectUnknown` linked to the pending receipt; never retryable.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn unknown(receipt: Token, reason: &'static str) -> Self {
        Self {
            code: ErrorCode::EffectUnknown,
            phase: Phase::Broker,
            effect_state: EffectState::Unknown,
            receipt: Some(receipt),
            reason,
        }
    }

    /// Whether a caller may retry: only when nothing happened and the code is transient.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn retryable(&self) -> bool {
        self.effect_state == EffectState::None
            && matches!(self.code, ErrorCode::Busy | ErrorCode::AuditUnavailable)
    }

    /// Builds the protocol error object; refuses a reason that is not a valid wire `name`.
    /// # Errors
    /// Refuses a reason that is not a valid protocol `Name`.
    pub fn to_wire(&self) -> Result<WireError, Refusal> {
        Ok(WireError {
            code: self.code,
            phase: self.phase,
            retryable: self.retryable(),
            effect_state: self.effect_state,
            receipt_ids: self.receipt.iter().cloned().collect(),
            details: ErrorDetails {
                reason: Name::new(self.reason)?,
                resource_handle: None,
                limit: None,
                output_committed: None,
                reap_pending: None,
                exit_code: None,
            },
        })
    }
}

impl fmt::Display for HostRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}: {}; next: {}",
            self.code.as_str(),
            self.phase.as_str(),
            self.reason,
            if self.effect_state != EffectState::None {
                "reconcile the linked receipt with trusted effect evidence before taking another action"
            } else {
                match self.code {
                    ErrorCode::AuditUnavailable => "restore durable audit storage before retrying",
                    ErrorCode::ResourceLimit => {
                        "release live resources or reduce work to the admitted quota"
                    }
                    ErrorCode::Conflict => {
                        "refresh the resource precondition and check the action receipt"
                    }
                    ErrorCode::Denied | ErrorCode::ConsentRequired => {
                        "obtain current consent for the requested resource"
                    }
                    ErrorCode::UnsupportedPlatform => {
                        "use a qualified confinement provider before launching"
                    }
                    ErrorCode::UnsupportedVersion | ErrorCode::UnsupportedCapability => {
                        "select a compatible plugin and supported operation"
                    }
                    ErrorCode::IntegrityFailure => {
                        "reverify the artifact against trusted release metadata"
                    }
                    ErrorCode::Busy => "finish the active invocation before starting another",
                    ErrorCode::Cancelled | ErrorCode::Stalled | ErrorCode::PluginExited => {
                        "finish cancellation and reap the worker before starting a fresh session"
                    }
                    ErrorCode::ExternalProgramMissing => {
                        "install the registered program required by this recipe"
                    }
                    ErrorCode::ExternalProgramFailed => {
                        "inspect the recipe status and refresh its resource precondition"
                    }
                    ErrorCode::EffectUnknown => {
                        "obtain trusted effect evidence and reconcile the action receipt"
                    }
                    ErrorCode::InvalidConfig => {
                        "correct the hook plan or configuration before initialization"
                    }
                    ErrorCode::InvalidMessage => {
                        "send a valid request in the required lifecycle phase"
                    }
                }
            }
        )
    }
}

impl std::error::Error for HostRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_effects_are_never_retryable() {
        let receipt = Token::new("receipt:1").expect("token");
        let unknown = HostRefusal::unknown(receipt.clone(), "completion-log-failed");
        assert!(!unknown.retryable());
        let wire = unknown.to_wire().expect("valid reason");
        assert_eq!(wire.effect_state, EffectState::Unknown);
        assert_eq!(wire.receipt_ids, vec![receipt]);
        assert!(HostRefusal::audit("audit-full").retryable());
        assert!(!HostRefusal::denied("not-granted").retryable());
        assert_eq!(
            HostRefusal::denied("not-granted").to_string(),
            "Denied at broker: not-granted; next: obtain current consent for the requested resource"
        );
    }
}
