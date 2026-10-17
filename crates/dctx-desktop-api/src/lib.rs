//! Desktop engine seam for snapshot, settings and recent projects.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4 (first half, DX-E4a).
//! Depends on the `dctx` facade, serde and serde_json. Tauri glue stays in the app.
//! Callers inject every directory and every recent-project timestamp. Git is never spawned (decision 078).

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod error;
mod recent;
mod settings;
mod snapshot;

pub use error::DesktopError;
pub use recent::{RecentProject, RecentStore, MAX_RECENT_PROJECTS};
pub use settings::{
    ensure_config_dirs, load_desktop_config, reset_desktop_config, save_desktop_config, DesktopConfig,
    DESKTOP_APP_NAME, DESKTOP_CONFIG_FILENAME,
};
pub use snapshot::{generate_snapshot, SnapshotResponse};

#[cfg(test)]
mod isolation {
    #[test]
    fn sources_do_not_spawn_or_discover_a_home() {
        let files: &[(&str, &str)] = &[
            ("lib.rs", include_str!("lib.rs")),
            ("error.rs", include_str!("error.rs")),
            ("settings.rs", include_str!("settings.rs")),
            ("recent.rs", include_str!("recent.rs")),
            ("snapshot.rs", include_str!("snapshot.rs")),
        ];
        let needles = [
            ["std::", "process"].concat(),
            ["Command", "::new"].concat(),
            ["env", "::var"].concat(),
            ["home", "_dir"].concat(),
            ["dirs", "::"].concat(),
            ["print", "ln!"].concat(),
            ["eprint", "ln!"].concat(),
        ];
        for (name, source) in files {
            for needle in &needles {
                assert!(!source.contains(needle), "{name} contains a forbidden call");
            }
        }
    }
}
