// SPDX-License-Identifier: Apache-2.0
//! Requested capability sets and digest-bound consent records.
//! Implements PLUGINS.md §4.1/§5.1 "reject capability expansion without new consent; existing consent may
//! cover a verified equal/narrower update" and requirements I-CONSENT. Pure; depends on the protocol's
//! canonical JSON encoder. Used by manifest parsing, the registry and the management service.
//! Entries compare by exact canonical bytes: any changed field, even a smaller bound, counts as new.

use std::collections::BTreeSet;

use dctx_plugin_protocol::{DecodeLimits, Json, decode_object, to_canonical};

use super::digest::{Fingerprint, Sha256Hex};
use super::fields::Closed;
use crate::error::{DistributionError, Document};

const SECTIONS: [&str; 4] = ["required", "optional", "outputs", "network"];

/// Capability entries as `(section, canonical entry bytes)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    entries: BTreeSet<(String, Vec<u8>)>,
}

impl CapabilitySet {
    /// Reads the manifest's closed `capabilities` object.
    /// # Errors
    /// Refuses missing/extra capability sections, invalid capability objects, or duplicates.
    pub fn from_manifest(value: &Json) -> Result<Self, DistributionError> {
        let object = Closed::new(value, Document::Manifest, &SECTIONS, &SECTIONS)?;
        let mut entries = BTreeSet::new();
        for section in SECTIONS {
            for entry in object.array(section, 0, 256)? {
                entry.as_object().ok_or(object.refuse("capability"))?;
                if !entries.insert((section.to_owned(), to_canonical(entry))) {
                    return Err(object.refuse("duplicate-capability"));
                }
            }
        }
        Ok(Self { entries })
    }

    /// True when every entry here is also in `granted`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_within(&self, granted: &Self) -> bool {
        self.entries.is_subset(&granted.entries)
    }

    /// Entries in `self` that `granted` does not contain, as display-ready canonical JSON.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn expansion_over(&self, granted: &Self) -> Vec<(String, String)> {
        self.entries
            .difference(&granted.entries)
            .map(|(section, bytes)| (section.clone(), String::from_utf8_lossy(bytes).into_owned()))
            .collect()
    }

    /// Number of entries.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is requested.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn to_json(&self) -> Json {
        let items = self
            .entries
            .iter()
            .map(|(section, bytes)| {
                let entry = decode_object(bytes, DecodeLimits::MANIFEST).unwrap_or(Json::Object(Vec::new()));
                Json::Object(vec![("section".to_owned(), Json::str(section)), ("entry".to_owned(), entry)])
            })
            .collect();
        Json::Array(items)
    }

    pub(crate) fn from_json(value: &Json) -> Result<Self, DistributionError> {
        let refuse = DistributionError::malformed(Document::Registry, "capabilities");
        let mut entries = BTreeSet::new();
        for item in value.as_array().ok_or(refuse)? {
            let item = Closed::new(item, Document::Registry, &["section", "entry"], &["section", "entry"])?;
            let section = item.str("section")?;
            if !SECTIONS.contains(&section) {
                return Err(refuse);
            }
            entries.insert((section.to_owned(), to_canonical(item.object("entry")?)));
        }
        Ok(Self { entries })
    }
}

/// Where a consent applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsentScope {
    /// Every project of this user.
    User,
    /// One project root, as the caller names it.
    Project(String),
}

/// Consent bound to a signer, an exact artifact digest and a capability set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentRecord {
    /// Artifact digest the consent was recorded or derived for.
    pub digest: Sha256Hex,
    /// Signer fingerprint at consent time.
    pub signer: Fingerprint,
    /// Granted capability entries.
    pub capabilities: CapabilitySet,
    /// Scope of the consent.
    pub scope: ConsentScope,
}

impl ConsentRecord {
    /// True when a release by `signer` requesting `requested` needs no new consent.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn covers(&self, signer: &Fingerprint, requested: &CapabilitySet) -> bool {
        self.signer == *signer && requested.is_within(&self.capabilities)
    }

    /// The record derived for an equal/narrower update, or `None` when consent must be asked again.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn derive(&self, digest: Sha256Hex, signer: &Fingerprint, requested: &CapabilitySet) -> Option<Self> {
        self.covers(signer, requested).then(|| Self { digest, ..self.clone() })
    }
}

/// Explicit capability approval given by the caller for one exact digest (`--approve` or a UI prompt).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentApproval {
    /// Digest shown in the consent display.
    pub digest: Sha256Hex,
    /// Capabilities shown and approved.
    pub capabilities: CapabilitySet,
    /// Approved scope.
    pub scope: ConsentScope,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(text: &str) -> CapabilitySet {
        CapabilitySet::from_manifest(&decode_object(text.as_bytes(), DecodeLimits::MANIFEST).unwrap()).unwrap()
    }

    const NARROW: &str = r#"{"required":[{"kind":"file.read","path":".gitignore"}],"optional":[],"outputs":[],"network":[]}"#;
    const WIDE: &str = r#"{"required":[{"path":".gitignore","kind":"file.read"}],"optional":[{"kind":"program.run","program":"git"}],"outputs":[],"network":[]}"#;

    #[test]
    fn member_order_does_not_change_identity_but_new_entries_expand() {
        let narrow = caps(NARROW);
        let wide = caps(WIDE);
        assert!(narrow.is_within(&wide));
        assert!(!wide.is_within(&narrow));
        assert_eq!(wide.expansion_over(&narrow).len(), 1);
        assert_eq!(CapabilitySet::from_json(&wide.to_json()).unwrap(), wide);
    }

    #[test]
    fn consent_covers_only_same_signer_and_no_expansion() {
        let digest = Sha256Hex::from_bytes([1; 32]);
        let signer = Sha256Hex::from_bytes([2; 32]);
        let record = ConsentRecord { digest, signer, capabilities: caps(WIDE), scope: ConsentScope::User };
        assert!(record.covers(&signer, &caps(NARROW)));
        assert!(!record.covers(&Sha256Hex::from_bytes([3; 32]), &caps(NARROW)));
        let derived = record.derive(Sha256Hex::from_bytes([4; 32]), &signer, &caps(NARROW)).unwrap();
        assert_eq!(derived.digest, Sha256Hex::from_bytes([4; 32]));
        let narrow_record = ConsentRecord { capabilities: caps(NARROW), ..record };
        assert!(narrow_record.derive(digest, &signer, &caps(WIDE)).is_none());
    }

    #[test]
    fn unknown_sections_and_duplicates_are_refused() {
        let bytes = br#"{"required":[],"optional":[],"outputs":[],"network":[],"exec":[]}"#;
        assert!(CapabilitySet::from_manifest(&decode_object(bytes, DecodeLimits::MANIFEST).unwrap()).is_err());
        let dup = br#"{"required":[{"kind":"a"},{"kind":"a"}],"optional":[],"outputs":[],"network":[]}"#;
        assert!(CapabilitySet::from_manifest(&decode_object(dup, DecodeLimits::MANIFEST).unwrap()).is_err());
    }
}
