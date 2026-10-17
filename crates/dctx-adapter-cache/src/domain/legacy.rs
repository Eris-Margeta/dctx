//! Legacy export classification. Dry-run is a pure function of the records the caller already holds.
//! Implements CACHE-v1 legacy safety: extension, place, version and ownership are all required.
//! Depends on nothing in the ledger except the adopted-export map for apply. GC does not call this.
//! Unregistered, modified, unknown-version and arbitrary paths stay untouched.

use super::quota::RECOGNIZED_LEGACY_VERSION;
use super::restart::{Put, Take};
use super::Lifecycle;
use crate::error::CacheError;
use std::collections::BTreeMap;

/// Where the caller found the record. Arbitrary locations are never candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPlace {
    /// The managed root registry.
    Registry,
    /// A configured output path.
    Configured,
    /// Somewhere else. Dry-run leaves it alone.
    Arbitrary,
}

/// What the caller already read. This crate does not open the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportEvidence {
    /// A recognizable legacy binary at the one version this crate names.
    OwnedBinary {
        /// Header version.
        version: u32,
        /// Identity recorded when dctx wrote it.
        identity: u64,
    },
    /// Generated text with the dctx header and an ownership record.
    OwnedText {
        /// Identity recorded when dctx wrote it.
        identity: u64,
    },
    /// Generated diff with the dctx header and an ownership record.
    OwnedDiff {
        /// Identity recorded when dctx wrote it.
        identity: u64,
    },
    /// Bytes no longer match the ownership record.
    Modified {
        /// Identity the file used to have.
        identity: u64,
    },
    /// A binary whose version this crate does not recognize.
    UnknownVersion {
        /// Header version.
        version: u32,
    },
    /// A name that merely ends in a known suffix.
    ExtensionOnly,
    /// No ownership record.
    Unregistered,
}

/// One record from the registry or a configured output list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownExport {
    /// Where it was found.
    pub place: OutputPlace,
    /// Caller label. Not opened, and not used as a search root.
    pub label: String,
    /// Size in bytes, as already measured by the caller.
    pub bytes: u64,
    /// Classification of bytes the caller already read.
    pub evidence: ExportEvidence,
}

impl KnownExport {
    /// Ownership identity, when the evidence carries one.
    pub fn identity(&self) -> Option<u64> {
        match self.evidence {
            ExportEvidence::OwnedBinary { identity, .. }
            | ExportEvidence::OwnedText { identity }
            | ExportEvidence::OwnedDiff { identity }
            | ExportEvidence::Modified { identity } => Some(identity),
            ExportEvidence::UnknownVersion { .. }
            | ExportEvidence::ExtensionOnly
            | ExportEvidence::Unregistered => None,
        }
    }
}

/// Why a record is not a deletion candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaveReason {
    /// The suffix is not ownership.
    ExtensionOnly,
    /// No ownership record.
    Unregistered,
    /// Bytes changed since the ownership record.
    Modified,
    /// The binary version is not one this crate can name.
    UnknownVersion,
    /// The place is not the registry or a configured output.
    NotConfiguredPlace,
    /// Selected, but never adopted into the ownership manifest.
    NotAdopted,
    /// Adopted identity does not match the fresh evidence.
    IdentityChanged,
}

/// A recognizable owned artifact at a configured place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyCandidate {
    /// Caller label.
    pub label: String,
    /// Ownership identity.
    pub identity: u64,
    /// Bytes the caller measured.
    pub bytes: u64,
}

/// A record dry-run will not touch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntouchedExport {
    /// Caller label, when there was one.
    pub label: String,
    /// Identity, when the evidence had one.
    pub identity: Option<u64>,
    /// Why it stays.
    pub reason: LeaveReason,
}

/// Plan from a read-only legacy inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyDryRun {
    /// Owned, unmodified, known-version records at configured places.
    pub candidates: Vec<LegacyCandidate>,
    /// Everything else that was shown to the inspector.
    pub left_alone: Vec<UntouchedExport>,
}

/// One adopted export removed by an explicit apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimedExport {
    /// Caller label.
    pub label: String,
    /// Bytes removed from the ownership manifest.
    pub bytes: u64,
}

/// Result of an explicit legacy apply. Untouched entries were not removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyApplyReport {
    /// Adopted candidates whose fresh evidence still matches.
    pub reclaimed: Vec<ReclaimedExport>,
    /// Selections that were refused.
    pub untouched: Vec<UntouchedExport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AdoptedExport {
    pub bytes: u64,
    pub identity: u64,
}

/// Classifies one record. A candidate needs place, ownership, a known version and unchanged bytes.
pub fn classify_export(export: &KnownExport) -> Result<u64, LeaveReason> {
    if !matches!(export.place, OutputPlace::Registry | OutputPlace::Configured) {
        return Err(LeaveReason::NotConfiguredPlace);
    }
    match export.evidence {
        ExportEvidence::OwnedBinary { version, identity }
            if version == RECOGNIZED_LEGACY_VERSION =>
        {
            Ok(identity)
        }
        ExportEvidence::OwnedText { identity } | ExportEvidence::OwnedDiff { identity } => {
            Ok(identity)
        }
        ExportEvidence::OwnedBinary { .. } | ExportEvidence::UnknownVersion { .. } => {
            Err(LeaveReason::UnknownVersion)
        }
        ExportEvidence::Modified { .. } => Err(LeaveReason::Modified),
        ExportEvidence::ExtensionOnly => Err(LeaveReason::ExtensionOnly),
        ExportEvidence::Unregistered => Err(LeaveReason::Unregistered),
    }
}

impl Lifecycle {
    /// Read-only plan. Does not retire, unlink, touch last-use, or move the cursor.
    pub fn legacy_dry_run(&self, exports: &[KnownExport]) -> LegacyDryRun {
        let mut candidates = Vec::new();
        let mut left_alone = Vec::new();
        for export in exports {
            match classify_export(export) {
                Ok(identity) => candidates.push(LegacyCandidate {
                    label: export.label.clone(),
                    identity,
                    bytes: export.bytes,
                }),
                Err(reason) => left_alone.push(UntouchedExport {
                    label: export.label.clone(),
                    identity: export.identity(),
                    reason,
                }),
            }
        }
        LegacyDryRun { candidates, left_alone }
    }

    /// Opts an already-classified candidate into the ownership manifest.
    pub fn adopt_legacy(&mut self, export: &KnownExport) -> Result<(), CacheError> {
        let identity = classify_export(export).map_err(|_| CacheError::InvalidRequest {
            reason: "export is not an owned legacy artifact",
        })?;
        self.exports.insert(export.label.clone(), AdoptedExport { bytes: export.bytes, identity });
        self.bump();
        Ok(())
    }

    /// Removes adopted records whose fresh evidence is still a candidate. Everything else stays.
    pub fn legacy_apply(
        &mut self,
        exports: &[KnownExport],
        selected: &[u64],
    ) -> Result<LegacyApplyReport, CacheError> {
        let mut reclaimed = Vec::new();
        let mut untouched = Vec::new();
        for identity in selected {
            let Some(export) = exports.iter().find(|export| export.identity() == Some(*identity)) else {
                untouched.push(UntouchedExport {
                    label: String::new(),
                    identity: Some(*identity),
                    reason: LeaveReason::Unregistered,
                });
                continue;
            };
            if let Err(reason) = classify_export(export) {
                untouched.push(UntouchedExport {
                    label: export.label.clone(),
                    identity: export.identity(),
                    reason,
                });
                continue;
            }
            let Some(stored) = self.exports.get(&export.label) else {
                untouched.push(UntouchedExport {
                    label: export.label.clone(),
                    identity: Some(*identity),
                    reason: LeaveReason::NotAdopted,
                });
                continue;
            };
            if stored.identity != *identity {
                untouched.push(UntouchedExport {
                    label: export.label.clone(),
                    identity: Some(*identity),
                    reason: LeaveReason::IdentityChanged,
                });
                continue;
            }
            let bytes = stored.bytes;
            self.exports.remove(&export.label);
            self.touches = self.touches.saturating_add(1);
            self.bump();
            reclaimed.push(ReclaimedExport { label: export.label.clone(), bytes });
        }
        Ok(LegacyApplyReport { reclaimed, untouched })
    }

    /// Labels adopted into the ownership manifest.
    pub fn adopted_labels(&self) -> Vec<String> {
        self.exports.keys().cloned().collect()
    }
}

pub(super) fn write_exports(life: &Lifecycle, out: &mut Put) -> Result<(), CacheError> {
    out.count(life.exports.len())?;
    for (label, export) in &life.exports {
        out.text(label)?;
        out.u64(export.bytes);
        out.u64(export.identity);
    }
    Ok(())
}

pub(super) fn read_exports(
    inp: &mut Take<'_>,
) -> Result<BTreeMap<String, AdoptedExport>, CacheError> {
    let mut exports = BTreeMap::new();
    for _ in 0..inp.count()? {
        let label = inp.text()?;
        let export = AdoptedExport { bytes: inp.u64()?, identity: inp.u64()? };
        if exports.insert(label, export).is_some() {
            return Err(super::restart::corrupt("duplicate"));
        }
    }
    Ok(exports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(place: OutputPlace, evidence: ExportEvidence) -> KnownExport {
        KnownExport { place, label: "out".to_string(), bytes: 4, evidence }
    }

    #[test]
    fn extension_and_arbitrary_place_are_not_ownership() {
        let extension = export(OutputPlace::Registry, ExportEvidence::ExtensionOnly);
        assert_eq!(classify_export(&extension), Err(LeaveReason::ExtensionOnly));
        let owned = ExportEvidence::OwnedBinary { version: RECOGNIZED_LEGACY_VERSION, identity: 9 };
        assert_eq!(
            classify_export(&export(OutputPlace::Arbitrary, owned)),
            Err(LeaveReason::NotConfiguredPlace)
        );
        assert_eq!(classify_export(&export(OutputPlace::Registry, owned)), Ok(9));
    }
}
