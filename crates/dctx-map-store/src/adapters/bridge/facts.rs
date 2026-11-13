// SPDX-License-Identifier: Apache-2.0
//! Namespace-scoped blob facts keyed by the dctx-hash `FactKey`.
//! Implements option B: identical retained keys in several roots are stored once.
//! Depends on the bridge ledger. [`super::super::SqliteStore::retain_fact`] is the caller.
//! A conflicting payload is refused and does not increment the parse count.

use super::ledger;
use crate::domain::{FactKey, Result, Retain, StoreError, plain_label};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

pub(crate) fn retain(
    connection: &mut Connection,
    now_ms: u64,
    namespace: &str,
    root: &str,
    key: &FactKey,
    payload: &[u8],
) -> Result<Retain> {
    plain_label(namespace)?;
    plain_label(root)?;
    key.check()?;
    let charge = ledger::charge(payload.len())?;
    let key_text = key.to_string();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (now, _) = ledger::touch(&transaction, now_ms)?;
    let now_i = clock(now)?;
    let existing: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT payload FROM blob_facts WHERE namespace = ?1 AND fact_key = ?2",
            params![namespace, key_text],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing.as_slice() != payload {
            return Err(StoreError::FactConflict);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO blob_refs(namespace, root_id, fact_key) VALUES(?1, ?2, ?3)",
            params![namespace, root, key_text],
        )?;
        transaction.execute(
            "UPDATE blob_facts SET last_used = ?1, retired_at = NULL
             WHERE namespace = ?2 AND fact_key = ?3",
            params![now_i, namespace, key_text],
        )?;
        transaction.commit()?;
        return Ok(Retain::Reused);
    }
    ledger::reclaim(&transaction, now, None)?;
    ledger::admit(&transaction, charge, 0)?;
    transaction.execute(
        "INSERT INTO blob_facts(namespace, fact_key, payload, bytes, last_used, retired_at)
         VALUES(?1, ?2, ?3, ?4, ?5, NULL)",
        params![namespace, key_text, payload, charge, now_i],
    )?;
    transaction.execute(
        "INSERT INTO blob_refs(namespace, root_id, fact_key) VALUES(?1, ?2, ?3)",
        params![namespace, root, key_text],
    )?;
    transaction.execute(
        "UPDATE bridge_ledger SET parses = parses + 1 WHERE id = 1",
        [],
    )?;
    transaction.commit()?;
    Ok(Retain::Parsed)
}

pub(crate) fn retained(
    connection: &mut Connection,
    now_ms: u64,
    namespace: &str,
    key: &FactKey,
) -> Result<Option<Vec<u8>>> {
    plain_label(namespace)?;
    key.check()?;
    let key_text = key.to_string();
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (now, _) = ledger::touch(&transaction, now_ms)?;
    ledger::reclaim(&transaction, now, None)?;
    let payload: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT payload FROM blob_facts WHERE namespace = ?1 AND fact_key = ?2",
            params![namespace, key_text],
            |row| row.get(0),
        )
        .optional()?;
    if payload.is_some() {
        transaction.execute(
            "UPDATE blob_facts SET last_used = ?1 WHERE namespace = ?2 AND fact_key = ?3",
            params![clock(now)?, namespace, key_text],
        )?;
    }
    transaction.commit()?;
    Ok(payload)
}

pub(crate) fn release_root(
    connection: &mut Connection,
    now_ms: u64,
    namespace: &str,
    root: &str,
) -> Result<()> {
    plain_label(namespace)?;
    plain_label(root)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (now, _) = ledger::touch(&transaction, now_ms)?;
    transaction.execute(
        "DELETE FROM blob_refs WHERE namespace = ?1 AND root_id = ?2",
        params![namespace, root],
    )?;
    transaction.execute(
        "UPDATE blob_facts SET retired_at = ?1
         WHERE namespace = ?2 AND retired_at IS NULL AND NOT EXISTS (
            SELECT 1 FROM blob_refs AS link
            WHERE link.namespace = blob_facts.namespace AND link.fact_key = blob_facts.fact_key
         )",
        params![clock(now)?, namespace],
    )?;
    ledger::reclaim(&transaction, now, None)?;
    transaction.commit()?;
    Ok(())
}

fn clock(now_ms: u64) -> Result<i64> {
    i64::try_from(now_ms).map_err(|_| StoreError::InvalidKey("clock is out of range"))
}
