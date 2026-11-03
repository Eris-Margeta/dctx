// SPDX-License-Identifier: Apache-2.0
//! Manifest v1 bytes: identity, segment references, and revisions.
//! Implements SEGMENTS-v1 manifest kinds 1, 2, and 5. Kind 6 is a VCS identity and is refused.
//! Depends on [`super::envelope`], [`super::bytes`], and [`super::options::Revisions`]. The store writes the result.
//! Digest order is the canonical segment order. A git object id has no field here.

use super::bytes::{push_str, structured, Cur};
use super::envelope::{self, Section, REQUIRED};
use super::error::StoreError;
use super::options::Revisions;
use crate::SegmentRole;

const MAGIC: [u8; 8] = *b"DCTXMAN\0";

/// One segment named by a manifest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentRef {
    /// `SegmentObject` digest.
    pub digest: [u8; 32],
    /// File length, in bytes.
    pub length: u64,
    /// Fact or view.
    pub role: SegmentRole,
}

/// Decoded manifest. Paths and git ids are not required to publish a generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// Trust namespace.
    pub namespace: String,
    /// Root id.
    pub root: String,
    /// Generation number.
    pub generation: u64,
    /// Previous manifest digest, when this generation has one.
    pub previous: Option<[u8; 32]>,
    /// Source snapshot digest.
    pub snapshot: [u8; 32],
    /// View digest.
    pub view: [u8; 32],
    /// Policy epoch.
    pub policy_epoch: u64,
    /// Segment references, sorted by digest.
    pub segments: Vec<SegmentRef>,
    /// Kind 5 revisions.
    pub revisions: Revisions,
}

pub(crate) fn manifest_digest(envelope: &[u8]) -> Result<[u8; 32], StoreError> {
    structured(10, envelope)
}

pub(crate) fn encode_manifest(manifest: &Manifest) -> Result<Vec<u8>, StoreError> {
    if manifest.generation == 0 {
        return Err(StoreError::InvalidRequest { reason: "generation" });
    }
    let mut identity = Vec::new();
    push_str(&mut identity, &manifest.namespace)?;
    push_str(&mut identity, &manifest.root)?;
    identity.extend_from_slice(&manifest.generation.to_le_bytes());
    match manifest.previous {
        Some(digest) => {
            identity.push(1);
            identity.extend_from_slice(&digest);
        }
        None => identity.push(0),
    }
    identity.extend_from_slice(&manifest.snapshot);
    identity.extend_from_slice(&manifest.view);
    identity.extend_from_slice(&manifest.policy_epoch.to_le_bytes());

    let mut refs = manifest.segments.clone();
    refs.sort_by_key(|segment| segment.digest);
    if refs.windows(2).any(|pair| pair[0].digest == pair[1].digest) {
        return Err(StoreError::InvalidRequest { reason: "segment" });
    }
    let mut segments = Vec::new();
    let count = u32::try_from(refs.len()).map_err(|_| StoreError::InvalidRequest { reason: "segment" })?;
    segments.extend_from_slice(&count.to_le_bytes());
    for seg in &refs {
        segments.extend_from_slice(&seg.digest);
        segments.extend_from_slice(&seg.length.to_le_bytes());
        segments.extend_from_slice(&role_code(seg.role).to_le_bytes());
        segments.extend_from_slice(&1u16.to_le_bytes());
        segments.extend_from_slice(&0u64.to_le_bytes());
    }

    let mut revisions = Vec::new();
    push_str(&mut revisions, &manifest.revisions.resolver)?;
    push_str(&mut revisions, &manifest.revisions.extraction)?;
    push_str(&mut revisions, &manifest.revisions.parser)?;
    revisions.extend_from_slice(&manifest.revisions.environment);
    revisions.extend_from_slice(&0u32.to_le_bytes());
    revisions.extend_from_slice(&0u64.to_le_bytes());

    let record_count = u64::from(count);
    envelope::assemble(
        &MAGIC,
        &[
            Section {
                kind: 1,
                flags: REQUIRED,
                record_count: 1,
                payload: identity,
            },
            Section {
                kind: 2,
                flags: REQUIRED,
                record_count,
                payload: segments,
            },
            Section {
                kind: 5,
                flags: REQUIRED,
                record_count: 1,
                payload: revisions,
            },
        ],
    )
}

pub(crate) fn decode_manifest(bytes: &[u8]) -> Result<Manifest, StoreError> {
    let sections = envelope::parse(&MAGIC, bytes)?;
    if sections.iter().any(|section| section.kind == 6) {
        return Err(StoreError::VcsRefused);
    }
    for section in &sections {
        if section.kind > 5 && section.flags & REQUIRED == REQUIRED {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
    }
    let identity = payload(&sections, 1)?;
    let segment_bytes = payload(&sections, 2)?;
    let revision_bytes = payload(&sections, 5)?;
    let mut cur = Cur::new(identity);
    let namespace = cur.text()?;
    let root = cur.text()?;
    let generation = cur.u64()?;
    let previous = match cur.u8()? {
        0 => None,
        1 => Some(cur.digest()?),
        _ => return Err(StoreError::CorruptIndex { reason: "manifest" }),
    };
    let snapshot = cur.digest()?;
    let view = cur.digest()?;
    let policy_epoch = cur.u64()?;
    cur.finish()?;
    if generation == 0 {
        return Err(StoreError::CorruptIndex { reason: "manifest" });
    }
    let mut cur = Cur::new(segment_bytes);
    let count = cur.u32()?;
    let mut segments = Vec::new();
    let mut previous_digest = None;
    for _ in 0..count {
        let digest = cur.digest()?;
        if previous_digest.is_some_and(|prev: [u8; 32]| prev >= digest) {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
        previous_digest = Some(digest);
        let length = cur.u64()?;
        let role = role_from(cur.u16()?)?;
        if cur.u16()? != 1 || cur.u64()? != 0 {
            return Err(StoreError::CorruptIndex { reason: "manifest" });
        }
        segments.push(SegmentRef { digest, length, role });
    }
    cur.finish()?;
    let mut cur = Cur::new(revision_bytes);
    let revisions = Revisions {
        resolver: cur.text()?,
        extraction: cur.text()?,
        parser: cur.text()?,
        environment: cur.digest()?,
    };
    if cur.u32()? != 0 || cur.u64()? != 0 {
        return Err(StoreError::CorruptIndex { reason: "manifest" });
    }
    cur.finish()?;
    Ok(Manifest {
        namespace,
        root,
        generation,
        previous,
        snapshot,
        view,
        policy_epoch,
        segments,
        revisions,
    })
}

fn payload(sections: &[Section], kind: u16) -> Result<&[u8], StoreError> {
    sections
        .iter()
        .find(|section| section.kind == kind)
        .map(|section| section.payload.as_slice())
        .ok_or(StoreError::CorruptIndex { reason: "manifest" })
}

fn role_code(role: SegmentRole) -> u16 {
    match role {
        SegmentRole::Fact => 1,
        SegmentRole::View => 2,
    }
}

fn role_from(code: u16) -> Result<SegmentRole, StoreError> {
    match code {
        1 => Ok(SegmentRole::Fact),
        2 => Ok(SegmentRole::View),
        _ => Err(StoreError::CorruptIndex { reason: "manifest" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            namespace: "local".into(),
            root: "main".into(),
            generation: 2,
            previous: Some([4u8; 32]),
            snapshot: [5u8; 32],
            view: [6u8; 32],
            policy_epoch: 7,
            segments: vec![SegmentRef {
                digest: [8u8; 32],
                length: 80,
                role: SegmentRole::Fact,
            }],
            revisions: Revisions::structural(),
        }
    }

    #[test]
    fn manifest_round_trips_and_kind_6_is_vcs() {
        let bytes = encode_manifest(&sample()).unwrap();
        assert_eq!(decode_manifest(&bytes).unwrap(), sample());
        assert_eq!(manifest_digest(&bytes).unwrap(), manifest_digest(&bytes).unwrap());
        let vcs = envelope::assemble(
            &MAGIC,
            &[Section {
                kind: 6,
                flags: REQUIRED,
                record_count: 1,
                payload: b"git-object".to_vec(),
            }],
        )
        .unwrap();
        assert_eq!(decode_manifest(&vcs).unwrap_err(), StoreError::VcsRefused);
    }
}
