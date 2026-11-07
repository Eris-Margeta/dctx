// SPDX-License-Identifier: Apache-2.0
//! Platform directory mappings for CLI subprocesses after `env_clear()`.

use std::{fs, path::Path, process::Command};

/// Keep Unix and Windows config, cache/data, home and temporary state in the fixture.
pub fn isolated_dirs(command: &mut Command, root: &Path) {
    for (variable, folder) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("APPDATA", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("LOCALAPPDATA", "local-app-data"),
        ("TMPDIR", "tmp"),
        ("TMP", "tmp"),
        ("TEMP", "tmp"),
    ] {
        let path = root.join(folder);
        fs::create_dir_all(&path).expect("isolated directory");
        command.env(variable, path);
    }
}
