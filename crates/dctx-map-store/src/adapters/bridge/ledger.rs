// SPDX-License-Identifier: Apache-2.0
//! Byte accounting, injected clock and automatic cleaning for the comparison cache.
//! Implements CACHE-v1 admission (committed bytes ≤ cap) and time-then-LRU reclamation.
//! Depends on [`crate::domain::retention`] and the bridge tables. Used by fact and pack writes.
//! Shared fact bytes are charged once. Referenced facts are not deleted.

use crate::domain::{
    accounted_bytes, cache_limits, expired, idle, observed_clock, Charge, MaintainReport, Result,
    StoreError, UNREACHABLE_GRACE_MS, VIEW_IDLE_MS,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(crate) fn parse_count(connection: &Connection) -> Result<u64> {
    let parses: i64 = connection.query_row(
        "SELECT parses FROM bridge_ledger WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(parses).map_err(|_| StoreError::Corrupt("cache accounting"))
}

pub(crate) fn cached_bytes(connection: &Connection) -> Result<u64> {
    u64::try_from(sum_i64(connection)?).map_err(|_| StoreError::Corrupt("cache accounting"))
}

pub(crate) fn cache_cap(connection: &Connection) -> Result<u64> {
    u64::try_from(cap_i64(connection)?).map_err(|_| StoreError::Corrupt("cache accounting"))
}

pub(crate) fn set_cap(connection: &mut Connection, cap: u64) -> Result<()> {
    let cap_i = i64::try_from(cap).map_err(|_| StoreError::InvalidKey("cache cap is out of range"))?;
    let transaction = connection.transaction()?;
    transaction.execute("UPDATE bridge_ledger SET hard_cap = ?1 WHERE id = 1", [cap_i])?;
    let now = clock_u64(&transaction)?;
    reclaim(&transaction, now, None)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn maintain(connection: &mut Connection, now_ms: u64) -> Result<MaintainReport> {
    let transaction = connection.transaction()?;
    let (observed, rewound) = touch(&transaction, now_ms)?;
    let before = sum_i64(&transaction)?;
    reclaim(&transaction, observed, None)?;
    let after = sum_i64(&transaction)?;
    let report = report_after(&transaction, before, after, rewound)?;
    transaction.commit()?;
    Ok(report)
}

pub(crate) fn charge(len: usize) -> Result<i64> {
    let len = u64::try_from(len).map_err(|_| StoreError::InvalidKey("invalid fact identity"))?;
    let bytes = accounted_bytes(len).ok_or(StoreError::InvalidKey("invalid fact identity"))?;
    i64::try_from(bytes).map_err(|_| StoreError::CacheLimit { requested: bytes, available: 0 })
}

pub(crate) fn touch(transaction: &Transaction<'_>, now_ms: u64) -> Result<(u64, bool)> {
    let previous = clock_u64(transaction)?;
    let (observed, rewound) = observed_clock(previous, now_ms);
    if !rewound {
        let stored = i64::try_from(observed).map_err(|_| StoreError::InvalidKey("clock is out of range"))?;
        transaction.execute("UPDATE bridge_ledger SET last_clock = ?1 WHERE id = 1", [stored])?;
    }
    Ok((observed, rewound))
}

pub(crate) fn reclaim(
    transaction: &Transaction<'_>,
    now_ms: u64,
    exclude_view: Option<&str>,
) -> Result<()> {
    delete_expired(transaction, now_ms)?;
    delete_idle_views(transaction, now_ms, exclude_view)?;
    pressure(transaction)?;
    Ok(())
}

pub(crate) fn admit(transaction: &Transaction<'_>, additional: i64, replacing: i64) -> Result<()> {
    loop {
        let cap = cap_i64(transaction)?;
        let used = sum_i64(transaction)?;
        let base = used.saturating_sub(replacing.max(0));
        let available = cap.saturating_sub(base).max(0);
        if additional <= cap && additional <= available {
            return Ok(());
        }
        let limit = StoreError::CacheLimit {
            requested: u64::try_from(additional.max(0)).unwrap_or(u64::MAX),
            available: u64::try_from(available).unwrap_or(0),
        };
        if additional > cap || !evict_one(transaction)? || sum_i64(transaction)? >= used {
            return Err(limit);
        }
    }
}

fn report_after(
    transaction: &Transaction<'_>,
    before: i64,
    after: i64,
    rewound: bool,
) -> Result<MaintainReport> {
    let cap = cache_cap(transaction)?;
    let (soft, target) = cache_limits(cap);
    let after_u = u64::try_from(after).map_err(|_| StoreError::Corrupt("cache accounting"))?;
    let blocked = if after_u > soft { after_u.saturating_sub(target) } else { 0 };
    let reclaimed = u64::try_from(before.saturating_sub(after).max(0))
        .map_err(|_| StoreError::Corrupt("cache accounting"))?;
    Ok(MaintainReport { reclaimed_bytes: reclaimed, blocked_bytes: blocked, clock_rewound: rewound })
}

fn delete_expired(transaction: &Transaction<'_>, now_ms: u64) -> Result<()> {
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT namespace, fact_key, retired_at FROM blob_facts AS fact
             WHERE retired_at IS NOT NULL AND NOT EXISTS (
                SELECT 1 FROM blob_refs AS link
                WHERE link.namespace = fact.namespace AND link.fact_key = fact.fact_key
             )",
        )?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (namespace, key, retired) in rows {
        let fact = Charge { retired_at: Some(nonneg(retired)?), referenced: false };
        if expired(&fact, now_ms, UNREACHABLE_GRACE_MS) {
            delete_fact(transaction, &namespace, &key)?;
        }
    }
    Ok(())
}

fn delete_idle_views(
    transaction: &Transaction<'_>,
    now_ms: u64,
    exclude_view: Option<&str>,
) -> Result<()> {
    let rows = {
        let mut statement = transaction.prepare("SELECT view_key, last_used FROM pack_snapshots")?;
        statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (view, last_used) in rows {
        if exclude_view == Some(view.as_str()) {
            continue;
        }
        if idle(nonneg(last_used)?, now_ms, VIEW_IDLE_MS) {
            transaction.execute("DELETE FROM pack_snapshots WHERE view_key = ?1", [&view])?;
        }
    }
    Ok(())
}

fn pressure(transaction: &Transaction<'_>) -> Result<()> {
    let cap = cache_cap(transaction)?;
    let (soft, target) = cache_limits(cap);
    let mut used = cached_bytes(transaction)?;
    if used <= soft {
        return Ok(());
    }
    while used > target {
        if !evict_one(transaction)? {
            break;
        }
        used = cached_bytes(transaction)?;
    }
    Ok(())
}

fn evict_one(transaction: &Transaction<'_>) -> Result<bool> {
    let row = transaction
        .query_row(
            "SELECT namespace, fact_key FROM blob_facts AS fact
             WHERE NOT EXISTS (
                SELECT 1 FROM blob_refs AS link
                WHERE link.namespace = fact.namespace AND link.fact_key = fact.fact_key
             )
             ORDER BY last_used ASC, namespace ASC, fact_key ASC
             LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((namespace, key)) = row else {
        return Ok(false);
    };
    Ok(delete_fact(transaction, &namespace, &key)? > 0)
}

pub(crate) fn delete_fact(transaction: &Transaction<'_>, namespace: &str, key: &str) -> Result<usize> {
    transaction.execute(
        "DELETE FROM blob_refs WHERE namespace = ?1 AND fact_key = ?2",
        params![namespace, key],
    )?;
    let removed = transaction.execute(
        "DELETE FROM blob_facts WHERE namespace = ?1 AND fact_key = ?2",
        params![namespace, key],
    )?;
    Ok(removed)
}

fn clock_u64(connection: &Connection) -> Result<u64> {
    let clock: i64 = connection.query_row(
        "SELECT last_clock FROM bridge_ledger WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    nonneg(clock)
}

fn cap_i64(connection: &Connection) -> Result<i64> {
    connection
        .query_row("SELECT hard_cap FROM bridge_ledger WHERE id = 1", [], |row| row.get(0))
        .map_err(StoreError::from)
}

fn sum_i64(connection: &Connection) -> Result<i64> {
    connection
        .query_row(
            "SELECT COALESCE(SUM(bytes), 0) FROM (
                SELECT bytes FROM blob_facts
                UNION ALL
                SELECT bytes FROM pack_snapshots
             ) AS stored",
            [],
            |row| row.get(0),
        )
        .map_err(StoreError::from)
}

fn nonneg(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt("cache accounting"))
}
