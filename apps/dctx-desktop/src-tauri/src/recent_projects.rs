// SPDX-License-Identifier: Apache-2.0
//! Recent-project directory discovery and command wrappers.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4 (second half, DX-E4b).
//! Depends on `dctx_desktop_api::RecentStore`, the desktop config directory and `chrono` for the timestamp.
//! `main.rs` calls these wrappers. Dedup, order and the cap stay in the seam crate.

use dctx_desktop_api::{DesktopError, RecentStore};

/// One directory the desktop has opened.
pub use dctx_desktop_api::RecentProject;

fn command_error(error: DesktopError) -> String {
    error.command_string()
}

fn store() -> Result<RecentStore, String> {
    Ok(RecentStore::new(crate::config::desktop_config_dir()?))
}

/// UTC timestamp stored on [`RecentStore::add`].
fn accessed_now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Loads the recent-project list. A missing file is empty.
pub fn load_recent_projects() -> Result<Vec<RecentProject>, String> {
    store()?.get().map_err(command_error)
}

/// Records `path` with the current UTC timestamp.
pub fn add_recent_project(path: &str) -> Result<(), String> {
    let accessed_at = accessed_now();
    store()?.add(path, &accessed_at).map_err(command_error)
}

/// Drops `path` from the recent-project list.
pub fn remove_recent_project(path: &str) -> Result<(), String> {
    store()?.remove(path).map_err(command_error)
}

#[cfg(test)]
mod tests {
    use dctx_desktop_api::DesktopError;

    use super::{accessed_now, command_error};

    #[test]
    fn current_timestamp_is_utc_rfc3339() {
        let stamp = accessed_now();
        let parsed = chrono::DateTime::parse_from_rfc3339(&stamp).expect("rfc3339");
        assert_eq!(parsed.offset().local_minus_utc(), 0);
    }

    #[test]
    fn recent_errors_use_the_api_command_string() {
        let error = DesktopError::RecentPathMissing {
            path: "/missing".to_string(),
        };
        assert_eq!(command_error(error), "Path does not exist: /missing");
    }
}
