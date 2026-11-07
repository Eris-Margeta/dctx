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
    let mut body = Vec::new();
    body.extend_from_slice(b"DCTXPK01");
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
        push_blob(&mut body, path.as_bytes())?;
        push_digest(&mut body, digest);
    }
    encode_node(&mut body, snapshot.tree(), 0)?;
    let checksum = dctx_hash::Sha256::digest(&body)
        .map_err(|_| StoreError::InvalidKey("invalid content digest"))?;
    body.extend_from_slice(checksum.as_bytes());
    Ok(body)
}

fn encode_node(out: &mut Vec<u8>, node: &DirContextTreeNode, depth: usize) -> Result<()> {
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
    if node.file_content.is_some() {
        flags |= 4;
    }
    if node.binary_file_type.is_some() {
        flags |= 8;
    }
    out.push(flags);
    push_u64(out, node.last_modified_timestamp);
    push_u64(out, node.content_offset_in_data_section);
    push_u64(out, node.content_size);
    push_blob(out, node.relative_path.as_bytes())?;
    let disk = node
        .disk_path
        .to_str()
        .ok_or(StoreError::InvalidKey("disk path is not utf-8"))?;
    push_blob(out, disk.as_bytes())?;
    push_blob(out, node.generated_id_for_llm.as_bytes())?;
    if let Some(content) = &node.file_content {
        push_blob(out, content)?;
    }
    if let Some(kind) = &node.binary_file_type {
        push_blob(out, kind.as_bytes())?;
    }
    push_u64(
        out,
        u64::try_from(node.children.len()).map_err(|_| StoreError::InvalidKey("pack field"))?,
    );
    for child in &node.children {
        encode_node(out, child, depth + 1)?;
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
    if cursor.take(8)? != b"DCTXPK01".as_slice() {
        return Err(corrupt());
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
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, at: 0 }
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

    fn blob(&mut self) -> Result<Vec<u8>> {
        let len = usize::try_from(self.u64()?).map_err(|_| corrupt())?;
        Ok(self.take(len)?.to_vec())
    }

    fn text(&mut self) -> Result<String> {
        String::from_utf8(self.blob()?).map_err(|_| corrupt())
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
        if flags & !0b1111 != 0 {
            return Err(corrupt());
        }
        let modified = self.u64()?;
        let offset = self.u64()?;
        let size = self.u64()?;
        let relative = self.text()?;
        let disk = self.text()?;
        let generated = self.text()?;
        let content = if flags & 4 == 0 {
            None
        } else {
            Some(self.blob()?)
        };
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
        node.file_content = content;
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
