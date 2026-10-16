//! Query-owned cache metadata beside the caller-selected SQLite store.
//! Implements SPEC-MAP-MODE section 7 without reaching into store internals.
//! Persists lossless retry inputs and moves; refreshed store facts repair interrupted writes.
use super::Index;
use crate::{Error, Result, domain::cached::CachedFile};
use dctx_map_model::StableId;
use dctx_map_store::SqliteStore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Serialize, Deserialize)]
struct Cache {
    version: u32,
    place: String,
    files: BTreeMap<String, CachedFile>,
    moves: BTreeMap<StableId, StableId>,
}
impl Index {
    /// Opens explicit storage; source directories are never selected as cache locations.
    /// Callers serialize access to this database and its companion metadata file.
    pub fn open(place: impl Into<String>, database: impl AsRef<Path>) -> Result<Self> {
        let path = database.as_ref();
        let mut index = Self::with_store(place.into(), SqliteStore::open(path)?);
        match fs::read(path.with_extension("query.json")) {
            Ok(bytes) => {
                let cache: Cache = serde_json::from_slice(&bytes)?;
                if cache.version != 1 || cache.place != index.place {
                    return Err(Error::Request("query cache requires rebuild".into()));
                }
                index.files = cache.files;
                index.moves = cache.moves;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(index)
    }

    /// Persists retry facts and move history after a successful refresh.
    /// This writes only the caller-selected cache, never the source place.
    pub fn save(&self, database: impl AsRef<Path>) -> Result<()> {
        let cache = Cache {
            version: 1,
            place: self.place.clone(),
            files: self.files.clone(),
            moves: self.moves.clone(),
        };
        let path = database.as_ref().with_extension("query.json");
        let temp = path.with_extension(format!("query-{}.tmp", std::process::id()));
        fs::write(&temp, serde_json::to_vec(&cache)?)?;
        fs::rename(temp, path)?;
        Ok(())
    }
}
