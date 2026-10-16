//! Typed protocol refusal: the single error every codec and session check returns.
//! Implements the validation layers of plugin-api README.md §Files and the predicate names of semantics.md/requirements.md.
//! Depends on records::failure for the closed error code set; used by every module and by host/worker callers.
//! A refusal carries only static safe names, never message bytes, paths or source data.

use std::fmt;

use crate::records::failure::ErrorCode;

/// Validation layer at which a message was refused (fixtures `expect.stage`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Length prefix, frame bounds, UTF-8 and root-object checks.
    Frame,
    /// Strict JSON grammar, duplicate keys, numbers, depth and members.
    Decode,
    /// Closed structural shape of a record.
    Schema,
    /// Cross-field rules inside one message.
    Semantic,
    /// Rules that need session state, pending requests or negotiation.
    Session,
}

impl Stage {
    /// Corpus spelling of the stage.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frame => "frame",
            Self::Decode => "decode",
            Self::Schema => "schema",
            Self::Semantic => "semantic",
            Self::Session => "session",
        }
    }
}

/// Named contract rule a refusal enforces (requirements.md predicate IDs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Predicate {
    /// W-FRAME: bounded u32 LE frames.
    WFrame,
    /// W-JSON: strict JSON grammar.
    WJson,
    /// S-BOUNDS: closed shapes, integer and string bounds.
    SBounds,
    /// W-BYTES: base64 canonical form and chunk arithmetic.
    WBytes,
    /// W-NEGOTIATE: version, identity, API and extension selection.
    WNegotiate,
    /// W-BINDING: request IDs, directions, sessions and sequences.
    WBinding,
    /// W-STATE: lifecycle state permissions.
    WState,
    /// W-RESULT: pending method selects the exact result shape.
    WResult,
    /// W-PROGRESS: event and progress sequencing.
    WProgress,
    /// W-HOOKS: declared hook boundaries.
    WHooks,
    /// W-LIMITS: outstanding work and queue bounds.
    WLimits,
    /// S-REPLAY: uncertain effects are never retryable.
    SReplay,
    /// S-RESULT: hook result rules.
    SResult,
}

impl Predicate {
    /// Corpus spelling of the predicate.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WFrame => "W-FRAME",
            Self::WJson => "W-JSON",
            Self::SBounds => "S-BOUNDS",
            Self::WBytes => "W-BYTES",
            Self::WNegotiate => "W-NEGOTIATE",
            Self::WBinding => "W-BINDING",
            Self::WState => "W-STATE",
            Self::WResult => "W-RESULT",
            Self::WProgress => "W-PROGRESS",
            Self::WHooks => "W-HOOKS",
            Self::WLimits => "W-LIMITS",
            Self::SReplay => "S-REPLAY",
            Self::SResult => "S-RESULT",
        }
    }
}

/// A typed refusal: closed error code, layer, rule and a safe kebab-case reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// Closed wire error code reported to the peer or caller.
    pub code: ErrorCode,
    /// Validation layer that refused.
    pub stage: Stage,
    /// Contract rule that refused.
    pub predicate: Predicate,
    /// Safe reason name; never contains message data.
    pub reason: &'static str,
}

impl Refusal {
    /// Builds a refusal from its parts.
    pub const fn new(code: ErrorCode, stage: Stage, predicate: Predicate, reason: &'static str) -> Self {
        Self { code, stage, predicate, reason }
    }

    /// Frame-layer InvalidMessage.
    pub(crate) const fn frame(reason: &'static str) -> Self {
        Self::new(ErrorCode::InvalidMessage, Stage::Frame, Predicate::WFrame, reason)
    }

    /// Decode-layer InvalidMessage.
    pub(crate) const fn decode(reason: &'static str) -> Self {
        Self::new(ErrorCode::InvalidMessage, Stage::Decode, Predicate::WJson, reason)
    }

    /// Schema-layer InvalidMessage.
    pub(crate) const fn schema(reason: &'static str) -> Self {
        Self::new(ErrorCode::InvalidMessage, Stage::Schema, Predicate::SBounds, reason)
    }

    /// Semantic-layer InvalidMessage for a named rule.
    pub(crate) const fn semantic(predicate: Predicate, reason: &'static str) -> Self {
        Self::new(ErrorCode::InvalidMessage, Stage::Semantic, predicate, reason)
    }

    /// Session-layer InvalidMessage for a named rule.
    pub(crate) const fn session(predicate: Predicate, reason: &'static str) -> Self {
        Self::new(ErrorCode::InvalidMessage, Stage::Session, predicate, reason)
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {} ({}): {}",
            self.code.as_str(),
            self.stage.as_str(),
            self.predicate.as_str(),
            self.reason
        )
    }
}

impl std::error::Error for Refusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_is_small_and_names_its_layer() {
        assert!(std::mem::size_of::<Refusal>() <= 32);
        let refusal = Refusal::decode("duplicate-key");
        assert_eq!(refusal.to_string(), "InvalidMessage at decode (W-JSON): duplicate-key");
    }
}
