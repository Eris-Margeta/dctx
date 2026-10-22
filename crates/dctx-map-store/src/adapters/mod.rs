//! SQLite map store and the DX-C2 comparison bridge.
//! Implements spec sections 6.4, 7 and 8, plus `FactStore` and content-addressed facts.
//! Uses model facts, dctx-hash and the bridge tables. Consumed by query and pack rollback.
mod bridge;
mod read;
mod schema;
mod tokens;
mod write;

use crate::{MapStore, domain::{Result, FactKey, Retain, MaintainReport, FileFacts, SearchHit, File}};
use dctx_core::error::DctxError;
use dctx_core::facts::PackSnapshot;
use dctx_core::ids::ViewKey;
use dctx_ports::FactStore;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Versioned SQLite map database and comparison bridge.
///
/// Map verbs keep the v1 graph tables. Pack snapshots and content-addressed fact
/// blobs live beside them. The clock is injected through [`Self::set_clock`].
pub struct SqliteStore {
    pub(super) connection: Connection,
    now_ms: u64,
}

impl SqliteStore {
    /// Opens a caller-selected database, creating its schema if absent.
    ///
    /// # Errors
    /// Returns database-open, schema initialization or comparison-bridge migration errors.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Creates an isolated in-memory map database.
    ///
    /// # Errors
    /// Returns database initialization or schema migration errors.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        schema::initialize(&mut connection)?;
        bridge::ensure(&mut connection)?;
        Ok(Self { connection, now_ms: 0 })
    }

    /// Sets the clock used by publish, retain and cleaning. Time is never read from the OS.
    pub fn set_clock(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
    }

    /// How many retained `FactKeys` required a parse in this database.
    ///
    /// # Errors
    /// Returns a database or stored-counter decoding error.
    pub fn parse_count(&self) -> Result<u64> {
        bridge::parse_count(&self.connection)
    }

    /// Accounted bytes of pack snapshots and blob facts, shared facts counted once.
    ///
    /// # Errors
    /// Returns a database, stored-size decoding or accounting-overflow error.
    pub fn cached_bytes(&self) -> Result<u64> {
        bridge::cached_bytes(&self.connection)
    }

    /// Hard cap in force. A fresh store uses the stage-0 2 GiB cap.
    ///
    /// # Errors
    /// Returns a database or stored-cap decoding error.
    pub fn cache_cap(&self) -> Result<u64> {
        bridge::cache_cap(&self.connection)
    }

    /// Lowers or raises the hard cap, then runs the cleaner at the stored clock.
    ///
    /// # Errors
    /// Returns a database, accounting or cache-cleaning error.
    pub fn set_cache_cap(&mut self, cap: u64) -> Result<()> {
        bridge::set_cap(&mut self.connection, cap)
    }

    /// Stores path-independent bytes for `key` in one trust namespace.
    ///
    /// The first root parses. Later roots with the same retained key reuse that payload.
    ///
    /// # Errors
    /// Refuses invalid identities, conflicting payloads or cap overflow; propagates database and cleaning errors.
    pub fn retain_fact(
        &mut self,
        namespace: &str,
        root: &str,
        key: &FactKey,
        payload: &[u8],
    ) -> Result<Retain> {
        bridge::retain(&mut self.connection, self.now_ms, namespace, root, key, payload)
    }

    /// Returns the retained payload for `key` after applying expiry and pressure.
    ///
    /// # Errors
    /// Refuses invalid keys or namespaces; propagates database, decoding and cleaning errors.
    pub fn retained_fact(&mut self, namespace: &str, key: &FactKey) -> Result<Option<Vec<u8>>> {
        bridge::retained(&mut self.connection, self.now_ms, namespace, key)
    }

    /// Drops one root's references and retires facts no root still uses.
    ///
    /// # Errors
    /// Refuses invalid namespace/root labels; propagates database and cleaning errors.
    pub fn release_root(&mut self, namespace: &str, root: &str) -> Result<()> {
        bridge::release_root(&mut self.connection, self.now_ms, namespace, root)
    }

    /// Applies time expiry and the size cap at the injected clock.
    ///
    /// # Errors
    /// Returns database, decoding, accounting or cache-cleaning errors.
    pub fn maintain(&mut self) -> Result<MaintainReport> {
        bridge::maintain(&mut self.connection, self.now_ms)
    }
}

impl FactStore for SqliteStore {
    fn load(&self, view: &ViewKey) -> std::result::Result<Option<PackSnapshot>, DctxError> {
        bridge::load(&self.connection, view).map_err(bridge::to_dctx)
    }

    fn publish(
        &mut self,
        view: &ViewKey,
        snapshot: &PackSnapshot,
    ) -> std::result::Result<(), DctxError> {
        bridge::publish(&mut self.connection, self.now_ms, view, snapshot).map_err(bridge::to_dctx)
    }
}

impl MapStore for SqliteStore {
    fn reindex(&mut self, facts: &FileFacts) -> Result<()> {
        write::validate(facts)?;
        let transaction = self.connection.transaction()?;
        write::remove(&transaction, &facts.file.path)?;
        write::insert(&transaction, facts)?;
        transaction.commit()?;
        Ok(())
    }

    fn prune(&mut self, live_paths: &[&str]) -> Result<usize> {
        let transaction = self.connection.transaction()?;
        let paths = {
            let mut statement = transaction.prepare("SELECT path FROM files ORDER BY path")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut removed = 0;
        for path in paths {
            if !live_paths.contains(&path.as_str()) {
                write::remove(&transaction, &path)?;
                removed += 1;
            }
        }
        transaction.commit()?;
        Ok(removed)
    }

    #[allow(clippy::cast_possible_wrap, reason = "The SQL result limit is clamped to 40 before converting it to i64.")]
    fn search(&self, expression: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let mut statement = self.connection.prepare_cached(
            "SELECT id,bm25(fts) FROM fts WHERE fts MATCH ?1 ORDER BY bm25(fts),id LIMIT ?2",
        )?;
        let rows = statement.query_map(params![expression, limit.min(40) as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        rows.map(|row| {
            let (id, score) = row?;
            Ok(SearchHit {
                id: id.parse()?,
                score,
            })
        })
        .collect()
    }

    fn file(&self, path: &str) -> Result<Option<FileFacts>> {
        let file = self
            .connection
            .query_row(
                "SELECT path,size,mtime,sha256,lang FROM files WHERE path=?1",
                [path],
                |row| {
                    Ok(File {
                        path: row.get(0)?,
                        size: row.get(1)?,
                        mtime: row.get(2)?,
                        sha256: row.get(3)?,
                        lang: row.get(4)?,
                    })
                },
            )
            .optional()?;
        file.map(|file| read::facts(&self.connection, file))
            .transpose()
    }
}

#[cfg(test)]
mod tests;
