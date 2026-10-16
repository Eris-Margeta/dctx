//! Atomic publication of changed query facts after file-owned store transactions.
//! Implements SPEC-MAP-MODE sections 7 and 8 without rewriting unchanged facts.
//! Readers consume these rows and FTS through one pinned SQLite WAL snapshot.
use super::Index;
use crate::Result;
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

impl Index {
    pub(super) fn publish(&self, connection: &mut Connection) -> Result<()> {
        let transaction = connection.transaction()?;
        for path in &self.dirty_files {
            if let Some(file) = self.files.get(path) {
                transaction
                    .prepare_cached("INSERT OR REPLACE INTO query_files VALUES(?1,?2)")?
                    .execute(params![path, serde_json::to_string(file)?])?;
            } else {
                transaction
                    .prepare_cached("DELETE FROM query_files WHERE path=?1")?
                    .execute([path])?;
            }
        }
        for id in &self.dirty_nodes {
            if let Some(node) = self.nodes.get(id) {
                transaction
                    .prepare_cached("INSERT OR REPLACE INTO query_nodes VALUES(?1,?2,?3,?4,?5,?6)")?
                    .execute(params![
                        id.as_str(),
                        serde_json::to_string(node)?,
                        self.degree.get(id).copied().unwrap_or(0) as i64,
                        node.path,
                        node.kind.as_str(),
                        node.id.alias()
                    ])?;
            } else {
                transaction
                    .prepare_cached("DELETE FROM query_nodes WHERE id=?1")?
                    .execute([id.as_str()])?;
            }
        }
        let mut edges = BTreeMap::<&str, Vec<_>>::new();
        for edge in &self.edges {
            edges.entry(&edge.owner_path).or_default().push(edge);
        }
        for path in &self.dirty_edges {
            transaction
                .prepare_cached("DELETE FROM query_edges WHERE owner=?1")?
                .execute([path])?;
            if let Some(edges) = edges.get(path.as_str()) {
                for edge in edges {
                    transaction
                        .prepare_cached("INSERT INTO query_edges VALUES(?1,?2,?3,?4,?5)")?
                        .execute(params![
                            path,
                            edge.src.as_str(),
                            edge.dst.as_str(),
                            edge.kind.as_str(),
                            serde_json::to_string(edge)?
                        ])?;
                }
            }
        }
        transaction.execute(
            "INSERT OR REPLACE INTO query_cache VALUES(1,3,?1,0,?2,?3,?4)",
            params![
                self.place,
                serde_json::to_string(&self.snapshot)?,
                serde_json::to_string(&self.report.stale)?,
                serde_json::to_string(&self.moves)?
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }
}
