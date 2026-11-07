// SPDX-License-Identifier: Apache-2.0
//! Comparison-bridge tables beside the v1 map schema.
//! Implements ARCHITECTURE-PROPOSAL §3 option B: shared blob facts plus pack snapshots.
//! Depends on the SQLite connection. Called on every [`super::super::SqliteStore`] open.
//! Existing map tables are left in place. `user_version` stays 1.

use crate::domain::{HARD_CAP_BYTES, Result, StoreError};
use rusqlite::Connection;

pub(crate) fn ensure(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS bridge_ledger (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            parses INTEGER NOT NULL,
            hard_cap INTEGER NOT NULL,
            last_clock INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS pack_snapshots (
            view_key TEXT PRIMARY KEY,
            generation INTEGER NOT NULL,
            payload BLOB NOT NULL,
            bytes INTEGER NOT NULL,
            last_used INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS blob_facts (
            namespace TEXT NOT NULL,
            fact_key TEXT NOT NULL,
            payload BLOB NOT NULL,
            bytes INTEGER NOT NULL,
            last_used INTEGER NOT NULL,
            retired_at INTEGER,
            PRIMARY KEY (namespace, fact_key)
         );
         CREATE TABLE IF NOT EXISTS blob_refs (
            namespace TEXT NOT NULL,
            root_id TEXT NOT NULL,
            fact_key TEXT NOT NULL,
            PRIMARY KEY (namespace, root_id, fact_key)
         );
         CREATE INDEX IF NOT EXISTS blob_refs_fact ON blob_refs(namespace, fact_key);",
    )?;
    let cap = i64::try_from(HARD_CAP_BYTES).map_err(|_| StoreError::Corrupt("cache accounting"))?;
    transaction.execute(
        "INSERT INTO bridge_ledger(id, parses, hard_cap, last_clock)
         SELECT 1, 0, ?1, 0 WHERE NOT EXISTS (SELECT 1 FROM bridge_ledger WHERE id = 1)",
        [cap],
    )?;
    transaction.commit()?;
    super::pack::migrate(connection)
}
