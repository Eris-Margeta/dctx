// SPDX-License-Identifier: Apache-2.0
use super::{MAX_STRING, records_cap};
use crate::domain::{DctxError, FreshnessMode};
use dctx_core::FileHandlingDecision;

pub(super) fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

pub(super) fn push_count(out: &mut Vec<u8>, count: usize) -> Result<(), DctxError> {
    let cap = records_cap();
    if count > cap {
        return Err(DctxError::ResourceLimit {
            resource: crate::domain::Resource::Records,
            limit: cap as u64,
        });
    }
    let raw = u64::try_from(count).map_err(|_| DctxError::ResourceLimit {
        resource: crate::domain::Resource::Records,
        limit: cap as u64,
    })?;
    push_u64(out, raw);
    Ok(())
}

pub(super) fn push_str(out: &mut Vec<u8>, text: &str) -> Result<(), DctxError> {
    if text.len() > MAX_STRING {
        return Err(DctxError::ResourceLimit {
            resource: crate::domain::Resource::PathBytes,
            limit: MAX_STRING as u64,
        });
    }
    let len = u32::try_from(text.len()).map_err(|_| DctxError::ResourceLimit {
        resource: crate::domain::Resource::PathBytes,
        limit: MAX_STRING as u64,
    })?;
    push_u32(out, len);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

pub(super) fn freshness_byte(mode: FreshnessMode) -> u8 {
    match mode {
        FreshnessMode::ImmutableSnapshot => 0,
        FreshnessMode::Strict => 1,
        FreshnessMode::Metadata => 2,
    }
}

pub(super) fn freshness_of(byte: u8) -> Result<FreshnessMode, DctxError> {
    match byte {
        0 => Ok(FreshnessMode::ImmutableSnapshot),
        1 => Ok(FreshnessMode::Strict),
        2 => Ok(FreshnessMode::Metadata),
        _ => Err(DctxError::CorruptIndex {
            reason: "generation freshness",
        }),
    }
}

pub(super) fn decision_byte(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 0,
        FileHandlingDecision::Exclude => 1,
        FileHandlingDecision::OmitContent => 2,
        FileHandlingDecision::DebugVisible => 3,
    }
}

pub(super) fn decision_of(byte: u8) -> Result<FileHandlingDecision, DctxError> {
    match byte {
        0 => Ok(FileHandlingDecision::Include),
        1 => Ok(FileHandlingDecision::Exclude),
        2 => Ok(FileHandlingDecision::OmitContent),
        3 => Ok(FileHandlingDecision::DebugVisible),
        _ => Err(DctxError::CorruptIndex {
            reason: "generation decision",
        }),
    }
}
