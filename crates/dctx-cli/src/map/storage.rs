//! External per-worktree cache selection without reader exclusion.
//! Implements SPEC-MAP-MODE sections 7 and 8 without writing into source places.
//! Uses filesystem paths; dctx-map-query owns WAL snapshots and writer locks.
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
};
pub(super) struct Cache {
    pub database: PathBuf,
}
impl Cache {
    pub fn open(root: &Path) -> Result<Self, String> {
        let base = if let Some(path) = std::env::var_os("DCTX_MAP_CACHE") {
            PathBuf::from(path)
        } else if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
            PathBuf::from(path).join("dctx/map")
        } else if let Some(path) = std::env::var_os("HOME") {
            PathBuf::from(path).join(".cache/dctx/map")
        } else {
            return Err("set DCTX_MAP_CACHE to a cache directory outside the place".into());
        };
        if !base.is_absolute() {
            return Err("DCTX_MAP_CACHE must be absolute".into());
        }
        // Check existing ancestors before creating directories, including symlinks.
        let mut ancestor = base.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or("cache has no existing ancestor")?;
        }
        let ancestor = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
        if ancestor.starts_with(root) || base.starts_with(root) {
            return Err("map cache must live outside the source place".into());
        }
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
        let base = fs::canonicalize(base).map_err(|e| e.to_string())?;
        if base.starts_with(root) {
            return Err("map cache must live outside the source place".into());
        }
        let mut hash = DefaultHasher::new();
        root.hash(&mut hash);
        let dir = base.join(format!("{:016x}", hash.finish()));
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(Self {
            database: dir.join("index.sqlite"),
        })
    }
}

pub(super) fn with_place(
    answer: &dctx_map_query::Answer,
    request: &dctx_map_query::Request,
    format: dctx_map_query::Format,
    place: &Path,
) -> Result<String, String> {
    dctx_map_query::render_answer_at(
        answer,
        request,
        format,
        Some(place.to_string_lossy().as_ref()),
    )
    .map_err(|e| e.to_string())
}
