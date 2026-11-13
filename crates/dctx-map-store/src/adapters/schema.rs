// SPDX-License-Identifier: Apache-2.0
//! Versioned SQLite schema for persistent map facts.
//! Implements spec section 8 including FTS5 and owner indexes.
//! Called only by the SQLite adapter at connection startup.
use crate::domain::{Result, StoreError};
use rusqlite::{Connection, TransactionBehavior};

pub(super) fn initialize(connection: &mut Connection) -> Result<()> {
    let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version != 0 && version != 1 {
        return Err(StoreError::RebuildRequired(version));
    }
    if version == 1 {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // A concurrent opener may have initialized the database while we waited.
    let version: i64 = transaction.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version == 1 {
        return Ok(());
    }
    if version != 0 {
        return Err(StoreError::RebuildRequired(version));
    }
    transaction.execute_batch(
        "CREATE TABLE meta(rev TEXT NOT NULL, schema INTEGER NOT NULL, built_at INTEGER NOT NULL);
         INSERT INTO meta VALUES('',1,0);
         CREATE TABLE files(path TEXT PRIMARY KEY, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
                            sha256 TEXT NOT NULL, lang TEXT NOT NULL);
         CREATE TABLE nodes(id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL,
             l0 INTEGER NOT NULL, l1 INTEGER NOT NULL, name TEXT NOT NULL, sig TEXT NOT NULL,
             fp TEXT NOT NULL, vis INTEGER NOT NULL, payload TEXT NOT NULL);
         CREATE INDEX nodes_path ON nodes(path);
         CREATE TABLE edges(src TEXT NOT NULL, dst TEXT NOT NULL, kind TEXT NOT NULL,
             conf TEXT NOT NULL, owner_path TEXT NOT NULL, payload TEXT NOT NULL,
             PRIMARY KEY(owner_path,src,dst,kind,conf));
         CREATE TABLE unresolved(owner_path TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
             PRIMARY KEY(owner_path,name,kind));
         CREATE TABLE summaries(id TEXT PRIMARY KEY, fp TEXT NOT NULL, text TEXT NOT NULL,
             src TEXT NOT NULL, model TEXT NOT NULL, at INTEGER NOT NULL);
         CREATE TABLE moved(old TEXT PRIMARY KEY, new TEXT NOT NULL, rev TEXT NOT NULL);
         CREATE VIRTUAL TABLE fts USING fts5(id UNINDEXED,tokens);
         PRAGMA user_version=1;",
    )?;
    transaction.commit()?;
    Ok(())
}
