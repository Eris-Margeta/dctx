// SPDX-License-Identifier: Apache-2.0
//! Verify the shared subprocess directory mapping on every platform.

mod common;

use std::{collections::BTreeMap, ffi::OsString, process::Command};

#[test]
fn isolated_directory_environment_includes_windows_config_cache_and_data() {
    let root = tempfile::tempdir().expect("fixture root");
    let mut command = Command::new("unused");
    command.env_clear();
    common::isolated_dirs(&mut command, root.path());
    let env: BTreeMap<_, _> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.expect("set").to_owned()))
        .collect();
    for (key, folder) in [
        ("XDG_CONFIG_HOME", "config"),
        ("APPDATA", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_DATA_HOME", "data"),
        ("LOCALAPPDATA", "local-app-data"),
    ] {
        let path = root.path().join(folder);
        assert_eq!(
            env.get(&OsString::from(key)),
            Some(&path.as_os_str().to_owned()),
            "{key}"
        );
        assert!(path.is_dir(), "{key}");
    }
}
