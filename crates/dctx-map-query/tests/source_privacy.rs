// SPDX-License-Identifier: Apache-2.0
//! Privacy refusals for secrets, ignored files, and symlinks.
//! Implements SPEC-MAP-MODE sections 6.1 and 7 on the public source port.
//! The engine records content opens and leaves denied paths unread.
use dctx_map_query::FileSource;
use dctx_map_query::ports::Source;
use std::fs;

#[test]
fn secrets_ignored_files_and_symlinks_never_enter_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.rs"), "fn public() {}").unwrap();
    fs::write(dir.path().join(".env"), "SECRET").unwrap();
    fs::write(dir.path().join(".gitignore"), "private.rs\n").unwrap();
    fs::write(dir.path().join("private.rs"), "SECRET").unwrap();
    let source = FileSource::new(dir.path()).unwrap();
    let snapshot = source.snapshot().unwrap();
    assert!(snapshot.files.contains_key("lib.rs"));
    assert!(!snapshot.files.contains_key(".env"));
    assert!(!snapshot.files.contains_key("private.rs"));
    assert!(source.read(".env").is_err());
    assert!(source.read("../escape").is_err());
    assert!(source.read("private.rs").is_err());
    assert!(source.read("lib.rs").is_ok());
    assert!(
        source
            .opened_paths()
            .iter()
            .all(|path| !path.ends_with(".env") && !path.ends_with("private.rs"))
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("lib.rs"), dir.path().join("link.rs")).unwrap();
        assert!(source.read("link.rs").is_err());
        assert!(!source.snapshot().unwrap().files.contains_key("link.rs"));
    }
}

/// v1 card 1: the shared sensitive-name deny also keeps token and secrets files out of the map-query engine.
#[test]
fn token_and_secrets_files_never_enter_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("deploy")).unwrap();
    fs::write(dir.path().join("credentials.py"), "def load(): pass\n").unwrap();
    for name in [
        ".npmrc",
        "deploy/secrets.yaml",
        "settings.env",
        ".envrc",
        "store.p12",
    ] {
        fs::write(dir.path().join(name), "SECRET").unwrap();
    }
    let source = FileSource::new(dir.path()).unwrap();
    let snapshot = source.snapshot().unwrap();
    assert!(snapshot.files.contains_key("credentials.py"));
    for name in [
        ".npmrc",
        "deploy/secrets.yaml",
        "settings.env",
        ".envrc",
        "store.p12",
    ] {
        assert!(!snapshot.files.contains_key(name), "{name}");
        assert!(source.read(name).is_err(), "{name}");
    }
    assert!(source.read("credentials.py").is_ok());
}
