// SPDX-License-Identifier: Apache-2.0
//! One hostile file never fails the whole map walk (v1 card 10, robustness corpus).
//! A Rust file that is not UTF-8, and a file name the confined path type cannot carry (a backslash), are skipped;
//! every other file is still indexed. Before the fix either one made the snapshot, and so every map verb, fail.
//! Depends on [`dctx::FsSource`] over a private temporary directory and the shipped `syntax-rust` default.

#![forbid(unsafe_code)]

#[path = "common/mod.rs"]
mod common;

use dctx::{FsSource, SnapshotRequest, VecSink};

use common::open_session;

/// Indexes `root` and returns the ids `find okfn` answers. The snapshot itself must succeed.
fn found(root: &std::path::Path) -> Vec<String> {
    let mut session = open_session(FsSource::open(root).expect("open"));
    let handle = session
        .snapshot(&SnapshotRequest::immutable())
        .expect("one hostile file must not fail the snapshot");
    let mut sink = VecSink::new();
    let answer = session.find(&handle, "okfn", &mut sink).expect("find");
    answer.records.into_iter().map(|record| record.id).collect()
}

/// The positive control every fixture carries.
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp");
    std::fs::write(dir.path().join("ok.rs"), b"pub fn okfn() {}\n").expect("ok.rs");
    dir
}

#[test]
fn a_rust_file_that_is_not_utf8_is_skipped_not_fatal() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("latin1.rs"),
        b"pub fn caf\xe9() { okfn(); }\n",
    )
    .expect("latin-1 rust");
    let ids = found(dir.path());
    assert!(ids.iter().any(|id| id == "s:ok.rs#okfn"), "{ids:?}");
    assert!(ids.iter().all(|id| !id.contains("latin1")), "{ids:?}");
}

#[cfg(unix)]
#[test]
fn a_name_with_a_backslash_is_skipped_not_fatal() {
    let dir = fixture();
    std::fs::write(
        dir.path().join("back\\slash.rs"),
        b"pub fn okfn_hidden() {}\n",
    )
    .expect("backslash name");
    let ids = found(dir.path());
    assert!(ids.iter().any(|id| id == "s:ok.rs#okfn"), "{ids:?}");
    assert!(ids.iter().all(|id| !id.contains("slash")), "{ids:?}");
}

#[cfg(unix)]
#[test]
fn control_characters_and_invalid_utf8_names_do_not_enter_the_index() {
    use std::os::unix::ffi::OsStringExt;
    let dir = fixture();
    for name in [
        std::ffi::OsString::from("line\nbreak.rs"),
        std::ffi::OsString::from("escape\u{1b}.rs"),
        std::ffi::OsString::from_vec(b"bad\xff.rs".to_vec()),
    ] {
        match std::fs::write(dir.path().join(&name), b"pub fn okfn_hidden() {}\n") {
            Ok(()) => {}
            // APFS and other Unicode-enforcing filesystems refuse non-UTF-8 names (EILSEQ):
            // such a file cannot exist there, so there is nothing to keep out of the index.
            Err(error)
                if error.raw_os_error() == Some(libc_eilseq()) && name.to_str().is_none() => {}
            Err(error) => panic!("{name:?}: {error}"),
        }
    }
    let ids = found(dir.path());
    assert!(ids.iter().any(|id| id == "s:ok.rs#okfn"), "{ids:?}");
    assert!(!ids.iter().any(|id| id.contains("hidden")), "{ids:?}");
}

#[cfg(unix)]
fn libc_eilseq() -> i32 {
    rustix::io::Errno::ILSEQ.raw_os_error()
}
