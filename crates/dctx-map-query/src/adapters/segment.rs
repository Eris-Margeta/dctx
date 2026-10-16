//! Segment generations for the query journal and a file-level view.
//! Implements DX-C3 publication and DX-C4 admission for one map root.
//! Cache load pins the latest generation; sqlite remains the comparison backend.
use super::journal::{self, Journal};
use super::source::digest_bytes;
use super::Index;
use crate::{Error, Result, ports::SourceFile};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use dctx_adapter_cache::{Lifecycle, RootId};
use dctx_adapter_store::{
    Pin, PublishRequest, Revisions, SegmentBlob, SegmentRole, SegmentStore, StdFs as StoreFs,
    StoreOptions,
};
use dctx_map_segment::{
    Completion, Coverage, DeclarationDraft, DeclarationKind, FactDraft, Language, NodeDraft,
    SegmentBuilder, SummarySource, ViewKeyDraft, decode,
};
use dctx_ports::Cache;

/// On-disk segment store next to the sqlite comparison file.
pub(crate) type Segments = SegmentStore<StoreFs>;

const BATCH: usize = 8_000_000;

/// Root id accepted by the segment store. Empty and `.git` become `place`.
pub(super) fn root_id(place: &str) -> String {
    let mut id: String = place
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-') {
                char
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    if id.is_empty() || id == ".git" {
        id = "place".into();
    }
    id
}

pub(super) fn open_store(directory: &Path, place: &str, database: &Path) -> Result<Segments> {
    let fs = StoreFs::open(directory)?;
    let store = SegmentStore::open(fs, StoreOptions::capped(512 * 1024 * 1024))?;
    let root = root_id(place);
    let warm = store
        .raw_current_generation(&root)
        .ok()
        .flatten()
        .is_some_and(|generation| recall(database, &root, generation).is_some());
    if !warm {
        store.recover()?;
    }
    Ok(store)
}

pub(super) fn persist(index: &mut Index) -> Result<()> {
    if index.segments.is_none() {
        return Ok(());
    }
    let root = root_id(&index.place);
    let generation = index
        .segments
        .as_ref()
        .ok_or_else(|| Error::Request("missing segment store".into()))?
        .committed(&root)?
        .unwrap_or(0)
        .saturating_add(1)
        .max(1);
    index.generation = generation;
    let journal = Journal::capture(index);
    let text = serde_json::to_string(&journal)?;
    let facts = fact_blobs(&text, generation)?;
    let view = view_blob(index, &root, generation)?;
    let view_digest = digest_bytes(&view)?;
    let mut admitted = view.len();
    let mut segments = Vec::new();
    for bytes in facts {
        admitted += bytes.len();
        segments.push(SegmentBlob {
            role: SegmentRole::Fact,
            bytes,
        });
    }
    segments.push(SegmentBlob {
        role: SegmentRole::View,
        bytes: view,
    });
    admit(u64::try_from(admitted).unwrap_or(u64::MAX))?;
    let request = PublishRequest {
        root: root.clone(),
        generation,
        snapshot: snapshot_digest(index)?,
        view: view_digest,
        policy_epoch: generation,
        segments,
        revisions: Revisions::structural(),
    };
    index
        .segments
        .as_ref()
        .ok_or_else(|| Error::Request("missing segment store".into()))?
        .publish(request)?;
    if let Some(database) = index.database.clone() {
        remember(database, root, generation, journal);
    }
    Ok(())
}

pub(super) fn read_journal(index: &Index) -> Result<Option<Journal>> {
    let Some(segments) = &index.segments else {
        return Ok(None);
    };
    let root = root_id(&index.place);
    if let Some(database) = &index.database
        && let Some(generation) = segments.raw_current_generation(&root)?
        && let Some(journal) = recall(database, &root, generation)
    {
        return Ok(Some(journal));
    }
    let Some(generation) = segments.committed(&root)? else {
        return Ok(None);
    };
    let pin = segments.pin(&root)?;
    let loaded = decode_pin(segments, &pin);
    let released = segments.release(pin).map_err(Error::from);
    match (loaded, released) {
        (Ok(journal), Ok(())) => {
            if let Some(database) = index.database.clone() {
                remember(database, root, generation, journal.clone());
            }
            Ok(Some(journal))
        }
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

struct Remembered {
    database: std::path::PathBuf,
    root: String,
    generation: u64,
    journal: Journal,
}

fn memory() -> MutexGuard<'static, Option<Remembered>> {
    static CACHE: Mutex<Option<Remembered>> = Mutex::new(None);
    CACHE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn remember(database: std::path::PathBuf, root: String, generation: u64, journal: Journal) {
    *memory() = Some(Remembered {
        database,
        root,
        generation,
        journal,
    });
}

fn recall(database: &Path, root: &str, generation: u64) -> Option<Journal> {
    memory().as_ref().and_then(|entry| {
        (entry.database == database && entry.root == root && entry.generation == generation)
            .then(|| entry.journal.clone())
    })
}

fn decode_pin(segments: &Segments, pin: &Pin) -> Result<Journal> {
    let mut parts = Vec::new();
    for segment in pin.segments() {
        if segment.role() != SegmentRole::Fact {
            continue;
        }
        let bytes = segments.read_segment(pin, segment.digest())?;
        let decoded = decode(&bytes, SegmentRole::Fact)
            .map_err(|error| Error::Request(error.to_string()))?;
        for fact in decoded.facts() {
            for declaration in &fact.declarations {
                let name = decoded
                    .string(declaration.name)
                    .map_err(|error| Error::Request(error.to_string()))?
                    .to_string();
                let summary = decoded
                    .string(declaration.summary)
                    .map_err(|error| Error::Request(error.to_string()))?
                    .to_string();
                parts.push((name, summary));
            }
        }
    }
    parts.sort();
    let text: String = parts.into_iter().map(|(_, summary)| summary).collect();
    if text.is_empty() {
        return Err(Error::Request("segment journal is empty".into()));
    }
    Ok(serde_json::from_str(&text)?)
}

fn fact_blobs(text: &str, generation: u64) -> Result<Vec<Vec<u8>>> {
    let mut blobs = Vec::new();
    let mut batch = Vec::new();
    let mut size = 0usize;
    for (index, part) in journal::chunk(text).into_iter().enumerate() {
        if size + part.len() > BATCH && !batch.is_empty() {
            blobs.push(build_facts(&batch, generation)?);
            batch.clear();
            size = 0;
        }
        size += part.len();
        batch.push((index, part));
    }
    if !batch.is_empty() {
        blobs.push(build_facts(&batch, generation)?);
    }
    Ok(blobs)
}

fn build_facts(batch: &[(usize, String)], generation: u64) -> Result<Vec<u8>> {
    let mut builder = SegmentBuilder::fact();
    for (index, chunk) in batch {
        let len = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        builder
            .add_fact(FactDraft {
                digest: digest_bytes(chunk.as_bytes())?,
                byte_length: len,
                language: Language::Opaque,
                dialect: "journal".into(),
                parser_id: "dctx-map-query".into(),
                parser_revision: "1".into(),
                extraction_plan: [0; 32],
                declarations: vec![DeclarationDraft {
                    name: format!("{index:05}"),
                    kind: DeclarationKind::Module,
                    span_start: 0,
                    span_end: len,
                    summary: chunk.clone(),
                    summary_source: SummarySource::Signature,
                    public: false,
                }],
                raw_refs: Vec::new(),
                coverage: Coverage::Opaque,
                completion: Completion::Complete,
                completion_reason: String::new(),
                extractor: "dctx-map-query".into(),
                generation,
                model: None,
                body_fingerprint: None,
            })
            .map_err(|error| Error::Request(error.to_string()))?;
    }
    builder
        .build()
        .map_err(|error| Error::Request(error.to_string()))
}

fn view_blob(index: &Index, root: &str, generation: u64) -> Result<Vec<u8>> {
    let mut builder = SegmentBuilder::view();
    builder
        .set_view_key(ViewKeyDraft {
            trust: "local".into(),
            root: root.into(),
            snapshot: snapshot_digest(index)?,
            overlay_generation: generation,
            policy: policy_digest(index)?,
            resolver_revision: "1".into(),
            environment: [0; 32],
        })
        .map_err(|error| Error::Request(error.to_string()))?;
    if let Some(snapshot) = &index.snapshot {
        for (path, file) in &snapshot.files {
            builder
                .add_node(node_draft(path, file)?)
                .map_err(|error| Error::Request(error.to_string()))?;
        }
    }
    builder
        .build()
        .map_err(|error| Error::Request(error.to_string()))
}

fn node_draft(path: &str, file: &SourceFile) -> Result<NodeDraft> {
    Ok(NodeDraft {
        path: path.into(),
        public_id: path.into(),
        digest: content_digest(&file.hash, path)?,
        byte_length: file.size,
        language: language(path),
        dialect: "source".into(),
        parser_id: "dctx-map-query".into(),
        parser_revision: "1".into(),
        extraction_plan: [0; 32],
        local_fact_id: 0,
        public: false,
        tier0: false,
    })
}

fn language(path: &str) -> Language {
    if path.ends_with(".rs") {
        Language::Rust
    } else if path.ends_with(".md") {
        Language::Markdown
    } else if path.ends_with(".js") {
        Language::JavaScript
    } else if path.ends_with(".ts") {
        Language::TypeScript
    } else {
        Language::Opaque
    }
}

fn content_digest(hash: &str, path: &str) -> Result<[u8; 32]> {
    if let Some(bytes) = decode_hex(hash) {
        return Ok(bytes);
    }
    digest_bytes(path.as_bytes())
}

fn snapshot_digest(index: &Index) -> Result<[u8; 32]> {
    let revision = index
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.revision.as_str())
        .unwrap_or("");
    if let Some(bytes) = decode_hex(revision) {
        return Ok(bytes);
    }
    digest_bytes(revision.as_bytes())
}

fn policy_digest(index: &Index) -> Result<[u8; 32]> {
    let mut bytes = Vec::new();
    if let Some(snapshot) = &index.snapshot {
        for (name, rule) in &snapshot.rules {
            bytes.extend(name.as_bytes());
            bytes.push(0);
            if let Some(text) = &rule.text {
                bytes.extend(text.as_bytes());
            }
        }
    }
    digest_bytes(&bytes)
}

fn decode_hex(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn admit(bytes: u64) -> Result<()> {
    if bytes == 0 {
        return Ok(());
    }
    let root = RootId::new(1);
    let mut life = Lifecycle::product_default();
    life.register_root(root)?;
    let object = life.insert_segment(bytes, 0, &[root])?;
    life.publish_view(root, bytes, &[object])?;
    life.publish_overlay(root, bytes, &[object])?;
    let reservation = Cache::reserve(&mut life, bytes)
        .map_err(|error| Error::Request(error.to_string()))?;
    Cache::commit(&mut life, reservation, bytes)
        .map_err(|error| Error::Request(error.to_string()))?;
    Ok(())
}
