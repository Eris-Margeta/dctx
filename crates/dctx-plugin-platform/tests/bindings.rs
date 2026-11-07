// SPDX-License-Identifier: Apache-2.0
//! Real-filesystem adversaries for PP2/PP9 and the PL-R binding contract.
//! Successful immutable snapshots accompany substitution, hardlink and symlink refusals.
//! Expected digests represent host-authorized bytes; no tests use developer project/home data.

#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use dctx_hash::Sha256;
use dctx_plugin_platform::linux::RootHandle;
use dctx_plugin_platform::{PlatformError, RelativePath, Unsupported};
use std::fs::{self, File};
use std::os::unix::fs::symlink;

fn root(path: &std::path::Path) -> RootHandle {
    RootHandle::new(File::open(path).unwrap()).unwrap()
}

#[test]
fn verified_copy_survives_executable_path_and_inode_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("worker");
    fs::write(&executable, b"verified image").unwrap();
    let root = root(dir.path());
    let path = RelativePath::new("worker").unwrap();
    let identity = root.identity(&path).unwrap();
    let digest = Sha256::digest(b"verified image").unwrap();
    let pinned = root.snapshot(&path, identity, digest).unwrap();
    assert_eq!(pinned.digest(), digest);
    assert_eq!(pinned.read(0, 14).unwrap(), b"verified image");
    fs::write(&executable, b"malicious data").unwrap();
    assert!(matches!(
        root.snapshot(&path, identity, digest),
        Err(PlatformError::Conflict)
    ));
    assert_eq!(pinned.read(0, 14).unwrap(), b"verified image");
    fs::write(dir.path().join("replacement"), b"verified image").unwrap();
    fs::rename(dir.path().join("replacement"), &executable).unwrap();
    assert!(matches!(
        root.snapshot(&path, identity, digest),
        Err(PlatformError::Conflict)
    ));
    let new_identity = root.identity(&path).unwrap();
    assert_ne!(identity, new_identity);
    assert!(root.snapshot(&path, new_identity, digest).is_ok());
}

#[test]
fn hardlinks_symlinks_and_nonregular_objects_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("authorized"), b"allowed").unwrap();
    fs::create_dir(dir.path().join("directory")).unwrap();
    let root = root(dir.path());
    let allowed = RelativePath::new("authorized").unwrap();
    let expected = root.identity(&allowed).unwrap();
    let digest = Sha256::digest(b"allowed").unwrap();
    assert!(root.snapshot(&allowed, expected, digest).is_ok());
    symlink("authorized", dir.path().join("link")).unwrap();
    symlink(dir.path(), dir.path().join("ancestor")).unwrap();
    for path in ["link", "ancestor/authorized", "directory"] {
        assert!(
            root.identity(&RelativePath::new(path).unwrap()).is_err(),
            "{path}"
        );
    }
    fs::hard_link(dir.path().join("authorized"), dir.path().join("hardlink")).unwrap();
    assert!(root.identity(&allowed).is_err());
    assert!(
        root.identity(&RelativePath::new("hardlink").unwrap())
            .is_err()
    );
    assert!(root.snapshot(&allowed, expected, digest).is_err());
}

#[test]
fn substitution_during_concurrent_resolution_never_delivers_outside_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret"), b"forbidden").unwrap();
    fs::write(dir.path().join("selected"), b"authorized").unwrap();
    let root = root(dir.path());
    let selected = RelativePath::new("selected").unwrap();
    let identity = root.identity(&selected).unwrap();
    let digest = Sha256::digest(b"authorized").unwrap();
    assert_eq!(
        root.snapshot(&selected, identity, digest)
            .unwrap()
            .read(0, 10)
            .unwrap(),
        b"authorized"
    );
    let base = dir.path().to_owned();
    let forbidden = outside.path().join("secret");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let started = barrier.clone();
    let writer = std::thread::spawn(move || {
        started.wait();
        for _ in 0..128 {
            symlink(&forbidden, base.join("next")).unwrap();
            fs::rename(base.join("next"), base.join("selected")).unwrap();
            fs::write(base.join("next"), b"authorized").unwrap();
            fs::rename(base.join("next"), base.join("selected")).unwrap();
        }
    });
    barrier.wait();
    for _ in 0..256 {
        if let Ok(snapshot) = root.snapshot(&selected, identity, digest) {
            assert_eq!(snapshot.read(0, 10).unwrap(), b"authorized");
        }
    }
    writer.join().unwrap();
    assert!(root.snapshot(&selected, identity, digest).is_err());
    assert_eq!(
        fs::read(outside.path().join("secret")).unwrap(),
        b"forbidden"
    );
}

#[test]
fn retained_root_survives_namespace_replacement_without_rebinding() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("project");
    fs::create_dir(&old).unwrap();
    fs::write(old.join("file"), b"authorized").unwrap();
    let root = root(&old);
    let path = RelativePath::new("file").unwrap();
    let identity = root.identity(&path).unwrap();
    fs::rename(&old, dir.path().join("retained")).unwrap();
    fs::create_dir(&old).unwrap();
    fs::write(old.join("file"), b"forbidden").unwrap();
    let snapshot = root
        .snapshot(&path, identity, Sha256::digest(b"authorized").unwrap())
        .unwrap();
    assert_eq!(snapshot.read(0, 10).unwrap(), b"authorized");
}

#[test]
fn conditional_replacement_refuses_before_write_or_recreation() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    fs::write(&file, b"original").unwrap();
    let root = root(dir.path());
    let path = RelativePath::new("file").unwrap();
    let identity = root.identity(&path).unwrap();
    assert!(matches!(
        root.replace(&path, identity, b"replacement"),
        Err(PlatformError::Unsupported(Unsupported::ConditionalReplace))
    ));
    assert_eq!(fs::read(&file).unwrap(), b"original");
    fs::remove_file(&file).unwrap();
    assert!(root.replace(&path, identity, b"replacement").is_err());
    assert!(!file.exists());
}
