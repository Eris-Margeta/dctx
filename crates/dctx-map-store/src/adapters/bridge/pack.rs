// SPDX-License-Identifier: Apache-2.0
//! Pack snapshot codec and the `FactStore` load/publish path.
//! Implements the frozen `FactStore` port: one commit point per view, previous snapshot on failure.
//! Depends on dctx-hash for the payload checksum and on the bridge ledger for the byte cap.
//! Used by [`super::super::SqliteStore`]. Tree bytes are not a git object id.

use super::ledger;
use crate::domain::{Result, StoreError, view_label};
use dctx_core::datatypes::{DirContextTreeNode, NodeType};
use dctx_core::domain::FileHandlingDecision;
use dctx_core::facts::{ContentDigests, PackSnapshot};
use dctx_core::ids::{Timestamp, ViewKey};
use dctx_core::version::Version;
use dctx_hash::ContentDigest;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const MAX_DEPTH: usize = 128;
const MAX_NODES: u32 = 100_000;

pub(crate) fn load(connection: &Connection, view: &ViewKey) -> Result<Option<PackSnapshot>> {
    view_label(view.as_str())?;
    let payload: Option<Vec<u8>> = connection
        .query_row(
            "SELECT payload FROM pack_snapshots WHERE view_key = ?1",
            [view.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    payload.map(|bytes| decode(&bytes)).transpose()
}

pub(crate) fn publish(
    connection: &mut Connection,
    now_ms: u64,
    view: &ViewKey,
    snapshot: &PackSnapshot,
) -> Result<()> {
    view_label(view.as_str())?;
    let payload = encode(snapshot)?;
    let bytes = ledger::charge(payload.len())?;
    let transaction = connection.transaction()?;
    let (now, _) = ledger::touch(&transaction, now_ms)?;
    let previous: Option<(i64, i64)> = transaction
        .query_row(
            "SELECT generation, bytes FROM pack_snapshots WHERE view_key = ?1",
            [view.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    ledger::reclaim(&transaction, now, Some(view.as_str()))?;
    let replacing = previous.map_or(0, |(_, old)| old);
    ledger::admit(&transaction, bytes, replacing)?;
    let generation = match previous {
        Some((current, _)) => current
            .checked_add(1)
            .ok_or(StoreError::Corrupt("pack generation"))?,
        None => 1,
    };
    let used = i64::try_from(now).map_err(|_| StoreError::InvalidKey("clock is out of range"))?;
    transaction.execute(
        "INSERT INTO pack_snapshots(view_key, generation, payload, bytes, last_used)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(view_key) DO UPDATE SET
            generation = ?2, payload = ?3, bytes = ?4, last_used = ?5",
        params![view.as_str(), generation, payload, bytes, used],
    )?;
    transaction.commit()?;
    Ok(())
}

fn encode(snapshot: &PackSnapshot) -> Result<Vec<u8>> {
    if snapshot.digests().len() > MAX_NODES as usize {
        return Err(StoreError::InvalidKey("pack digests"));
    }
    let mut body = Vec::new();
    body.extend_from_slice(b"DCTXPK02");
    let strings = Strings::collect(snapshot)?;
    push_u32(&mut body, strings.values.len() as u32);
    for value in &strings.values {
        push_blob(&mut body, value.as_bytes())?;
    }
    push_u32(&mut body, snapshot.version().major);
    push_u32(&mut body, snapshot.version().minor);
    push_u64(&mut body, snapshot.taken_at().millis());
    push_digest(&mut body, &snapshot.digest().0);
    push_u64(
        &mut body,
        u64::try_from(snapshot.digests().len())
            .map_err(|_| StoreError::InvalidKey("pack field"))?,
    );
    for (path, digest) in snapshot.digests() {
        strings.push(&mut body, path)?;
        push_digest(&mut body, digest);
    }
    encode_node(&mut body, snapshot.tree(), 0, &strings)?;
    let checksum = dctx_hash::Sha256::digest(&body)
        .map_err(|_| StoreError::InvalidKey("invalid content digest"))?;
    body.extend_from_slice(checksum.as_bytes());
    Ok(body)
}

fn encode_node(
    out: &mut Vec<u8>,
    node: &DirContextTreeNode,
    depth: usize,
    strings: &Strings<'_>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(StoreError::InvalidKey("pack depth"));
    }
    out.push(match node.node_type {
        NodeType::File => 1,
        NodeType::Directory => 2,
    });
    out.push(match node.handling_decision {
        FileHandlingDecision::Include => 0,
        FileHandlingDecision::Exclude => 1,
        FileHandlingDecision::DebugVisible => 2,
        FileHandlingDecision::OmitContent => 3,
    });
    let mut flags = u8::from(node.is_ignored) | (u8::from(node.is_binary_file) << 1);
    if node.binary_file_type.is_some() {
        flags |= 8;
    }
    out.push(flags);
    push_u64(out, node.last_modified_timestamp);
    push_u64(out, node.content_offset_in_data_section);
    push_u64(out, node.content_size);
    strings.push(out, &node.relative_path)?;
    let disk = node
        .disk_path
        .to_str()
        .ok_or(StoreError::InvalidKey("disk path is not utf-8"))?;
    strings.push(out, disk)?;
    strings.push(out, &node.generated_id_for_llm)?;
    if let Some(kind) = &node.binary_file_type {
        strings.push(out, kind)?;
    }
    push_u64(
        out,
        u64::try_from(node.children.len()).map_err(|_| StoreError::InvalidKey("pack field"))?,
    );
    for child in &node.children {
        encode_node(out, child, depth + 1, strings)?;
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<PackSnapshot> {
    if bytes.len() < 32 {
        return Err(corrupt());
    }
    let (body, sum) = bytes.split_at(bytes.len() - 32);
    let checksum = dctx_hash::Sha256::digest(body).map_err(|_| corrupt())?;
    if checksum.as_bytes().as_slice() != sum {
        return Err(corrupt());
    }
    let mut cursor = Cursor::new(body);
    cursor.legacy = match cursor.take(8)? {
        b"DCTXPK01" => true,
        b"DCTXPK02" => false,
        _ => return Err(corrupt()),
    };
    if !cursor.legacy {
        let count = cursor.u32()? as usize;
        if count > MAX_NODES as usize * 5 || count > body.len() / 8 {
            return Err(corrupt());
        }
        for _ in 0..count {
            let text = cursor.raw_text()?;
            cursor.strings.push(text);
        }
    }
    let version = Version {
        major: cursor.u32()?,
        minor: cursor.u32()?,
    };
    let taken_at = Timestamp(cursor.u64()?);
    let stored = cursor.digest()?;
    let count = cursor.u64()?;
    if count > u64::from(MAX_NODES) {
        return Err(corrupt());
    }
    let mut digests = ContentDigests::new();
    for _ in 0..count {
        let path = cursor.text()?;
        let digest = cursor.digest()?;
        if digests.insert(path, digest).is_some() {
            return Err(corrupt());
        }
    }
    let mut nodes = 0;
    let tree = cursor.node(0, &mut nodes)?;
    cursor.finish()?;
    let snapshot = PackSnapshot::new(tree, version, taken_at, digests).map_err(|_| corrupt())?;
    if snapshot.digest().0 != stored {
        return Err(corrupt());
    }
    Ok(snapshot)
}

fn corrupt() -> StoreError {
    StoreError::Corrupt("pack snapshot")
}

struct Cursor<'a> {
    buf: &'a [u8],
    at: usize,
    legacy: bool,
    strings: Vec<&'a str>,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            at: 0,
            legacy: false,
            strings: Vec::new(),
        }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(len).ok_or_else(corrupt)?;
        if end > self.buf.len() {
            return Err(corrupt());
        }
        let slice = &self.buf[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u32(&mut self) -> Result<u32> {
        let mut raw = [0; 4];
        raw.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(raw))
    }

    fn u64(&mut self) -> Result<u64> {
        let mut raw = [0; 8];
        raw.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(raw))
    }

    fn raw_text(&mut self) -> Result<&'a str> {
        let len = usize::try_from(self.u64()?).map_err(|_| corrupt())?;
        std::str::from_utf8(self.take(len)?).map_err(|_| corrupt())
    }

    fn text(&mut self) -> Result<String> {
        if self.legacy {
            return Ok(self.raw_text()?.to_string());
        }
        let id = self.u32()? as usize;
        self.strings
            .get(id)
            .map(|value| (*value).to_string())
            .ok_or_else(corrupt)
    }

    fn digest(&mut self) -> Result<ContentDigest> {
        let version = self.take(1)?[0];
        let mut alg_raw = [0; 2];
        alg_raw.copy_from_slice(self.take(2)?);
        let algorithm = u16::from_le_bytes(alg_raw);
        let mut bytes = [0; 32];
        bytes.copy_from_slice(self.take(32)?);
        let byte_length = self.u64()?;
        if version != dctx_hash::IDENTITY_VERSION
            || algorithm != dctx_hash::IdentityAlgorithm::SHA256
        {
            return Err(corrupt());
        }
        Ok(ContentDigest::sha256(bytes, byte_length))
    }

    fn node(&mut self, depth: usize, nodes: &mut u32) -> Result<DirContextTreeNode> {
        if depth > MAX_DEPTH {
            return Err(corrupt());
        }
        *nodes = nodes.checked_add(1).ok_or_else(corrupt)?;
        if *nodes > MAX_NODES {
            return Err(corrupt());
        }
        let kind = match self.take(1)?[0] {
            1 => NodeType::File,
            2 => NodeType::Directory,
            _ => return Err(corrupt()),
        };
        let decision = match self.take(1)?[0] {
            0 => FileHandlingDecision::Include,
            1 => FileHandlingDecision::Exclude,
            2 => FileHandlingDecision::DebugVisible,
            3 => FileHandlingDecision::OmitContent,
            _ => return Err(corrupt()),
        };
        let flags = self.take(1)?[0];
        if flags & !0b1111 != 0 || (!self.legacy && flags & 4 != 0) {
            return Err(corrupt());
        }
        let modified = self.u64()?;
        let offset = self.u64()?;
        let size = self.u64()?;
        let relative = self.text()?;
        let disk = self.text()?;
        let generated = self.text()?;
        if flags & 4 != 0 {
            // Legacy migration validates framing/checksum but never retains a historical body.
            let len = usize::try_from(self.u64()?).map_err(|_| corrupt())?;
            self.take(len)?;
        }
        let binary_type = if flags & 8 == 0 {
            None
        } else {
            Some(self.text()?)
        };
        let child_count = self.u64()?;
        let room = u64::from(MAX_NODES.saturating_sub(*nodes));
        if child_count > room {
            return Err(corrupt());
        }
        let mut children = Vec::new();
        for _ in 0..child_count {
            children.push(self.node(depth + 1, nodes)?);
        }
        let mut node = DirContextTreeNode::new(kind, &relative, PathBuf::from(disk));
        node.last_modified_timestamp = modified;
        node.content_offset_in_data_section = offset;
        node.content_size = size;
        node.generated_id_for_llm = generated;
        node.file_content = None;
        node.is_binary_file = flags & 2 != 0;
        node.binary_file_type = binary_type;
        node.is_ignored = flags & 1 != 0;
        node.handling_decision = decision;
        node.children = children;
        Ok(node)
    }

    fn finish(self) -> Result<()> {
        if self.at == self.buf.len() {
            Ok(())
        } else {
            Err(corrupt())
        }
    }
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_blob(out: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    let len = u64::try_from(bytes.len()).map_err(|_| StoreError::InvalidKey("pack field"))?;
    push_u64(out, len);
    out.extend_from_slice(bytes);
    Ok(())
}

fn push_digest(out: &mut Vec<u8>, digest: &ContentDigest) {
    out.push(digest.version());
    out.extend_from_slice(&digest.algorithm_code().to_le_bytes());
    out.extend_from_slice(digest.as_bytes());
    push_u64(out, digest.byte_length());
}

/// The dictionary shares paths between digest and tree rows and stores common labels once.
struct Strings<'a> {
    values: Vec<&'a str>,
    ids: BTreeMap<&'a str, u32>,
}

impl<'a> Strings<'a> {
    fn collect(snapshot: &'a PackSnapshot) -> Result<Self> {
        let mut unique = BTreeSet::new();
        unique.extend(snapshot.digests().keys().map(String::as_str));
        let mut stack = vec![(snapshot.tree(), 0)];
        let mut nodes = 0;
        while let Some((node, depth)) = stack.pop() {
            nodes += 1;
            if nodes > MAX_NODES || depth > MAX_DEPTH {
                return Err(StoreError::InvalidKey("pack bounds"));
            }
            unique.insert(node.relative_path.as_str());
            unique.insert(
                node.disk_path
                    .to_str()
                    .ok_or(StoreError::InvalidKey("disk path is not utf-8"))?,
            );
            unique.insert(node.generated_id_for_llm.as_str());
            if let Some(kind) = &node.binary_file_type {
                unique.insert(kind);
            }
            stack.extend(node.children.iter().map(|child| (child, depth + 1)));
        }
        let values: Vec<_> = unique.into_iter().collect();
        let ids = values
            .iter()
            .enumerate()
            .map(|(id, value)| (*value, id as u32))
            .collect();
        Ok(Self { values, ids })
    }

    fn push(&self, out: &mut Vec<u8>, value: &str) -> Result<()> {
        push_u32(
            out,
            *self
                .ids
                .get(value)
                .ok_or(StoreError::InvalidKey("pack string"))?,
        );
        Ok(())
    }
}

/// Replace old payloads atomically without advancing history versions or generations.
/// Secure deletion and vacuum remove the obsolete content-bearing pages from the active cache.
pub(super) fn migrate(connection: &mut Connection) -> Result<()> {
    let views: Vec<String> = {
        let mut statement = connection.prepare(
            "SELECT view_key FROM pack_snapshots WHERE substr(payload, 1, 8) = x'44435458504b3031'",
        )?;
        statement
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    if views.is_empty() {
        return Ok(());
    }
    connection.pragma_update(None, "secure_delete", "ON")?;
    let transaction = connection.transaction()?;
    for view in views {
        // Bound migration memory to one historical payload. Any bad row rolls back every update.
        let old: Vec<u8> = transaction.query_row(
            "SELECT payload FROM pack_snapshots WHERE view_key = ?1",
            [&view],
            |row| row.get(0),
        )?;
        let bytes = encode(&decode(&old)?)?;
        transaction.execute(
            "UPDATE pack_snapshots SET payload = ?1, bytes = ?2 WHERE view_key = ?3",
            params![bytes, ledger::charge(bytes.len())?, view],
        )?;
    }
    let used = ledger::cached_bytes(&transaction)?;
    let cap = ledger::cache_cap(&transaction)?;
    if used > cap {
        return Err(StoreError::CacheLimit {
            requested: used,
            available: cap,
        });
    }
    transaction.commit()?;
    connection.execute_batch("VACUUM")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteStore;

    const CONTENT: &[u8] = b"historical-body-canary-which-must-never-remain";

    fn snapshot() -> PackSnapshot {
        let mut root = DirContextTreeNode::new(NodeType::Directory, "", PathBuf::new());
        let mut file = DirContextTreeNode::new(NodeType::File, "a.txt", PathBuf::from("a.txt"));
        file.content_size = CONTENT.len() as u64;
        file.file_content = Some(CONTENT.to_vec());
        root.children.push(file);
        let digests =
            ContentDigests::from([("a.txt".into(), dctx_hash::Sha256::digest(CONTENT).unwrap())]);
        PackSnapshot::new(root, Version::new_v1(), Timestamp(42), digests).unwrap()
    }

    // Independent v1 framing fixture: old caches embedded bodies after the node id.
    fn legacy(snapshot: &PackSnapshot) -> Vec<u8> {
        fn node(out: &mut Vec<u8>, tree: &DirContextTreeNode) {
            out.push(if tree.node_type == NodeType::File {
                1
            } else {
                2
            });
            out.push(0);
            out.push(if tree.file_content.is_some() { 4 } else { 0 });
            push_u64(out, tree.last_modified_timestamp);
            push_u64(out, 0);
            push_u64(out, tree.content_size);
            push_blob(out, tree.relative_path.as_bytes()).unwrap();
            push_blob(out, tree.disk_path.to_str().unwrap().as_bytes()).unwrap();
            push_blob(out, b"").unwrap();
            if let Some(content) = &tree.file_content {
                push_blob(out, content).unwrap();
            }
            push_u64(out, tree.children.len() as u64);
            for child in &tree.children {
                node(out, child);
            }
        }
        let mut bytes = b"DCTXPK01".to_vec();
        push_u32(&mut bytes, snapshot.version().major);
        push_u32(&mut bytes, snapshot.version().minor);
        push_u64(&mut bytes, snapshot.taken_at().millis());
        push_digest(&mut bytes, &snapshot.digest().0);
        push_u64(&mut bytes, snapshot.digests().len() as u64);
        for (path, digest) in snapshot.digests() {
            push_blob(&mut bytes, path.as_bytes()).unwrap();
            push_digest(&mut bytes, digest);
        }
        node(&mut bytes, snapshot.tree());
        let checksum = dctx_hash::Sha256::digest(&bytes).unwrap();
        bytes.extend_from_slice(checksum.as_bytes());
        bytes
    }

    #[test]
    fn compact_history_has_no_bodies_and_rejects_every_truncation() {
        let original = snapshot();
        let bytes = encode(&original).unwrap();
        assert_eq!(&bytes[..8], b"DCTXPK02");
        assert!(!bytes.windows(CONTENT.len()).any(|window| window == CONTENT));
        let loaded = decode(&bytes).unwrap();
        assert_eq!(loaded.digest(), original.digest());
        assert_eq!(loaded.digests(), original.digests());
        assert!(loaded.tree().children[0].file_content.is_none());
        assert_eq!(encode(&loaded).unwrap(), bytes);
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err(), "{end}");
        }
        // A valid checksum cannot hide an invalid dictionary reference.
        let mut damaged = bytes[..bytes.len() - 32].to_vec();
        let end = damaged.len();
        // Last child has zero children; its generated-id reference precedes that count.
        damaged[end - 12..end - 8].copy_from_slice(&u32::MAX.to_le_bytes());
        let sum = dctx_hash::Sha256::digest(&damaged).unwrap();
        damaged.extend_from_slice(sum.as_bytes());
        assert!(decode(&damaged).is_err());
    }

    #[test]
    fn opening_bridge_migrates_old_history_without_advancing_it() {
        let mut store = SqliteStore::in_memory().unwrap();
        let original = snapshot();
        let bytes = legacy(&original);
        assert!(bytes.windows(CONTENT.len()).any(|window| window == CONTENT));
        store
            .connection
            .execute(
                "INSERT INTO pack_snapshots VALUES('old', 7, ?1, ?2, 19)",
                params![bytes, bytes.len() as i64],
            )
            .unwrap();
        super::super::schema::ensure(&mut store.connection).unwrap();
        let (generation, bytes, charged, used): (i64, Vec<u8>, i64, i64) = store
            .connection
            .query_row(
                "SELECT generation, payload, bytes, last_used FROM pack_snapshots",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!((generation, used), (7, 19));
        assert_eq!(charged, ledger::charge(bytes.len()).unwrap());
        assert_eq!(bytes, encode(&original).unwrap());
        assert_eq!(decode(&bytes).unwrap().digest(), original.digest());
        super::super::schema::ensure(&mut store.connection).unwrap();
    }

    #[test]
    fn migration_never_exceeds_the_configured_cap_or_discards_history() {
        let mut store = SqliteStore::in_memory().unwrap();
        let original = legacy(&snapshot());
        store
            .connection
            .execute(
                "INSERT INTO pack_snapshots VALUES('old', 7, ?1, ?2, 19)",
                params![original, original.len() as i64],
            )
            .unwrap();
        store
            .connection
            .execute("UPDATE bridge_ledger SET hard_cap = 1", [])
            .unwrap();
        assert!(matches!(
            super::super::schema::ensure(&mut store.connection),
            Err(StoreError::CacheLimit { .. })
        ));
        let saved: Vec<u8> = store
            .connection
            .query_row(
                "SELECT payload FROM pack_snapshots WHERE view_key='old'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, original);
    }

    #[test]
    fn corrupt_legacy_row_leaves_all_history_unchanged() {
        let mut store = SqliteStore::in_memory().unwrap();
        let valid = legacy(&snapshot());
        let mut invalid = valid.clone();
        invalid[10] ^= 1;
        for (view, bytes) in [("good", &valid), ("bad", &invalid)] {
            store
                .connection
                .execute(
                    "INSERT INTO pack_snapshots VALUES(?1, 7, ?2, ?3, 19)",
                    params![view, bytes, bytes.len() as i64],
                )
                .unwrap();
        }
        assert!(super::super::schema::ensure(&mut store.connection).is_err());
        let saved: Vec<u8> = store
            .connection
            .query_row(
                "SELECT payload FROM pack_snapshots WHERE view_key='good'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(saved, valid);
    }
}
