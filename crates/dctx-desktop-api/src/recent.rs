// SPDX-License-Identifier: Apache-2.0
//! Recent projects: add, remove and get with dedup, newest-first order and a cap of 20.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a and review F18, matching `recent_projects.rs`.
//! Depends on [`crate::error::DesktopError`] and `serde_json`. The caller injects the storage directory and the timestamp.
//! Parse errors are `recent_parse` plus a bounded category. This module does not read a clock or a home directory.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::DesktopError;

/// Maximum recent projects kept on disk.
pub const MAX_RECENT_PROJECTS: usize = 20;

const RECENT_FILE: &str = "recent_projects.json";

/// Stable code stored on [`DesktopError::ParseRecent`]. Never a fragment of the file.
const RECENT_PARSE_CODE: &str = "recent_parse";

/// Largest line or column reported for a recent-project parse failure.
const PARSE_LOCATION_MAX: u32 = 1_000_000;

/// One directory the desktop has opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentProject {
    /// Path as supplied by the caller.
    pub path: String,
    /// Final path segment, or `Unknown` when the path has none.
    pub name: String,
    /// Caller-supplied timestamp. ISO-8601 strings sort newest-first.
    pub last_accessed: String,
    /// How many times `add` has seen this path.
    pub access_count: u32,
}

/// JSON file of recent projects under an injected directory.
#[derive(Debug, Clone)]
pub struct RecentStore {
    dir: PathBuf,
}

impl RecentStore {
    /// Stores `recent_projects.json` inside `storage_dir`. The directory is created on use.
    pub fn new(storage_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: storage_dir.into(),
        }
    }

    /// Loads the list. A missing file is an empty list.
    ///
    /// # Errors
    /// Propagates storage-directory creation, recent-file read and JSON decoding errors; a missing file is an empty list.
    pub fn get(&self) -> Result<Vec<RecentProject>, DesktopError> {
        let path = self.file_path()?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path).map_err(|error| DesktopError::ReadRecent {
            detail: error.to_string(),
        })?;
        serde_json::from_str(&content).map_err(|error| parse_recent_error(&error))
    }

    /// Inserts or refreshes `path`, then sorts newest-first and drops past the cap.
    ///
    /// `accessed_at` is stored as given. An existing path keeps its name, updates the timestamp
    /// and increments `access_count`.
    ///
    /// # Errors
    /// Refuses nonexistent paths and propagates recent-list loading, encoding and persistence failures.
    pub fn add(&self, path: &str, accessed_at: &str) -> Result<(), DesktopError> {
        if !Path::new(path).exists() {
            return Err(DesktopError::RecentPathMissing {
                path: path.to_string(),
            });
        }
        let mut projects = self.get()?;
        if let Some(existing) = projects.iter_mut().find(|project| project.path == path) {
            existing.last_accessed = accessed_at.to_string();
            existing.access_count = existing.access_count.saturating_add(1);
        } else {
            projects.push(RecentProject {
                path: path.to_string(),
                name: project_name(path),
                last_accessed: accessed_at.to_string(),
                access_count: 1,
            });
        }
        projects.sort_by(|left, right| right.last_accessed.cmp(&left.last_accessed));
        if projects.len() > MAX_RECENT_PROJECTS {
            projects.truncate(MAX_RECENT_PROJECTS);
        }
        self.save(&projects)
    }

    /// Drops the project whose path equals `path`.
    ///
    /// # Errors
    /// Refuses absent recent entries and propagates recent-list loading and persistence failures.
    pub fn remove(&self, path: &str) -> Result<(), DesktopError> {
        let mut projects = self.get()?;
        let original = projects.len();
        projects.retain(|project| project.path != path);
        if projects.len() == original {
            return Err(DesktopError::RecentMissing {
                path: path.to_string(),
            });
        }
        self.save(&projects)
    }

    fn file_path(&self) -> Result<PathBuf, DesktopError> {
        fs::create_dir_all(&self.dir).map_err(|error| DesktopError::CreateStorageDir {
            detail: error.to_string(),
        })?;
        Ok(self.dir.join(RECENT_FILE))
    }

    fn save(&self, projects: &[RecentProject]) -> Result<(), DesktopError> {
        let path = self.file_path()?;
        let json = serde_json::to_string_pretty(projects).map_err(|error| {
            DesktopError::SerializeRecent {
                detail: error.to_string(),
            }
        })?;
        fs::write(path, json).map_err(|error| DesktopError::WriteRecent {
            detail: error.to_string(),
        })
    }
}

fn parse_recent_error(error: &serde_json::Error) -> DesktopError {
    let category = match error.classify() {
        serde_json::error::Category::Io => "io",
        serde_json::error::Category::Syntax => "syntax",
        serde_json::error::Category::Data => "data",
        serde_json::error::Category::Eof => "eof",
    };
    DesktopError::ParseRecent {
        code: RECENT_PARSE_CODE,
        category,
        line: bound_location(error.line()),
        column: bound_location(error.column()),
    }
}

fn bound_location(value: usize) -> u32 {
    match u32::try_from(value) {
        Ok(value) if value <= PARSE_LOCATION_MAX => value,
        _ => PARSE_LOCATION_MAX,
    }
}

fn project_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path =
            std::env::temp_dir().join(format!("dctx-desktop-api-recent-{label}-{n}-{nanos}"));
        fs::create_dir_all(&path).expect("temp");
        path
    }

    #[test]
    fn project_json_round_trips() {
        let project = RecentProject {
            path: "/home/user/project".to_string(),
            name: "project".to_string(),
            last_accessed: "2025-01-15T10:30:00Z".to_string(),
            access_count: 5,
        };
        let json = serde_json::to_string(&project).expect("json");
        let parsed: RecentProject = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, project);
    }

    #[test]
    fn add_dedups_and_remove_drops_the_path() {
        let root = scratch("dedup");
        let project = root.join("app");
        fs::create_dir(&project).expect("project");
        let path = project.to_str().expect("utf8");
        let store = RecentStore::new(root.join("state"));
        assert_eq!(store.get().expect("empty"), [] as [RecentProject; 0]);
        store.add(path, "2026-01-01T00:00:00Z").expect("first");
        store.add(path, "2026-01-02T00:00:00Z").expect("second");
        let listed = store.get().expect("listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "app");
        assert_eq!(listed[0].access_count, 2);
        assert_eq!(listed[0].last_accessed, "2026-01-02T00:00:00Z");
        store.remove(path).expect("remove");
        assert_eq!(store.get().expect("gone"), [] as [RecentProject; 0]);
        let missing = store.remove(path).expect_err("missing");
        assert_eq!(
            missing.command_string(),
            format!("Project not found in recent list: {path}")
        );
        let _ = fs::remove_dir_all(&root);
    }
}
