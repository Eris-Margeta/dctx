//! Persisted map generation reused when content digests still match.
//! Implements DCTX-SERIOUS-GAPS G6 (CLI-WARM-REUSE) on the session cache lease.
//! Depends on Session, the index walk and `digest_text` (the `read_verified` check).
//! The CLI calls `Session::snapshot_reusable`. Isolation without a cache root does not.
//! Size and mtime are a prefilter. A same-stamp byte change still rebuilds.

use std::collections::HashMap;

use dctx_hash::ContentDigest;

use super::Session;
use crate::domain::{DctxError, FreshnessMode, SnapshotRequest};
use crate::index::{
    Generation, IndexedEdge, IndexedFile, IndexedNode, TextBlob, WalkArgs, ordinal_suffix,
    walk_source,
};

mod bytes;
mod encode;
mod entry;
mod keys;
mod reader;

use bytes::{decision_of, freshness_of};
use keys::{file_inventory, limits_match, parser_compatible};
use reader::Reader;

pub(super) use encode::encode_generation;

const REUSE_ATTEMPTS: u32 = 32;
const MAGIC: &[u8] = b"dctxgen1";
// 2: module nodes carry the export list in `signature` (DCX-55c); a version-1 cache lacks it and is rebuilt.
const VERSION: u32 = 2;
const MAX_STRING: usize = 1_048_576;
const MAX_COUNT: usize = 1_000_000;
const SYNTAX_PARSER: &str = "dctx-lex-rust/syn-3.0.9";

impl Session {
    fn try_reuse(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<Option<super::SnapshotHandle>, DctxError> {
        let (limits, parser, scan_limits) = self.reject_snapshot(request)?;
        let Some(bytes) = self.load_retained()? else {
            return Ok(None);
        };
        let Ok(decoded) = decode_generation(&bytes) else {
            return Ok(None);
        };
        if decoded.source_key != source_key
            || decoded.view != self.view
            || decoded.generation.freshness != request.freshness
            || decoded.generation.epoch != self.epoch
            || !limits_match(&decoded.limits, &limits)
            || !parser_compatible(&decoded.generation.parser, parser)
        {
            return Ok(None);
        }
        let started = self.clock.now().millis();
        crate::domain::check_deadline(started, started, scan_limits.deadline_ms)?;
        let walked = walk_source(&WalkArgs {
            source: self.source.as_ref(),
            cancel: self.cancel.as_ref(),
            policy: self.policy_view(),
            limits: &limits,
            freshness: FreshnessMode::Metadata,
            parser,
            deadline_ms: scan_limits.deadline_ms,
            started_ms: started,
            now_ms: started,
        })?;
        let Some(stored_files) = file_inventory(&decoded.generation.files) else {
            return Ok(None);
        };
        let Some(live_files) = file_inventory(&walked.files) else {
            return Ok(None);
        };
        if stored_files != live_files {
            return Ok(None);
        }
        let mut confirmed = 0u64;
        let mut verified = 0u64;
        for file in &decoded.generation.files {
            let Some(digest) = file.digest else {
                continue;
            };
            verified += 1;
            match self.digest_matches_source(&file.path, digest, limits.file_bytes) {
                Ok(true) => confirmed += 1,
                Err(
                    error @ (DctxError::Cancelled
                    | DctxError::ResourceLimit { .. }
                    | DctxError::PolicyDenied),
                ) => {
                    return Err(error);
                }
                Ok(false) | Err(_) => {
                    self.content_reads += verified;
                    return Ok(None);
                }
            }
        }
        self.cached_content_reads += confirmed;
        let encoded_len = if cfg!(feature = "segments") {
            u64::try_from(bytes.len()).unwrap_or(u64::MAX)
        } else {
            0
        };
        let mut generation = decoded.generation;
        generation.encoded_len = encoded_len;
        if cfg!(feature = "segments") {
            self.cache.segment_admit(encoded_len)?;
        }
        let now = self.clock.now().millis();
        let ticket = match self.pin_retained(now) {
            Ok(Some(ticket)) => Some(ticket),
            Ok(None) => {
                if cfg!(feature = "segments") {
                    self.cache.segment_release(encoded_len);
                }
                return Ok(None);
            }
            Err(error) => {
                if cfg!(feature = "segments") {
                    self.cache.segment_release(encoded_len);
                }
                return Err(error);
            }
        };
        Ok(Some(self.pin_generation(generation, ticket, true)))
    }
}

struct PersistedGeneration {
    source_key: String,
    view: String,
    limits: [u64; 4],
    generation: Generation,
}

#[allow(
    clippy::too_many_lines,
    reason = "Keep the reusable-generation acceptance fixture and its sequential pointer/lease assertions together."
)]
fn decode_generation(bytes: &[u8]) -> Result<PersistedGeneration, DctxError> {
    let mut reader = Reader { rest: bytes };
    let magic = reader.take(MAGIC.len())?;
    if magic != MAGIC {
        return Err(DctxError::CorruptIndex {
            reason: "generation magic",
        });
    }
    let version = reader.u32()?;
    if version != VERSION {
        return Err(DctxError::UnsupportedVersion { found: version });
    }
    let source_key = reader.string()?;
    let view = reader.string()?;
    let freshness = freshness_of(reader.u8()?)?;
    let epoch = reader.u64()?;
    let digest = reader.string()?;
    let coverage = reader.string()?;
    let completion = reader.string()?;
    let parser = reader.string()?;
    let taken_at = reader.u64()?;
    let id = reader.u64()?;
    let limits = [reader.u64()?, reader.u64()?, reader.u64()?, reader.u64()?];
    let file_count = reader.count()?;
    let mut files = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        let path = reader.string()?;
        let decision = decision_of(reader.u8()?)?;
        let digest = match reader.u8()? {
            0 => None,
            1 => {
                let raw: [u8; 32] =
                    reader
                        .take(32)?
                        .try_into()
                        .map_err(|_| DctxError::CorruptIndex {
                            reason: "generation digest",
                        })?;
                Some(ContentDigest::sha256(raw, reader.u64()?))
            }
            _ => {
                return Err(DctxError::CorruptIndex {
                    reason: "generation digest",
                });
            }
        };
        files.push(IndexedFile {
            path,
            decision,
            digest,
            size: reader.u64()?,
            modified: reader.u64()?,
        });
    }
    let mut text = TextBlob::default();
    let node_count = reader.count()?;
    let mut nodes = Vec::with_capacity(node_count);
    let mut by_id: HashMap<String, u32> = HashMap::with_capacity(node_count);
    for _ in 0..node_count {
        let id = reader.string()?;
        let kind = reader.string()?;
        let name = reader.string()?;
        let path = reader.string()?;
        let letter = id.bytes().next().unwrap_or(b'm');
        let ordinal = ordinal_suffix(&id, &name);
        let Some(index) = u32::try_from(nodes.len()).ok() else {
            return Err(DctxError::CorruptIndex {
                reason: "generation count",
            });
        };
        if let std::collections::hash_map::Entry::Vacant(slot) = by_id.entry(id) {
            slot.insert(index);
        }
        nodes.push(IndexedNode {
            letter,
            ordinal,
            kind,
            name,
            path: text.push(&path),
            summary: reader.string()?,
            summary_source: reader.string()?,
            signature: reader.string()?,
            lines: [reader.u32()?, reader.u32()?],
            span: [reader.u64()?, reader.u64()?],
        });
    }
    let edge_count = reader.count()?;
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let src = reader.string()?;
        let dst = reader.string()?;
        let edge = reader.string()?;
        let confidence = reader.string()?;
        let Some(&src) = by_id.get(&src) else {
            return Err(DctxError::CorruptIndex {
                reason: "generation edge",
            });
        };
        let Some(&dst) = by_id.get(&dst) else {
            return Err(DctxError::CorruptIndex {
                reason: "generation edge",
            });
        };
        edges.push(IndexedEdge {
            src,
            dst,
            edge: text.push(&edge),
            confidence: text.push(&confidence),
        });
    }
    if !reader.rest.is_empty() {
        return Err(DctxError::CorruptIndex {
            reason: "generation trailing",
        });
    }
    Ok(PersistedGeneration {
        source_key,
        view,
        limits,
        generation: Generation {
            id,
            digest,
            encoded_len: 0,
            files,
            nodes,
            edges,
            text,
            freshness,
            taken_at,
            epoch,
            coverage,
            completion,
            parser,
            ids: crate::index::IdIndex::default(),
        },
    })
}

#[cfg(test)]
mod tests;
