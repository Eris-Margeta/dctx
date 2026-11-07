// SPDX-License-Identifier: Apache-2.0
//! Typed failure records: the closed 18-code error set, phases, effect states and the wire error object.
//! Implements common.schema.json `error_code`, `phase`, `effect_state`, `error` and the S-REPLAY rule that
//! partial/unknown effects are never retryable. Depends on json, scalars and refusal.
//! Used by refusal (codes), envelope (error responses), bootstrap and host/worker error reporting.

use crate::json::fields::{Fields, Obj, array, boolean, int_in, unique_list};
use crate::json::value::Json;
use crate::records::scalars::{Name, Token};
use crate::refusal::{Predicate, Refusal};

wire_enum!(
    /// Closed host/plugin error code set (wire.md, PLUGINS.md §3.2).
    ErrorCode {
        /// Authority refused the action.
        Denied => "Denied",
        /// Consent is missing for the requested scope.
        ConsentRequired => "ConsentRequired",
        /// No common protocol version.
        UnsupportedVersion => "UnsupportedVersion",
        /// A required capability is unavailable.
        UnsupportedCapability => "UnsupportedCapability",
        /// The platform cannot confine or run the plugin.
        UnsupportedPlatform => "UnsupportedPlatform",
        /// Configuration is invalid.
        InvalidConfig => "InvalidConfig",
        /// A message violated the protocol.
        InvalidMessage => "InvalidMessage",
        /// A bound or budget was exceeded.
        ResourceLimit => "ResourceLimit",
        /// A precondition or replay key conflicted.
        Conflict => "Conflict",
        /// Another invocation is active.
        Busy => "Busy",
        /// The invocation was cancelled.
        Cancelled => "Cancelled",
        /// Liveness evidence showed a stall.
        Stalled => "Stalled",
        /// The worker exited without a terminal response.
        PluginExited => "PluginExited",
        /// The external program is missing.
        ExternalProgramMissing => "ExternalProgramMissing",
        /// The external program failed.
        ExternalProgramFailed => "ExternalProgramFailed",
        /// A digest, size or signature check failed.
        IntegrityFailure => "IntegrityFailure",
        /// Durable audit is unavailable.
        AuditUnavailable => "AuditUnavailable",
        /// The outcome of an effect is not known.
        EffectUnknown => "EffectUnknown",
    }
);

wire_enum!(
    /// Lifecycle phase in which an error occurred.
    Phase {
        /// Plugin resolution.
        Resolve => "resolve",
        /// Artifact verification.
        Verify => "verify",
        /// Confinement setup.
        Confine => "confine",
        /// Version negotiation.
        Negotiate => "negotiate",
        /// Session initialization.
        Initialize => "initialize",
        /// Ready state.
        Ready => "ready",
        /// Active invocation.
        Invoke => "invoke",
        /// Brokered effect.
        Broker => "broker",
        /// Cancellation.
        Cancel => "cancel",
        /// Shutdown.
        Shutdown => "shutdown",
        /// Process reaping.
        Reap => "reap",
        /// Installation.
        Install => "install",
    }
);

wire_enum!(
    /// What is known about the effect of a failed action.
    EffectState {
        /// No effect happened.
        None => "none",
        /// The effect committed.
        Committed => "committed",
        /// Part of the effect happened.
        Partial => "partial",
        /// The outcome is unknown.
        Unknown => "unknown",
    }
);

/// Safe structured error details (no raw paths, stacks or source data).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorDetails {
    /// Safe reason name.
    pub reason: Name,
    /// Affected resource handle, if any.
    pub resource_handle: Option<Token>,
    /// Exceeded limit name, if any.
    pub limit: Option<Name>,
    /// Whether output was already committed.
    pub output_committed: Option<bool>,
    /// Whether process reaping is still pending.
    pub reap_pending: Option<bool>,
    /// Process exit code, if observed.
    pub exit_code: Option<u32>,
}

/// The wire error object carried by error responses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireError {
    /// Closed error code.
    pub code: ErrorCode,
    /// Phase in which it occurred.
    pub phase: Phase,
    /// Whether retrying is permitted at all (never for uncertain effects).
    pub retryable: bool,
    /// Known effect state.
    pub effect_state: EffectState,
    /// Host receipt references (at most 64, unique).
    pub receipt_ids: Vec<Token>,
    /// Structured safe details.
    pub details: ErrorDetails,
}

impl WireError {
    /// A refusal with no effect and no receipts.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn refusal(code: ErrorCode, phase: Phase, reason: Name) -> Self {
        let details = ErrorDetails {
            reason,
            resource_handle: None,
            limit: None,
            output_committed: None,
            reap_pending: None,
            exit_code: None,
        };
        Self {
            code,
            phase,
            retryable: false,
            effect_state: EffectState::None,
            receipt_ids: Vec::new(),
            details,
        }
    }

    /// Decodes the closed error object and applies the S-REPLAY retry rule.
    /// # Errors
    /// Refuses invalid error fields, bounds, or a retryable error with partial/unknown effects.
    pub fn decode(value: &Json) -> Result<Self, Refusal> {
        let mut fields = Fields::of(value)?;
        let error = Self {
            code: ErrorCode::decode(fields.req("code")?)?,
            phase: Phase::decode(fields.req("phase")?)?,
            retryable: boolean(fields.req("retryable")?)?,
            effect_state: EffectState::decode(fields.req("effect_state")?)?,
            receipt_ids: unique_list(fields.req("receipt_ids")?, 0, 64, Token::decode)?,
            details: decode_details(fields.req("details")?)?,
        };
        fields.finish()?;
        if error.retryable
            && matches!(
                error.effect_state,
                EffectState::Partial | EffectState::Unknown
            )
        {
            return Err(Refusal::semantic(
                Predicate::SReplay,
                "uncertain-effect-retryable",
            ));
        }
        Ok(error)
    }

    /// Encodes the closed error object.
    pub fn encode(&self) -> Json {
        let d = &self.details;
        let details = Obj::new()
            .put("reason", d.reason.encode())
            .maybe(
                "resource_handle",
                d.resource_handle.as_ref().map(Token::encode),
            )
            .maybe("limit", d.limit.as_ref().map(Name::encode))
            .maybe("output_committed", d.output_committed.map(Json::Bool))
            .maybe("reap_pending", d.reap_pending.map(Json::Bool))
            .maybe(
                "exit_code",
                d.exit_code.map(|code| Json::uint(u64::from(code))),
            )
            .build();
        Obj::new()
            .put("code", self.code.encode())
            .put("phase", self.phase.encode())
            .put("retryable", Json::Bool(self.retryable))
            .put("effect_state", self.effect_state.encode())
            .put("receipt_ids", array(&self.receipt_ids, Token::encode))
            .put("details", details)
            .build()
    }
}

fn decode_details(value: &Json) -> Result<ErrorDetails, Refusal> {
    let mut fields = Fields::of(value)?;
    let details = ErrorDetails {
        reason: Name::decode(fields.req("reason")?)?,
        resource_handle: fields
            .opt("resource_handle")
            .map(Token::decode)
            .transpose()?,
        limit: fields.opt("limit").map(Name::decode).transpose()?,
        output_committed: fields.opt("output_committed").map(boolean).transpose()?,
        reap_pending: fields.opt("reap_pending").map(boolean).transpose()?,
        exit_code: fields
            .opt("exit_code")
            .map(|v| {
                int_in(v, 0, u64::from(u32::MAX))
                    .map(|code| u32::try_from(code).unwrap_or(u32::MAX))
            })
            .transpose()?,
    };
    fields.finish()?;
    Ok(details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode::{DecodeLimits, decode_object};

    fn error(text: &str) -> Result<WireError, Refusal> {
        WireError::decode(&decode_object(text.as_bytes(), DecodeLimits::WIRE).expect("json"))
    }

    #[test]
    fn eighteen_codes_round_trip() {
        assert_eq!(ErrorCode::ALL.len(), 18);
        for code in ErrorCode::ALL {
            assert_eq!(ErrorCode::parse(code.as_str()), Some(*code));
        }
        assert_eq!(ErrorCode::parse("Teapot"), None);
        assert_eq!(Phase::ALL.len(), 12);
    }

    #[test]
    fn uncertain_effects_must_not_be_retryable() {
        let base = r#"{"code":"EffectUnknown","phase":"broker","retryable":RETRY,"effect_state":"unknown","receipt_ids":[],"details":{"reason":"completion-unrecorded"}}"#;
        assert!(error(&base.replace("RETRY", "false")).is_ok());
        let refused = error(&base.replace("RETRY", "true")).unwrap_err();
        assert_eq!(
            (refused.predicate, refused.reason),
            (Predicate::SReplay, "uncertain-effect-retryable")
        );
        let committed = base
            .replace("RETRY", "true")
            .replace("\"unknown\"", "\"none\"");
        assert!(error(&committed).is_ok());
    }

    #[test]
    fn closed_details_and_round_trip() {
        let text = r#"{"code":"PluginExited","phase":"invoke","retryable":false,"effect_state":"committed","receipt_ids":["receipt:replace"],"details":{"reason":"worker-exited","output_committed":true,"exit_code":0}}"#;
        let decoded = error(text).expect("valid");
        assert_eq!(WireError::decode(&decoded.encode()), Ok(decoded));
        assert!(error(&text.replace("\"exit_code\":0", "\"stack\":\"x\"")).is_err());
        assert!(error(&text.replace("[\"receipt:replace\"]", "[\"a\",\"a\"]")).is_err());
        let refusal = WireError::refusal(
            ErrorCode::Busy,
            Phase::Invoke,
            Name::new("busy").expect("name"),
        );
        assert_eq!(WireError::decode(&refusal.encode()), Ok(refusal));
    }
}
