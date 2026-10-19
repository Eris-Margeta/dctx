//! Persisted map generation reused when content digests still match.
//! Implements DCTX-SERIOUS-GAPS G6 (CLI-WARM-REUSE) on the session cache lease.
//! Depends on Session, the index walk and digest_text (the read_verified check).
//! The CLI calls Session::snapshot_reusable. Isolation without a cache root does not.
//! Size and mtime are a prefilter. A same-stamp byte change still rebuilds.

use std::collections::BTreeMap;

use dctx_core::FileHandlingDecision;
use dctx_hash::ContentDigest;

use super::Session;
use crate::domain::{FreshnessMode, ParserChoice, ScanLimits, SnapshotRequest, DctxError, STRUCTURAL_PARSER};
use crate::index::{walk_source, Generation, IndexedEdge, IndexedFile, IndexedNode, WalkArgs};

const REUSE_ATTEMPTS: u32 = 32;
const MAGIC: &[u8] = b"dctxgen1";
const VERSION: u32 = 1;
const MAX_STRING: usize = 1_048_576;
const MAX_COUNT: usize = 1_000_000;
const SYNTAX_PARSER: &str = "dctx-lex-rust/syn-3.0.9";

impl Session {
    /// Takes a snapshot, reusing a persisted generation when every stored content digest still matches.
    ///
    /// The check is `digest_text` equality on the source bytes, the same comparison `read_verified`
    /// uses before it returns `SourceChanged`. Size and modification time only skip that read when
    /// the inventory already differs. An empty `source_key`, or a session with no cache root, walks
    /// the source the same way as [`Self::snapshot`].
    pub fn snapshot_reusable(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<super::SnapshotHandle, DctxError> {
        if self.managed.is_none() || source_key.is_empty() {
            return self.snapshot(request);
        }
        let mut last = DctxError::Busy;
        for _ in 0..REUSE_ATTEMPTS {
            match self.snapshot_reusable_once(request, source_key) {
                Err(error) if retryable(&error) => {
                    last = error;
                    std::thread::yield_now();
                }
                other => return other,
            }
        }
        Err(last)
    }

    fn snapshot_reusable_once(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<super::SnapshotHandle, DctxError> {
        self.reject_snapshot(request)?;
        self.hooks_before_index()?;
        match self.try_reuse(request, source_key)? {
            Some(handle) => {
                self.hooks_after_index()?;
                Ok(handle)
            }
            None => self.snapshot_inner(request, Some(source_key), false),
        }
    }

    fn try_reuse(
        &mut self,
        request: &SnapshotRequest,
        source_key: &str,
    ) -> Result<Option<super::SnapshotHandle>, DctxError> {
        let (limits, parser, scan_limits) = self.reject_snapshot(request)?;
        let Some(bytes) = self.load_retained()? else {
            return Ok(None);
        };
        let decoded = match decode_generation(&bytes) {
            Ok(decoded) => decoded,
            Err(_) => return Ok(None),
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
                Ok(false) => {
                    self.content_reads += verified;
                    return Ok(None);
                }
                Err(error @ (DctxError::Cancelled | DctxError::ResourceLimit { .. } | DctxError::PolicyDenied)) => {
                    return Err(error);
                }
                Err(_) => {
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

    fn load_retained(&mut self) -> Result<Option<Vec<u8>>, DctxError> {
        let Some(cache) = self.managed.as_mut() else {
            return Ok(None);
        };
        cache.load_current_segment()
    }

    fn pin_retained(&mut self, now_ms: u64) -> Result<Option<super::ledger::SnapshotLease>, DctxError> {
        let Some(cache) = self.managed.as_mut() else {
            return Ok(None);
        };
        cache.pin_current(now_ms)
    }
}

fn retryable(error: &DctxError) -> bool {
    match error {
        DctxError::Busy => true,
        DctxError::InvalidRequest { reason, .. } if *reason == "stale ledger" => true,
        _ => false,
    }
}

fn parser_compatible(stored: &str, choice: ParserChoice) -> bool {
    match choice {
        ParserChoice::Opaque => stored == "opaque/1",
        ParserChoice::Structural => stored == STRUCTURAL_PARSER,
        ParserChoice::Syn => stored == STRUCTURAL_PARSER || stored == SYNTAX_PARSER,
        ParserChoice::TreeSitter => false,
    }
}

fn limits_match(stored: &[u64; 4], live: &ScanLimits) -> bool {
    stored[0] == live.entries && stored[1] == live.depth && stored[2] == live.file_bytes && stored[3] == live.source_bytes
}

fn file_inventory(files: &[IndexedFile]) -> Option<BTreeMap<&str, (FileHandlingDecision, u64, u64)>> {
    let mut map = BTreeMap::new();
    for file in files {
        if map.insert(file.path.as_str(), (file.decision, file.size, file.modified)).is_some() {
            return None;
        }
    }
    Some(map)
}

struct PersistedGeneration {
    source_key: String,
    view: String,
    limits: [u64; 4],
    generation: Generation,
}

pub(super) fn encode_generation(
    source_key: &str,
    view: &str,
    limits: &ScanLimits,
    generation: &Generation,
) -> Result<Vec<u8>, DctxError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    push_str(&mut out, source_key)?;
    push_str(&mut out, view)?;
    out.push(freshness_byte(generation.freshness));
    push_u64(&mut out, generation.epoch);
    push_str(&mut out, &generation.digest)?;
    push_str(&mut out, &generation.coverage)?;
    push_str(&mut out, &generation.completion)?;
    push_str(&mut out, &generation.parser)?;
    push_u64(&mut out, generation.taken_at);
    push_u64(&mut out, generation.id);
    push_u64(&mut out, limits.entries);
    push_u64(&mut out, limits.depth);
    push_u64(&mut out, limits.file_bytes);
    push_u64(&mut out, limits.source_bytes);
    push_count(&mut out, generation.files.len())?;
    for file in &generation.files {
        push_str(&mut out, &file.path)?;
        out.push(decision_byte(file.decision));
        match file.digest {
            Some(digest) => {
                out.push(1);
                out.extend_from_slice(digest.as_bytes());
                push_u64(&mut out, digest.byte_length());
            }
            None => out.push(0),
        }
        push_u64(&mut out, file.size);
        push_u64(&mut out, file.modified);
    }
    push_count(&mut out, generation.nodes.len())?;
    for node in &generation.nodes {
        push_str(&mut out, &node.id)?;
        push_str(&mut out, &node.kind)?;
        push_str(&mut out, &node.name)?;
        push_str(&mut out, &node.path)?;
        push_str(&mut out, &node.summary)?;
        push_str(&mut out, &node.summary_source)?;
        push_str(&mut out, &node.signature)?;
        push_u32(&mut out, node.lines[0]);
        push_u32(&mut out, node.lines[1]);
        push_u64(&mut out, node.span[0]);
        push_u64(&mut out, node.span[1]);
    }
    push_count(&mut out, generation.edges.len())?;
    for edge in &generation.edges {
        push_str(&mut out, &edge.src)?;
        push_str(&mut out, &edge.dst)?;
        push_str(&mut out, &edge.edge)?;
        push_str(&mut out, &edge.confidence)?;
    }
    Ok(out)
}

fn decode_generation(bytes: &[u8]) -> Result<PersistedGeneration, DctxError> {
    let mut reader = Reader { rest: bytes };
    let magic = reader.take(MAGIC.len())?;
    if magic != MAGIC {
        return Err(DctxError::CorruptIndex { reason: "generation magic" });
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
                let raw: [u8; 32] = reader
                    .take(32)?
                    .try_into()
                    .map_err(|_| DctxError::CorruptIndex { reason: "generation digest" })?;
                Some(ContentDigest::sha256(raw, reader.u64()?))
            }
            _ => return Err(DctxError::CorruptIndex { reason: "generation digest" }),
        };
        files.push(IndexedFile { path, decision, digest, size: reader.u64()?, modified: reader.u64()? });
    }
    let node_count = reader.count()?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(IndexedNode {
            id: reader.string()?,
            kind: reader.string()?,
            name: reader.string()?,
            path: reader.string()?,
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
        edges.push(IndexedEdge {
            src: reader.string()?,
            dst: reader.string()?,
            edge: reader.string()?,
            confidence: reader.string()?,
        });
    }
    if !reader.rest.is_empty() {
        return Err(DctxError::CorruptIndex { reason: "generation trailing" });
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
            freshness,
            taken_at,
            epoch,
            coverage,
            completion,
            parser,
        },
    })
}

struct Reader<'a> {
    rest: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], DctxError> {
        if self.rest.len() < len {
            return Err(DctxError::CorruptIndex { reason: "generation truncated" });
        }
        let (head, tail) = self.rest.split_at(len);
        self.rest = tail;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, DctxError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, DctxError> {
        let bytes = self.take(4)?;
        let raw: [u8; 4] = bytes.try_into().map_err(|_| DctxError::CorruptIndex { reason: "generation truncated" })?;
        Ok(u32::from_le_bytes(raw))
    }

    fn u64(&mut self) -> Result<u64, DctxError> {
        let bytes = self.take(8)?;
        let raw: [u8; 8] = bytes.try_into().map_err(|_| DctxError::CorruptIndex { reason: "generation truncated" })?;
        Ok(u64::from_le_bytes(raw))
    }

    fn string(&mut self) -> Result<String, DctxError> {
        let len = usize::try_from(self.u32()?).map_err(|_| DctxError::CorruptIndex { reason: "generation string" })?;
        if len > MAX_STRING {
            return Err(DctxError::CorruptIndex { reason: "generation string" });
        }
        let bytes = self.take(len)?;
        let text = std::str::from_utf8(bytes).map_err(|_| DctxError::CorruptIndex { reason: "generation string" })?;
        Ok(text.to_string())
    }

    fn count(&mut self) -> Result<usize, DctxError> {
        let count = usize::try_from(self.u64()?).map_err(|_| DctxError::CorruptIndex { reason: "generation count" })?;
        if count > MAX_COUNT {
            return Err(DctxError::CorruptIndex { reason: "generation count" });
        }
        Ok(count)
    }
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_count(out: &mut Vec<u8>, count: usize) -> Result<(), DctxError> {
    if count > MAX_COUNT {
        return Err(DctxError::ResourceLimit { resource: crate::domain::Resource::Records, limit: MAX_COUNT as u64 });
    }
    let raw = u64::try_from(count).map_err(|_| DctxError::ResourceLimit {
        resource: crate::domain::Resource::Records,
        limit: MAX_COUNT as u64,
    })?;
    push_u64(out, raw);
    Ok(())
}

fn push_str(out: &mut Vec<u8>, text: &str) -> Result<(), DctxError> {
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

fn freshness_byte(mode: FreshnessMode) -> u8 {
    match mode {
        FreshnessMode::ImmutableSnapshot => 0,
        FreshnessMode::Strict => 1,
        FreshnessMode::Metadata => 2,
    }
}

fn freshness_of(byte: u8) -> Result<FreshnessMode, DctxError> {
    match byte {
        0 => Ok(FreshnessMode::ImmutableSnapshot),
        1 => Ok(FreshnessMode::Strict),
        2 => Ok(FreshnessMode::Metadata),
        _ => Err(DctxError::CorruptIndex { reason: "generation freshness" }),
    }
}

fn decision_byte(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 0,
        FileHandlingDecision::Exclude => 1,
        FileHandlingDecision::OmitContent => 2,
        FileHandlingDecision::DebugVisible => 3,
    }
}

fn decision_of(byte: u8) -> Result<FileHandlingDecision, DctxError> {
    match byte {
        0 => Ok(FileHandlingDecision::Include),
        1 => Ok(FileHandlingDecision::Exclude),
        2 => Ok(FileHandlingDecision::OmitContent),
        3 => Ok(FileHandlingDecision::DebugVisible),
        _ => Err(DctxError::CorruptIndex { reason: "generation decision" }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dctx_core::{SelectionPolicy, Timestamp};
    use dctx_ports::memory::{CancelFlag, FixedClock, MemorySource};

    use super::super::SnapshotHandle;
    use super::*;
    use crate::domain::{AutoCleanConfig, HostGrants, MapRequest, VecSink, Verb};
    use crate::{HostCeiling, RuleTexts, StoragePolicy};

    fn open(source: MemorySource, cache: &std::path::Path) -> Session {
        let mut auto = AutoCleanConfig::default();
        auto.enabled = false;
        Session::builder(source)
            .clock(FixedClock(Timestamp(0)))
            .cancellation(Arc::new(CancelFlag::new()))
            .selection(SelectionPolicy::from_texts(&RuleTexts::default()))
            .ceiling(HostCeiling::base())
            .grants(HostGrants::PermitAll)
            .storage(StoragePolicy::Ephemeral { max_bytes: 8 * 1024 * 1024 })
            .place("fixture")
            .view("default")
            .cache_root(cache, 1)
            .auto_clean(auto)
            .build()
            .expect("session")
    }

    fn source(body: &[u8]) -> MemorySource {
        let mut source = MemorySource::new();
        source.insert_file("item.rs", body, 9).expect("file");
        source
    }

    fn find_ids(session: &mut Session, handle: &SnapshotHandle, query: &str) -> Vec<String> {
        let mut request = MapRequest::new(Verb::Find);
        request.query = query.to_string();
        let mut sink = VecSink::new();
        let answer = session.query(handle, &request, &mut sink).expect("query");
        answer.records.into_iter().map(|record| record.id).collect()
    }

    #[test]
    fn same_stamp_byte_change_rebuilds_and_a_match_reuses() {
        let dir = tempfile::tempdir().expect("temp");
        let cache = dir.path().join("cache");
        std::fs::create_dir(&cache).expect("dir");
        let mut session = open(source(b"fn kept() {}\n"), &cache);
        let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
        assert!(!handle.reused());
        assert!(session.content_reads() >= 1);
        assert_eq!(session.cached_content_reads(), 0);
        let cold_reads = session.content_reads();
        let kept = find_ids(&mut session, &handle, "kept");
        assert!(kept.iter().any(|id| id.contains("kept")), "{kept:?}");
        drop(handle);
        drop(session);

        let mut warm = open(source(b"fn kept() {}\n"), &cache);
        let handle = warm.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("warm");
        assert!(handle.reused());
        assert_eq!(handle.generation(), 1);
        assert_eq!(warm.content_reads(), 0);
        assert!(warm.content_reads() < cold_reads);
        assert!(warm.cached_content_reads() >= 1);
        assert_eq!(find_ids(&mut warm, &handle, "kept"), kept);
        drop(handle);
        drop(warm);

        let mut third = open(source(b"fn lost() {}\n"), &cache);
        let handle = third.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("third");
        assert!(!handle.reused());
        assert!(third.content_reads() >= 1);
        let lost = find_ids(&mut third, &handle, "lost");
        assert!(lost.iter().any(|id| id.contains("lost")), "{lost:?}");
        assert!(find_ids(&mut third, &handle, "kept").is_empty());
    }

    #[test]
    fn a_different_source_key_does_not_reuse() {
        let dir = tempfile::tempdir().expect("temp");
        let cache = dir.path().join("cache");
        std::fs::create_dir(&cache).expect("dir");
        let mut session = open(source(b"fn kept() {}\n"), &cache);
        let handle = session.snapshot_reusable(&SnapshotRequest::strict(), "/src").expect("cold");
        assert!(!handle.reused());
        drop(handle);
        drop(session);

        let mut other = open(source(b"fn kept() {}\n"), &cache);
        let handle = other.snapshot_reusable(&SnapshotRequest::strict(), "/other").expect("other");
        assert!(!handle.reused());
        assert!(other.content_reads() >= 1);
        assert_eq!(other.cached_content_reads(), 0);
    }
}
