//! Persistent query metadata and lazy graph loading from a pinned WAL snapshot.
//! Implements SPEC-MAP-MODE sections 7 and 8 with query-owned SQLite tables.
//! Find reads bounded candidates; traversal and writers explicitly load the graph.
use super::{Index, database};
use crate::{Error, Result, domain::cached::CachedFile, ports::Snapshot};
use dctx_map_model::{Edge, Node};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize, Serialize)]
struct LegacyCache {
    version: u32,
    place: String,
    files: BTreeMap<String, CachedFile>,
    moves: BTreeMap<dctx_map_model::StableId, dctx_map_model::StableId>,
}

impl Index {
    /// Opens a consistent WAL reader, initializing a missing cache under a writer lock.
    /// Existing lock-file contents from older builds have no locking authority.
    pub fn open(place: impl Into<String>, database: impl AsRef<Path>) -> Result<Self> {
        let path = database.as_ref();
        database::initialize(path)?;
        let mut index = Self::with_store(place.into(), None);
        index.database = Some(path.into());
        index.load()?;
        Ok(index)
    }

    pub(super) fn load(&mut self) -> Result<()> {
        self.reader = None;
        let path = self
            .database
            .as_ref()
            .ok_or_else(|| Error::Request("missing cache path".into()))?;
        let connection = database::reader(path)?;
        let row = connection
            .query_row(
                "SELECT version,place,writing,snapshot,stale,moves FROM query_cache WHERE id=1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, bool>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        self.ready = false;
        self.graph_loaded = true;
        self.files.clear();
        self.nodes.clear();
        self.edges.clear();
        self.moves.clear();
        self.snapshot = None;
        self.dirty_files.clear();
        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        if let Some((version, place, writing, snapshot, stale, moves)) = row {
            if version != 3 || place != self.place {
                return Err(Error::Request("query cache requires rebuild".into()));
            }
            let snapshot: Snapshot = serde_json::from_str(&snapshot)?;
            self.files = snapshot
                .files
                .iter()
                .map(|(path, file)| {
                    (
                        path.clone(),
                        CachedFile {
                            hash: file.hash.clone(),
                            size: file.size,
                            mtime: file.mtime,
                            readable: file.readable,
                            nodes: vec![],
                            edges: vec![],
                            references: vec![],
                        },
                    )
                })
                .collect();
            self.report.stale = serde_json::from_str(&stale)?;
            self.moves = serde_json::from_str(&moves)?;
            self.snapshot = Some(snapshot);
            self.ready = !writing;
            self.graph_loaded = false;
        } else {
            match fs::read(path.with_extension("query.json")) {
                Ok(bytes) => {
                    let cache: LegacyCache = serde_json::from_slice(&bytes)?;
                    if cache.version != 1 || cache.place != self.place {
                        return Err(Error::Request("query cache requires rebuild".into()));
                    }
                    self.files = cache.files;
                    self.moves = cache.moves;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.reader = Some(connection);
        Ok(())
    }

    pub(super) fn load_graph(&mut self) -> Result<()> {
        if self.graph_loaded {
            return Ok(());
        }
        let reader = self
            .reader
            .as_ref()
            .ok_or_else(|| Error::Request("missing snapshot".into()))?;
        let mut nodes = reader.prepare("SELECT payload FROM query_nodes ORDER BY id")?;
        for row in nodes.query_map([], |r| r.get::<_, String>(0))? {
            let node: Node = serde_json::from_str(&row?)?;
            self.nodes.insert(node.id.clone(), node);
        }
        let mut edges = reader.prepare("SELECT payload FROM query_edges ORDER BY owner")?;
        for row in edges.query_map([], |r| r.get::<_, String>(0))? {
            self.edges.push(serde_json::from_str::<Edge>(&row?)?);
        }
        self.edges.sort_by(|a, b| {
            (&a.owner_path, &a.src, &a.dst, a.kind).cmp(&(&b.owner_path, &b.src, &b.dst, b.kind))
        });
        drop(nodes);
        drop(edges);
        self.adjacency();
        self.graph_loaded = true;
        Ok(())
    }

    pub(super) fn load_retry_facts(&mut self) -> Result<()> {
        if self.snapshot.is_none() {
            return Ok(());
        }
        self.load_graph()?;
        if let Some(connection) = &self.reader {
            let mut statement = connection.prepare("SELECT path,payload FROM query_files")?;
            self.files.clear();
            for row in
                statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            {
                let (path, payload) = row?;
                self.files.insert(path, serde_json::from_str(&payload)?);
            }
        }
        Ok(())
    }

    /// Refresh commits persistent caches automatically while holding the writer lock.
    /// Retained for callers that explicitly saved after refreshing in earlier releases.
    pub fn save(&self, database: impl AsRef<Path>) -> Result<()> {
        if self.database.as_deref() != Some(database.as_ref()) {
            return Err(Error::Request(
                "save requires the database selected by Index::open".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileSource, Request, Verb};

    #[test]
    fn legacy_retry_facts_and_moves_survive_cache_upgrade() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let database = cache.path().join("index.sqlite");
        fs::write(root.path().join("old.rs"), "pub fn work() {}").unwrap();
        let source = FileSource::new(root.path()).unwrap();
        let mut old = Index::in_memory("fixture").unwrap();
        old.refresh(&source, None).unwrap();
        old.moves.insert(
            "s:earlier.rs#work".parse().unwrap(),
            "s:old.rs#work".parse().unwrap(),
        );
        let legacy = LegacyCache {
            version: 1,
            place: old.place,
            files: old.files,
            moves: old.moves,
        };
        fs::write(
            database.with_extension("query.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .unwrap();
        fs::write(cache.path().join("writer.lock"), "abandoned legacy lock").unwrap();
        fs::rename(root.path().join("old.rs"), root.path().join("new.rs")).unwrap();
        let request = Request::new(Verb::Explain, "s:earlier.rs#work");
        let mut upgraded = Index::open("fixture", &database).unwrap();
        let answer = upgraded.query(&source, &request).unwrap();
        assert_eq!(answer.records[0].id.as_str(), "s:new.rs#work");
        drop(upgraded);
        let reads = source.content_reads();
        let mut reopened = Index::open("fixture", &database).unwrap();
        let answer = reopened.query(&source, &request).unwrap();
        assert_eq!(answer.records[0].id.as_str(), "s:new.rs#work");
        assert_eq!(source.content_reads(), reads);
    }
}
