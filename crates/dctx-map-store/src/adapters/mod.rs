//! SQLite implementation of the map persistence port.
//! Implements spec sections 6.4, 7 and 8 with bundled SQLite.
//! Uses model facts and schema helpers; consumed by query and index layers.
mod read;
mod schema;
mod tokens;
mod write;

use crate::{MapStore, domain::*};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Versioned SQLite map database; no filesystem paths are chosen implicitly.
pub struct SqliteStore {
    pub(super) connection: Connection,
}

impl SqliteStore {
    /// Opens a caller-selected database, creating its schema if absent.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_connection(Connection::open(path)?)
    }

    /// Creates an isolated in-memory map database.
    pub fn in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(mut connection: Connection) -> Result<Self> {
        schema::initialize(&mut connection)?;
        Ok(Self { connection })
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
