//! Coordinates metadata pre-filtering and exclusive incremental refreshes.
//! Implements SPEC-MAP-MODE section 7. Warm readers do not take the writer mutex.
//! The mutex is in-process; segment publication locks only for its own publish call.
use super::{Index, IndexReport};
use crate::{
    Result,
    ports::{Snapshot, Source},
};
use std::time::{Duration, Instant};

impl Index {
    /// Checks metadata first; hashes and reindexes only flagged authorized content.
    /// Disk writers serialize. A warm index returns without taking that mutex.
    pub fn refresh(
        &mut self,
        source: &impl Source,
        deadline: Option<Duration>,
    ) -> Result<&IndexReport> {
        if self.cancelled() {
            return Err(crate::Error::Request("cancelled".into()));
        }
        let started = Instant::now();
        let captured = source.snapshot_since(self.snapshot.as_ref())?;
        if !self.needs_refresh(&captured) {
            return Ok(self.fresh_report(captured, started));
        }
        let Some(path) = self.database.clone() else {
            return self.apply_snapshot(captured, started, deadline);
        };
        let _writer = super::database::writer_lock();
        let _ = path;
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
            return Ok(self.fresh_report(snapshot, started));
        }
        self.writing = true;
        self.persist()?;
        self.apply_snapshot(snapshot, started, deadline)?;
        self.writing = false;
        self.persist()?;
        Ok(&self.report)
    }

    fn fresh_report(&mut self, snapshot: Snapshot, started: Instant) -> &IndexReport {
        self.snapshot = Some(snapshot);
        self.report = IndexReport {
            scan_time: started.elapsed(),
            diagnostics: self.report.diagnostics.clone(),
            ..IndexReport::default()
        };
        &self.report
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
