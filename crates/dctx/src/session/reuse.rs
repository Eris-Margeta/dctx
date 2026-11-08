// SPDX-License-Identifier: Apache-2.0
//! Persisted map generation reused when content digests still match.
//! Implements DCTX-SERIOUS-GAPS G6 (CLI-WARM-REUSE) on the session cache lease.
//! Depends on Session, the index walk and `digest_text` (the `read_verified` check).
//! The CLI calls `Session::snapshot_reusable`. Isolation without a cache root does not.
//! Size and mtime are a prefilter. A same-stamp byte change still rebuilds, except where the stat
//! stamp (size, mtime ns, inode) of a settled file is equal: that file is trusted without a read.
//! `DCTX_FRESHNESS=strict_live` turns that trust off and hashes every file.

use std::collections::HashMap;

use dctx_core::facts::EntryStamp;
use dctx_hash::ContentDigest;

use super::Session;
use crate::domain::{DctxError, FreshnessMode, SnapshotRequest};
use crate::index::{
    Generation, IndexedEdge, IndexedFile, IndexedNode, TextBlob, WalkArgs, walk_source,
};

mod bytes;
mod encode;
mod entry;
mod keys;
mod reader;
#[cfg(feature = "warm-index-cache")]
mod warm;

use bytes::{decision_of, freshness_of};
use keys::{file_inventory, limits_match, parser_compatible};
use reader::Reader;

pub(super) use encode::encode_generation;

const REUSE_ATTEMPTS: u32 = 32;
const MAGIC: &[u8] = b"dctxgen1";
// 2: module nodes carry the export list in `signature` (DCX-55c); a version-1 cache lacks it and is rebuilt.
// 3: each file row ends with its stat stamp (DCX-P-C5); a version-2 cache lacks it and is rebuilt.
// 4: scoped queries retain per-file parser and completion provenance.
// 5: interned strings, structural node ids and numeric edge endpoints; older caches rebuild.
const VERSION: u32 = 5;
const MAX_STRING: usize = 1_048_576;
const MAX_COUNT: usize = 1_000_000;
const SYNTAX_PARSER: &str = "dctx-lex-rust/syn-3.0.9";
/// Environment variable that picks the freshness rule of a warm start.
pub const FRESHNESS_ENV: &str = "DCTX_FRESHNESS";

/// Records one generation may carry: `DCTX_MAX_RECORDS` when valid, otherwise 1,000,000.
fn records_cap() -> usize {
    crate::domain::records_ceiling()
        .ok()
        .and_then(|cap| usize::try_from(cap).ok())
        .unwrap_or(MAX_COUNT)
}

/// True when a warm start may trust an equal stat stamp. `DCTX_FRESHNESS=strict_live` hashes every file instead.
fn stamp_pass_enabled() -> Result<bool, DctxError> {
    match std::env::var(FRESHNESS_ENV) {
        Ok(text) => stamp_pass_mode(Some(&text)),
        Err(std::env::VarError::NotPresent) => stamp_pass_mode(None),
        Err(std::env::VarError::NotUnicode(_)) => stamp_pass_mode(Some("not unicode")),
    }
}

fn stamp_pass_mode(raw: Option<&str>) -> Result<bool, DctxError> {
    match raw.map(str::trim) {
        None | Some("" | "stamp") => Ok(true),
        Some("strict_live") => Ok(false),
        Some(other) => Err(DctxError::invalid_detail(
            "DCTX_FRESHNESS must be `stamp` or `strict_live`",
            other,
        )),
    }
}

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
        #[cfg(feature = "warm-index-cache")]
        let decoded = warm::decode(&bytes);
        #[cfg(not(feature = "warm-index-cache"))]
        let decoded = decode_generation(&bytes);
        let Ok(decoded) = decoded else {
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
            elapsed: None,
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
        let trust_stamps = stamp_pass_enabled()?;
        let live_stamps: HashMap<&str, EntryStamp> = if trust_stamps {
            walked
                .files
                .iter()
                .filter_map(|file| file.stamp.map(|stamp| (file.path.as_str(), stamp)))
                .collect()
        } else {
            HashMap::new()
        };
        let mut confirmed = 0u64;
        let mut skipped = 0u64;
        let mut verified = 0u64;
        for file in &decoded.generation.files {
            let Some(digest) = file.digest else {
                continue;
            };
            // Equal stat stamp of a settled file: the bytes are not read. Any difference, a missing live
            // stamp (racy window, new inode source) or `strict_live` falls through to the digest.
            if let (Some(stored), Some(live)) = (file.stamp, live_stamps.get(file.path.as_str()))
                && stored == *live
            {
                skipped += 1;
                continue;
            }
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
        self.stamp_skipped_reads += skipped;
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

#[derive(Clone)]
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
    let mut reader = Reader {
        rest: bytes,
        strings: Vec::new(),
    };
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
    reader.load_strings()?;
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
    let partial_count = reader.count(4)?;
    let mut partial_dirs = Vec::with_capacity(partial_count);
    for _ in 0..partial_count {
        partial_dirs.push(reader.string()?);
    }
    let file_count = reader.count(23)?;
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
        let size = reader.u64()?;
        let modified = reader.u64()?;
        let stamp = match reader.u8()? {
            0 => None,
            1 => Some(EntryStamp {
                size,
                modified_ns: reader.u128()?,
                inode: reader.u64()?,
            }),
            _ => {
                return Err(DctxError::CorruptIndex {
                    reason: "generation stamp",
                });
            }
        };
        files.push(IndexedFile {
            path,
            decision,
            digest,
            size,
            modified,
            stamp,
            syntax: reader.boolean()?,
            partial: reader.boolean()?,
        });
    }
    let mut text = TextBlob::default();
    text.partial_dirs = partial_dirs;
    let node_count = reader.count(53)?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let letter = reader.u8()?;
        if !matches!(letter, b'p' | b'k' | b'm' | b's' | b'd' | b't') {
            return Err(DctxError::CorruptIndex {
                reason: "generation node id",
            });
        }
        let ordinal = reader.u32()?;
        let kind = reader.string()?;
        let name = reader.string()?;
        let path = reader.string()?;
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
    let edge_count = reader.count(16)?;
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let src = reader.u32()?;
        let dst = reader.u32()?;
        let edge = reader.string()?;
        let confidence = reader.string()?;
        if src as usize >= nodes.len() || dst as usize >= nodes.len() {
            return Err(DctxError::CorruptIndex {
                reason: "generation edge",
            });
        }
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
