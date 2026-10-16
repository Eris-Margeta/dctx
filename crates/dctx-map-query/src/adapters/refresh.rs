//! Coordinates metadata pre-filtering and exclusive incremental refreshes.
//! Implements SPEC-MAP-MODE section 7 with lock-free warm WAL readers.
//! Rechecks after acquiring the OS lock and repairs interrupted publications.
use super::{Index, IndexReport, database};
use crate::{
    Result,
    ports::{Snapshot, Source},
};
use dctx_map_store::SqliteStore;
use rusqlite::Connection;
use std::time::{Duration, Instant};

impl Index {
    /// Checks metadata first; hashes and reindexes only flagged authorized content.
    /// Persistent writers serialize with an OS lock released even after process death.
    pub fn refresh(
        &mut self,
        source: &impl Source,
        deadline: Option<Duration>,
    ) -> Result<&IndexReport> {
        let started = Instant::now();
        let captured = source.snapshot_since(self.snapshot.as_ref())?;
        if !self.needs_refresh(&captured) {
            self.snapshot = Some(captured);
            self.report = IndexReport {
                scan_time: started.elapsed(),
                ..IndexReport::default()
            };
            return Ok(&self.report);
        }
        let Some(path) = self.database.clone() else {
            return self.apply_snapshot(captured, started, deadline);
        };
        self.reader = None;
        let _writer = database::lock(&path)?;
        self.load()?;
        let mut snapshot = source.snapshot_since(Some(&captured))?;
        for (path, file) in &mut snapshot.files {
            if let Some(captured) = captured.files.get(path)
                && captured.stamp == file.stamp
                && captured.hash == file.hash
            {
                file.bytes = captured.bytes.clone();
            }
        }
        if !self.needs_refresh(&snapshot) {
            self.snapshot = Some(snapshot);
            self.report = IndexReport {
                scan_time: started.elapsed(),
                ..IndexReport::default()
            };
            return Ok(&self.report);
        }
        self.load_retry_facts()?;
        self.reader = None;
        let mut connection = Connection::open(&path)?;
        connection.execute("UPDATE query_cache SET writing=1 WHERE id=1", [])?;
        self.store = Some(SqliteStore::open(&path)?);
        self.apply_snapshot(snapshot, started, deadline)?;
        self.publish(&mut connection)?;
        self.store = None;
        self.reader = Some(database::reader(&path)?);
        // Pin FTS to the generation just published before releasing the writer.
        if let Some(reader) = &self.reader {
            reader.query_row("SELECT version FROM query_cache WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })?;
        }
        Ok(&self.report)
    }

    fn needs_refresh(&self, snapshot: &Snapshot) -> bool {
        if !self.ready || !self.report.stale.is_empty() {
            return true;
        }
        let Some(old) = &self.snapshot else {
            return true;
        };
        old.files.len() != snapshot.files.len()
            || old.directories != snapshot.directories
            || snapshot.rules.iter().any(|(path, rule)| {
                old.rules
                    .get(path)
                    .is_none_or(|old| old.stamp != rule.stamp)
            })
            || snapshot.files.iter().any(|(path, file)| {
                old.files.get(path).is_none_or(|old| {
                    old.stamp != file.stamp
                        || old.hash != file.hash
                        || old.readable != file.readable
                })
            })
    }
}
