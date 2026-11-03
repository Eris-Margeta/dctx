// SPDX-License-Identifier: Apache-2.0
use super::*;

pub(super) fn prepare(blobs: &[SegmentBlob]) -> Result<Vec<Prepared>, StoreError> {
    let mut prepared = Vec::new();
    for blob in blobs {
        decode(&blob.bytes, blob.role).map_err(|_| StoreError::CorruptIndex { reason: "segment" })?;
        let digest = segment_object_digest(&blob.bytes).map_err(|_| StoreError::CorruptIndex {
            reason: "segment",
        })?;
        if let Some(existing) = prepared.iter().find(|item: &&Prepared| item.digest == digest) {
            if existing.bytes != blob.bytes {
                return Err(StoreError::CorruptIndex { reason: "segment" });
            }
            continue;
        }
        prepared.push(Prepared {
            digest,
            role: blob.role,
            bytes: blob.bytes.clone(),
        });
    }
    prepared.sort_by_key(|left| left.digest);
    Ok(prepared)
}

pub(super) fn build_manifest(
    options: &StoreOptions,
    request: &PublishRequest,
    prepared: &[Prepared],
    previous: Option<[u8; 32]>,
) -> Manifest {
    Manifest {
        namespace: options.namespace.clone(),
        root: request.root.clone(),
        generation: request.generation,
        previous,
        snapshot: request.snapshot,
        view: request.view,
        policy_epoch: request.policy_epoch,
        segments: prepared
            .iter()
            .map(|segment| SegmentRef {
                digest: segment.digest,
                length: u64::try_from(segment.bytes.len()).unwrap_or(u64::MAX),
                role: segment.role,
            })
            .collect(),
        revisions: request.revisions.clone(),
    }
}
