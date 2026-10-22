//! Version negotiation and hello acceptance (pure functions, no IO).
//! Implements wire.md §Bootstrap and semantics.md S-MANIFEST ranges (W-NEGOTIATE): highest common major then
//! minor, min <= max and one range per major, identity equal to the install record, required APIs offered,
//! selected extensions a subset of the offer and of the registered extension set (empty in base v1).
//! Depends on records and refusal; used by session construction and host/worker bootstrap code.

use crate::records::bootstrap::{BootstrapMessage, Hello, HelloSelected};
use crate::records::common::{ApiVersion, HookName, Identity, Limits, VersionRange};
use crate::records::failure::{ErrorCode, Phase, WireError};
use crate::records::scalars::{Name, RequestId};
use crate::refusal::{Predicate, Refusal, Stage};

/// Versions this implementation supports: exactly 1.0.
pub const SUPPORTED_VERSIONS: [VersionRange; 1] = [VersionRange { major: 1, min_minor: 0, max_minor: 0 }];

fn negotiation(code: ErrorCode, reason: &'static str) -> Refusal {
    Refusal::new(code, Stage::Session, Predicate::WNegotiate, reason)
}

/// Checks min <= max and one range per major.
/// # Errors
/// Refuses reversed minor ranges or duplicate major versions.
pub fn validate_ranges(ranges: &[VersionRange]) -> Result<(), Refusal> {
    for (index, range) in ranges.iter().enumerate() {
        if range.min_minor > range.max_minor {
            return Err(Refusal::semantic(Predicate::WNegotiate, "range-reversed"));
        }
        if ranges[..index].iter().any(|other| other.major == range.major) {
            return Err(Refusal::semantic(Predicate::WNegotiate, "duplicate-major"));
        }
    }
    Ok(())
}

/// Highest common major, then highest common minor.
/// # Errors
/// Refuses invalid ranges or an offer with no supported version in common.
pub fn select_version(offer: &[VersionRange], supported: &[VersionRange]) -> Result<ApiVersion, Refusal> {
    validate_ranges(offer)?;
    validate_ranges(supported)?;
    let mut best: Option<ApiVersion> = None;
    for ours in offer {
        for theirs in supported.iter().filter(|theirs| theirs.major == ours.major) {
            let low = ours.min_minor.max(theirs.min_minor);
            let high = ours.max_minor.min(theirs.max_minor);
            let candidate = ApiVersion { major: ours.major, minor: high };
            if low <= high && best.is_none_or(|best| candidate > best) {
                best = Some(candidate);
            }
        }
    }
    best.ok_or(negotiation(ErrorCode::UnsupportedVersion, "no-common-version"))
}

/// What a worker declares about itself when answering hello.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerProfile {
    /// Versions the worker supports.
    pub supported: Vec<VersionRange>,
    /// The worker's identity.
    pub identity: Identity,
    /// APIs it requires.
    pub required_apis: Vec<Name>,
    /// APIs it uses when offered.
    pub optional_apis: Vec<Name>,
}

/// Worker side: answers one hello with a selection or an `UnsupportedVersion` refusal.
/// # Errors
/// Propagates malformed version-range refusals; lack of a common version is encoded as a refused answer.
pub fn answer_hello(id: &RequestId, hello: &Hello, worker: &WorkerProfile) -> Result<BootstrapMessage, Refusal> {
    Ok(match select_version(&hello.versions, &worker.supported) {
        Ok(version) => BootstrapMessage::Selected {
            id: id.clone(),
            selected: Box::new(HelloSelected {
                version,
                identity: worker.identity.clone(),
                required_apis: worker.required_apis.clone(),
                optional_apis: worker.optional_apis.clone(),
                extensions: Vec::new(),
            }),
        },
        Err(refusal) if refusal.code == ErrorCode::UnsupportedVersion => BootstrapMessage::Refused {
            id: id.clone(),
            error: Box::new(WireError::refusal(refusal.code, Phase::Negotiate, Name::new(refusal.reason)?)),
        },
        Err(refusal) => return Err(refusal),
    })
}

/// The negotiated facts a session starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Negotiated {
    /// Selected version.
    pub version: ApiVersion,
    /// Verified identity (equal to the install record).
    pub identity: Identity,
    /// Usable APIs: required ones plus offered optional ones.
    pub apis: Vec<Name>,
    /// Optional APIs the host did not offer (unavailable).
    pub unavailable: Vec<Name>,
    /// Selected extensions.
    pub extensions: Vec<Name>,
    /// Offered hooks.
    pub hooks: Vec<HookName>,
    /// Host ceilings from hello.
    pub limits: Limits,
    /// Frame cap after negotiation.
    pub frame_cap: u32,
}

/// Host side: accepts a worker's answer to `hello` against the install record and extension registry.
/// # Errors
/// Refuses an unbound response, worker refusal, invalid selection, identity mismatch, or unsupported API/extension requirements.
pub fn accept_answer(
    hello_id: &RequestId,
    hello: &Hello,
    answer: &BootstrapMessage,
    install: &Identity,
    registry: &[Name],
) -> Result<Negotiated, Refusal> {
    let (id, selected) = match answer {
        BootstrapMessage::Selected { id, selected } => (id, selected),
        BootstrapMessage::Refused { id, error } if id == hello_id => {
            return Err(negotiation(error.code, "worker-refused"));
        }
        BootstrapMessage::Refused { .. } => return Err(negotiation(ErrorCode::InvalidMessage, "unknown-response")),
        BootstrapMessage::Hello { .. } => return Err(negotiation(ErrorCode::InvalidMessage, "wrong-direction")),
    };
    if id != hello_id {
        return Err(negotiation(ErrorCode::InvalidMessage, "unknown-response"));
    }
    let expected = select_version(&hello.versions, &SUPPORTED_VERSIONS)?;
    if selected.version != expected {
        return Err(negotiation(ErrorCode::UnsupportedVersion, "not-highest-common-version"));
    }
    if &selected.identity != install {
        return Err(negotiation(ErrorCode::IntegrityFailure, "identity-mismatch"));
    }
    if selected.required_apis.iter().any(|api| !hello.apis.contains(api)) {
        return Err(negotiation(ErrorCode::UnsupportedCapability, "required-api-unoffered"));
    }
    if selected.extensions.iter().any(|ext| !hello.extensions.contains(ext) || !registry.contains(ext)) {
        return Err(negotiation(ErrorCode::InvalidMessage, "unnegotiated-extension"));
    }
    let (offered, unavailable): (Vec<Name>, Vec<Name>) =
        selected.optional_apis.iter().cloned().partition(|api| hello.apis.contains(api));
    let mut apis = selected.required_apis.clone();
    apis.extend(offered);
    Ok(Negotiated {
        version: selected.version,
        identity: selected.identity.clone(),
        apis,
        unavailable,
        extensions: selected.extensions.clone(),
        hooks: hello.hooks.clone(),
        limits: hello.limits,
        frame_cap: u32::try_from(hello.limits.frame_bytes).unwrap_or(crate::frame::MAX_FRAME_CAP),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(major: u64, min_minor: u64, max_minor: u64) -> VersionRange {
        VersionRange { major, min_minor, max_minor }
    }

    #[test]
    fn selects_highest_common_major_then_minor() {
        let offer = [range(1, 0, 4), range(2, 0, 1), range(3, 2, 5)];
        let supported = [range(1, 1, 9), range(2, 1, 3), range(3, 6, 7)];
        assert_eq!(select_version(&offer, &supported), Ok(ApiVersion { major: 2, minor: 1 }));
        assert_eq!(select_version(&[range(1, 0, 0)], &SUPPORTED_VERSIONS), Ok(ApiVersion::V1_0));
        let none = select_version(&[range(2, 0, 0)], &SUPPORTED_VERSIONS).unwrap_err();
        assert_eq!((none.code, none.reason), (ErrorCode::UnsupportedVersion, "no-common-version"));
        assert!(select_version(&[range(1, 0, 3)], &[range(1, 4, 5)]).is_err());
    }

    #[test]
    fn malformed_ranges_are_refused() {
        assert_eq!(validate_ranges(&[range(1, 2, 1)]).unwrap_err().reason, "range-reversed");
        assert_eq!(validate_ranges(&[range(1, 0, 0), range(1, 1, 1)]).unwrap_err().reason, "duplicate-major");
        assert_eq!(validate_ranges(&[range(1, 0, 0), range(2, 0, 0)]), Ok(()));
    }
}
