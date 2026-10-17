//! Desktop engine seam for snapshots, settings, recent projects and plugin management.
//! Implements ARCHITECTURE-PROPOSAL §7 DX-E4a and PLUGINS.md §7.3 PL-APP's backend.
//! Depends on the dctx facade, distribution, bundled catalog and serde. Tauri glue stays in the app.
//! Callers inject directories, timestamps and plugin ports. Git is never spawned (decision 078).

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

mod error;
mod plugins;
mod plugins_ops;
mod plugins_view;
mod recent;
mod settings;
mod snapshot;

pub use error::DesktopError;
pub use plugins::{
    bundled_plugin_version, PluginFailure, PluginOps, PluginRow, PluginSource, PluginState,
    PluginUpdate,
};
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
            ("plugins.rs", include_str!("plugins.rs")),
            ("plugins_ops.rs", include_str!("plugins_ops.rs")),
            ("plugins_view.rs", include_str!("plugins_view.rs")),
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
