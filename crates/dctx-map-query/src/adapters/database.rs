//! WAL snapshot and advisory writer-lock adapter for the map cache.
//! Implements SPEC-MAP-MODE section 8; graph writes remain with MapStore.
//! FTS reads use the documented v1 SQLite schema on the reader's own snapshot.
use crate::Result;
use dctx_map_model::Node;
use dctx_map_store::SqliteStore;
use rusqlite::{Connection, OpenFlags, params};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};

pub(super) fn lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_extension("writer.lock"))?;
    file.lock()?;
    Ok(file)
}

pub(super) fn reader(path: &Path) -> Result<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(Duration::from_secs(30))?;
    connection.execute_batch("BEGIN DEFERRED")?;
    Ok(connection)
}

fn initialized(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let connection = reader(path)?;
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='query_cache')",
        [],
        |r| r.get(0),
    )?)
}

pub(super) fn initialize(path: &Path) -> Result<()> {
    if initialized(path)? {
        return Ok(());
    }
    let _writer = lock(path)?;
    if initialized(path)? {
        return Ok(());
    }
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(30))?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    let _store = SqliteStore::open(path)?;
    connection.execute_batch(
        "BEGIN;
         CREATE TABLE IF NOT EXISTS query_cache(
            id INTEGER PRIMARY KEY,version INTEGER NOT NULL,place TEXT NOT NULL,
            writing INTEGER NOT NULL,snapshot TEXT NOT NULL,stale TEXT NOT NULL,moves TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS query_files(path TEXT PRIMARY KEY,payload TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS query_nodes(id TEXT PRIMARY KEY,payload TEXT NOT NULL,degree INTEGER NOT NULL,
             path TEXT NOT NULL,kind TEXT NOT NULL,alias TEXT);
         CREATE INDEX IF NOT EXISTS query_node_path ON query_nodes(path,kind);
         CREATE INDEX IF NOT EXISTS query_node_alias ON query_nodes(alias);
         CREATE TABLE IF NOT EXISTS query_edges(owner TEXT NOT NULL,src TEXT NOT NULL,dst TEXT NOT NULL,
             kind TEXT NOT NULL,payload TEXT NOT NULL);
         CREATE INDEX IF NOT EXISTS query_edge_owner ON query_edges(owner);
         CREATE INDEX IF NOT EXISTS query_edge_src ON query_edges(src,kind);
         CREATE INDEX IF NOT EXISTS query_edge_dst ON query_edges(dst,kind);
         COMMIT;",
    )?;
    Ok(())
}

pub(super) fn search(connection: &Connection, expression: &str) -> Result<Vec<(Node, f64, usize)>> {
    let mut statement = connection.prepare_cached(
        "SELECT n.payload,bm25(fts),n.degree FROM fts JOIN query_nodes n ON n.id=fts.id
         WHERE fts MATCH ?1 ORDER BY bm25(fts),fts.id LIMIT ?2",
    )?;
    let rows = statement.query_map(params![expression, 40], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, f64>(1)?,
            r.get::<_, usize>(2)?,
        ))
    })?;
    rows.map(|row| {
        let (payload, score, degree) = row?;
        Ok((serde_json::from_str(&payload)?, score, degree))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileSource, Index, Request, Verb};
    use std::{
        fs,
        io::{BufRead, Write},
        process::{Command, Stdio},
    };

    #[test]
    fn lock_holder() {
        let Some(path) = std::env::var_os("DCTX_TEST_LOCK_HOLDER") else {
            return;
        };
        let _lock = lock(Path::new(&path)).unwrap();
        println!("LOCKED");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }

    #[test]
    fn killed_holder_and_legacy_lock_file_do_not_block_refresh() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let db = cache.path().join("index.sqlite");
        fs::write(root.path().join("lib.rs"), "pub fn live() {}").unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "adapters::database::tests::lock_holder",
                "--nocapture",
            ])
            .env("DCTX_TEST_LOCK_HOLDER", &db)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        assert!(output.lines().any(|line| line.unwrap() == "LOCKED"));
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(db.with_extension("writer.lock"))
            .unwrap();
        assert!(matches!(
            probe.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        child.kill().unwrap();
        child.wait().unwrap();
        probe.try_lock().unwrap();
        drop(probe);
        assert!(db.with_extension("writer.lock").exists());
        fs::write(cache.path().join("writer.lock"), "dead legacy pid").unwrap();
        let source = FileSource::new(root.path()).unwrap();
        let mut index = Index::open("fixture", &db).unwrap();
        let result = index
            .query(&source, &Request::new(Verb::Find, "live"))
            .unwrap();
        assert!(
            result
                .records
                .iter()
                .any(|r| r.id.as_str() == "s:lib.rs#live")
        );
    }
}
