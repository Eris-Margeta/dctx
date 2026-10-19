//! Loads and repairs a segment journal beside the sqlite comparison store.
//! Implements SPEC-MAP-MODE sections 7 and 8 without query-owned SQL tables.
//! A missing generation may still import a version-1 `query.json` cache.
//! Each live index owns one sqlite mirror. Drop and replacement delete the previous copy.
use super::segment;
use super::{Index, IndexReport};
use crate::{
    Error, ParseStatus, Result,
    domain::{cached::CachedFile, parse_status::PARSER_REVISION},
};
use dctx_adapter_fs::ports::SourceFs;
use dctx_adapter_fs::StdFs;
use dctx_adapter_store::ports::StoreFs;
use dctx_map_model::{Node, StableId};
use dctx_map_store::{MapStore, SqliteStore};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Deserialize, Serialize)]
struct LegacyCache {
    version: u32,
    place: String,
    files: BTreeMap<String, CachedFile>,
    moves: BTreeMap<StableId, StableId>,
}

impl Index {
    /// Opens the segment journal and sqlite comparison store for `database`.
    /// An empty store is a new index. A legacy `query.json` upgrades on refresh.
    pub fn open(place: impl Into<String>, database: impl AsRef<Path>) -> Result<Self> {
        let _guard = super::database::writer_lock();
        let path = database.as_ref();
        let place = place
            .into()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_");
        let disk = SqliteStore::open(path)?;
        let segments = segment::open_store(&path.with_extension("segments"), &place, path)?;
        let mut index = Self::with_store(place, None);
        index.disk = Some(disk);
        index.segments = Some(segments);
        index.database = Some(path.to_path_buf());
        index.load()?;
        Ok(index)
    }

    pub(super) fn load(&mut self) -> Result<()> {
        self.ready = false;
        self.writing = false;
        self.report = IndexReport::default();
        self.graph_loaded = true;
        self.files.clear();
        self.nodes.clear();
        self.edges.clear();
        self.moves.clear();
        self.snapshot = None;
        self.overlay = crate::Overlay::default();
        if let Some(journal) = segment::read_journal(self)? {
            self.install(journal)?;
        } else {
            self.load_legacy()?;
        }
        self.sync_mirror()?;
        Ok(())
    }

    fn install(&mut self, journal: super::journal::Journal) -> Result<()> {
        if journal.version != 3 || journal.place != self.place {
            return Err(Error::Request("query cache requires rebuild".into()));
        }
        let mut snapshot = journal.snapshot;
        let mut parser_changed = false;
        for (path, file) in &mut snapshot.files {
            let revision = journal
                .files
                .get(path)
                .map(|cached| cached.parser_revision)
                .unwrap_or(0);
            if revision != PARSER_REVISION {
                file.stamp = crate::ports::FileStamp::default();
                parser_changed = true;
            }
        }
        self.report = IndexReport::default();
        for (path, file) in &journal.files {
            if file.status != ParseStatus::Parsed {
                self.report.diagnostics.insert(path.clone(), file.status.clone());
            }
        }
        self.report.stale = journal.stale;
        self.files = journal.files;
        self.nodes = journal
            .nodes
            .into_iter()
            .map(|node| (node.id.clone(), node))
            .collect();
        self.edges = journal.edges;
        self.moves = journal.moves;
        self.overlay = journal.overlay;
        self.generation = journal.generation;
        self.writing = journal.writing;
        self.snapshot = Some(snapshot);
        self.ready = !self.writing && !parser_changed;
        self.graph_loaded = true;
        self.adjacency();
        Ok(())
    }

    fn load_legacy(&mut self) -> Result<()> {
        let Some(path) = self.database.clone() else {
            return Ok(());
        };
        let legacy = path.with_extension("query.json");
        match StdFs.read_file(&legacy) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
            Ok(bytes) => {
                let cache: LegacyCache = serde_json::from_slice(&bytes)?;
                if cache.version != 1 || cache.place != self.place {
                    return Err(Error::Request("query cache requires rebuild".into()));
                }
                self.files = cache.files;
                self.moves = cache.moves;
                self.ready = false;
                self.graph_loaded = false;
                Ok(())
            }
        }
    }

    fn sync_mirror(&mut self) -> Result<()> {
        let Some(database) = self.database.clone() else {
            return Ok(());
        };
        if self.disk.is_none() {
            return Ok(());
        }
        let parent = database
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .ok_or_else(|| Error::Request("database has no directory".into()))?;
        let name = database
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Request("database name is not utf-8".into()))?;
        let fs = dctx_adapter_store::StdFs::open(parent)?;
        let bytes = fs.read(name)?;
        let mirror_name = format!(
            "mirror-{}.sqlite",
            MIRROR_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let mirror_path = parent.join(&mirror_name);
        if let Err(error) = write_mirror(&fs, &mirror_name, &bytes) {
            let _ = fs.unlink(&mirror_name);
            return Err(error);
        }
        let opened = match SqliteStore::open(&mirror_path) {
            Ok(store) => store,
            Err(error) => {
                let _ = fs.unlink(&mirror_name);
                return Err(error.into());
            }
        };
        self.adopt_mirror(opened, mirror_path);
        Ok(())
    }

    pub(super) fn release_mirror(&mut self) {
        self.store = None;
        if let Some(path) = self.mirror.take() {
            remove_mirror(&path);
        }
    }

    fn adopt_mirror(&mut self, opened: SqliteStore, path: std::path::PathBuf) {
        let previous = self.mirror.replace(path);
        self.store = Some(opened);
        if let Some(old) = previous {
            remove_mirror(&old);
        }
    }

    /// JSON payload retained for one indexed path.
    pub fn file_payload(&self, path: &str) -> Result<String> {
        let file = self
            .files
            .get(path)
            .ok_or_else(|| Error::Request(format!("missing file payload: {path}")))?;
        Ok(serde_json::to_string(file)?)
    }

    /// Replaces one retained payload. A missing parser revision forces one reparse.
    pub fn replace_file_payload(&mut self, path: &str, json: &str) -> Result<()> {
        if self.database.is_none() {
            return Err(Error::Request("replace requires a database".into()));
        }
        let _guard = super::database::writer_lock();
        let file: CachedFile = serde_json::from_str(json)?;
        let revision = file.parser_revision;
        self.files.insert(path.to_string(), file);
        if revision != PARSER_REVISION
            && let Some(snapshot) = &mut self.snapshot
            && let Some(source) = snapshot.files.get_mut(path)
        {
            source.stamp = crate::ports::FileStamp::default();
        }
        self.ready = false;
        self.persist()?;
        Ok(())
    }

    /// Publishes the current journal as an interrupted generation and leaves sqlite unchanged.
    pub fn mark_publication_interrupted(&mut self) -> Result<()> {
        if self.segments.is_none() {
            return Err(Error::Request(
                "mark requires the database selected by Index::open".into(),
            ));
        }
        let _guard = super::database::writer_lock();
        self.writing = true;
        self.ready = false;
        self.persist()?;
        Ok(())
    }

    /// True when the segment journal and the sqlite comparison store describe this graph.
    pub fn stores_agree(&self) -> Result<bool> {
        if !self.graph_loaded {
            return Ok(false);
        }
        let store = self
            .disk
            .as_ref()
            .or(self.store.as_ref())
            .ok_or_else(|| Error::Request("index storage unavailable".into()))?;
        for (path, file) in &self.files {
            let Some(stored) = store.file(path)? else {
                return Ok(false);
            };
            if !file.hash.is_empty() && stored.file.sha256 != file.hash {
                return Ok(false);
            }
            if !same_nodes(&file.nodes, &stored.nodes) {
                return Ok(false);
            }
        }
        if self.segments.is_some() {
            let Some(journal) = segment::read_journal(self)? else {
                return Ok(false);
            };
            if journal.nodes.len() != self.nodes.len() || journal.edges.len() != self.edges.len() {
                return Ok(false);
            }
            for (path, file) in &self.files {
                if journal.files.get(path).is_none_or(|saved| saved.hash != file.hash) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
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

static MIRROR_SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn write_mirror(fs: &dctx_adapter_store::StdFs, name: &str, bytes: &[u8]) -> Result<()> {
    fs.create(name)?;
    fs.write(name, bytes)?;
    Ok(())
}

fn remove_mirror(path: &Path) {
    let mut sidecars = Vec::new();
    if let (Some(parent), Some(name)) = (
        path.parent(),
        path.file_name().and_then(|name| name.to_str()),
    ) {
        for suffix in ["-journal", "-wal", "-shm"] {
            sidecars.push(parent.join(format!("{name}{suffix}")));
        }
    }
    let _ = std::fs::remove_file(path);
    for sidecar in sidecars {
        let _ = std::fs::remove_file(sidecar);
    }
}

fn same_nodes(expected: &[Node], stored: &[Node]) -> bool {
    let mut left: Vec<_> = expected.iter().map(|node| node.id.as_str()).collect();
    let mut right: Vec<_> = stored
        .iter()
        .filter(|node| {
            !matches!(
                node.kind,
                dctx_map_model::NodeKind::Place | dctx_map_model::NodeKind::Package
            )
        })
        .map(|node| node.id.as_str())
        .collect();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

#[cfg(test)]
mod tests {
    use crate::Index;

    fn mirror_files(dir: &std::path::Path) -> Vec<String> {
        let mut names = std::fs::read_dir(dir)
            .expect("cache dir")
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("mirror-"))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    #[test]
    fn each_live_index_owns_one_mirror_and_drop_removes_it() {
        let dir = tempfile::tempdir().expect("temp");
        let database = dir.path().join("index.sqlite");
        let first = Index::open("fixture", &database).expect("open");
        assert_eq!(mirror_files(dir.path()).len(), 1);
        let second = Index::open("fixture", &database).expect("open");
        assert_eq!(mirror_files(dir.path()).len(), 2);
        drop(first);
        assert_eq!(mirror_files(dir.path()).len(), 1);
        drop(second);
        assert!(mirror_files(dir.path()).is_empty());
        for _ in 0..6 {
            drop(Index::open("fixture", &database).expect("reopen"));
            assert!(mirror_files(dir.path()).is_empty());
        }
    }

    #[test]
    fn refresh_replaces_the_mirror_instead_of_keeping_every_copy() {
        let root = tempfile::tempdir().expect("root");
        let cache = tempfile::tempdir().expect("cache");
        std::fs::write(root.path().join("a.rs"), "pub fn a() {}\n").expect("fixture");
        let source = crate::FileSource::new(root.path()).expect("source");
        let database = cache.path().join("index.sqlite");
        let mut index = Index::open("fixture", &database).expect("open");
        for generation in 0..4 {
            std::fs::write(
                root.path().join("a.rs"),
                format!("pub fn a() {{ let _ = {generation}; }}\n"),
            )
            .expect("edit");
            index.refresh(&source, None).expect("refresh");
            assert_eq!(mirror_files(cache.path()).len(), 1, "refresh {generation}");
        }
        drop(index);
        assert!(mirror_files(cache.path()).is_empty());
    }
}
