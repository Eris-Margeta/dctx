// SPDX-License-Identifier: Apache-2.0
//! Deterministic public platform boundaries. String APIs cannot receive invalid UTF-8;
//! Unix file contents and configured environment values exercise arbitrary raw bytes.
//! Irreversible restriction runs only in a disposable probe process.

use dctx_plugin_platform::*;
use std::process::Command;

#[test]
fn relative_names_cover_empty_one_byte_exact_and_over_limits() {
    assert!(RelativePath::new("").is_err());
    for b in 0..=127 {
        let text = char::from(b).to_string();
        let accepted = b >= 0x20 && b != 0x7f && !b"/\\:*?\"<>|.".contains(&b) && b != b' ';
        assert_eq!(RelativePath::new(&text).is_ok(), accepted, "byte {b}");
    }
    assert!(RelativePath::new(&"a".repeat(255)).is_ok());
    assert!(RelativePath::new(&"a".repeat(256)).is_err());
    let exact = std::iter::repeat_n("a".repeat(255), 16)
        .collect::<Vec<_>>()
        .join("/")
        + "/a";
    // Sixteen 255-byte components plus separators use 4095 bytes.
    let exact = &exact[..4096];
    assert_eq!(exact.len(), 4096);
    // The 4096-byte spelling above ends in a separator and is ambiguous.
    assert!(RelativePath::new(exact).is_err());
    let exact = std::iter::repeat_n("a".repeat(254), 16)
        .collect::<Vec<_>>()
        .join("/")
        + "/"
        + &"b".repeat(16);
    assert_eq!(exact.len(), 4096);
    assert_eq!(RelativePath::new(&exact).unwrap().as_str(), exact);
    assert!(RelativePath::new(&(exact + "b")).is_err());
}

#[test]
fn every_small_path_truncation_and_ambiguous_component_is_safe() {
    let sample = "dir/file.txt";
    for end in 0..=sample.len() {
        let result = RelativePath::new(&sample[..end]);
        assert_eq!(result.is_ok(), end != 0 && end != 4 && end != 9);
        if let Ok(path) = result {
            assert_eq!(RelativePath::new(path.as_str()).unwrap(), path);
        }
    }
    for bad in [
        ".", "..", "a/../b", "a//b", "/a", "a/", "a\\b", "a\0b", "a\u{7f}", "é", "CON", "con.txt",
        "COM1.dat", "lpt9", "a.", "a ",
    ] {
        assert!(RelativePath::new(bad).is_err(), "{bad:?}");
    }
    for good in ["a/b", "b/a", "a/a", ".gitignore", "COM0", "lpt10"] {
        let first = RelativePath::new(good).unwrap();
        assert_eq!(RelativePath::new(good).unwrap(), first);
    }
}

#[test]
fn environment_cleanup_is_repeatable_with_duplicate_and_reordered_values() {
    for keys in [
        ["TOKEN", "LANG", "LC_ALL", "LD_PRELOAD"],
        ["LD_PRELOAD", "LC_ALL", "LANG", "TOKEN"],
    ] {
        let mut command = Command::new("unused");
        for key in keys {
            command.env(key, "hostile").env(key, "replacement");
        }
        NoSecretEnvironment.apply(&mut command);
        let first: Vec<_> = command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
            .collect();
        NoSecretEnvironment.apply(&mut command);
        let second: Vec<_> = command
            .get_envs()
            .map(|(k, v)| (k.to_owned(), v.map(ToOwned::to_owned)))
            .collect();
        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert!(
            first
                .iter()
                .all(|(k, v)| (k == "LANG" || k == "LC_ALL") && v.as_deref() == Some("C".as_ref()))
        );
    }
    let expected =
        std::env::vars_os().all(|(key, value)| (key == "LANG" || key == "LC_ALL") && value == "C");
    assert_eq!(NoSecretEnvironment.is_clean(), expected);
}

#[test]
fn provider_refusals_and_error_categories_are_stable_under_concurrent_use() {
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    for kind in [
                        LaunchKind::ThirdParty,
                        LaunchKind::BundledGit,
                        LaunchKind::GitRecipe,
                    ] {
                        let provider = NativeProvider;
                        assert_eq!(provider.support(kind), provider.support(kind));
                        assert_eq!(provider.support(kind).target, Target::current());
                        let first = provider.launch(kind).unwrap_err();
                        let second = provider.launch(kind).unwrap_err();
                        assert_eq!(first.to_string(), second.to_string());
                        assert_eq!(
                            first.code(),
                            dctx_plugin_protocol::ErrorCode::UnsupportedPlatform
                        );
                    }
                    assert!(macos::require_provider().is_err());
                    assert_eq!(macos::DENY_ALL_PROFILE, "(version 1)\n(deny default)\n");
                    for path in ["", "a", "a/../b", "é"] {
                        assert_eq!(
                            RelativePath::new(path)
                                .map(|p| p.as_str().to_owned())
                                .map_err(|e| e.to_string()),
                            RelativePath::new(path)
                                .map(|p| p.as_str().to_owned())
                                .map_err(|e| e.to_string())
                        );
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use dctx_hash::{ContentDigest, Sha256};
    use dctx_plugin_platform::linux::{MAX_SNAPSHOT_BYTES, RestrictedEntry, RootHandle};
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::process::Stdio;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(name: &str) -> Self {
            let base = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/n1113/platform")
                .join(name);
            fs::create_dir_all(&base).unwrap();
            Self(base)
        }
        fn root(&self) -> RootHandle {
            RootHandle::new(File::open(&self.0).unwrap()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn private_entry_numeric_boundaries_have_no_os_side_effects() {
        for parent in [0, 1, u32::MAX] {
            assert!(RestrictedEntry::new(parent, 16 << 20).is_err());
        }
        for parent in [2, i32::MAX as u32] {
            for bytes in [0, 1, (16 << 20) - 1, (1 << 30) + 1, u64::MAX] {
                assert!(RestrictedEntry::new(parent, bytes).is_err());
            }
            for bytes in [16 << 20, 1 << 30] {
                assert!(RestrictedEntry::new(parent, bytes).is_ok());
            }
        }
    }

    #[test]
    fn snapshots_preserve_empty_one_byte_invalid_utf8_and_every_truncation() {
        let fixture = Fixture::new("truncation");
        let root = fixture.root();
        let path = RelativePath::new("file").unwrap();
        let sample = b"a\xff\xc0\x80\0z";
        for end in 0..=sample.len() {
            let bytes = &sample[..end];
            fs::write(fixture.0.join("file"), bytes).unwrap();
            let identity = root.identity(&path).unwrap();
            let digest = Sha256::digest(bytes).unwrap();
            let snapshot = root.snapshot(&path, identity, digest).unwrap();
            assert_eq!(snapshot.digest(), digest);
            for offset in 0..=end {
                let first = snapshot.read(offset as u64, end - offset).unwrap();
                assert_eq!(first, bytes[offset..]);
                assert_eq!(first, snapshot.read(offset as u64, end - offset).unwrap());
            }
            assert!(snapshot.read(0, end + 1).is_err());
            assert!(snapshot.read(u64::MAX, 1).is_err());
            assert!(snapshot.read(end as u64 + 1, 0).is_err());
            assert!(
                root.snapshot(&path, identity, ContentDigest::sha256([0; 32], end as u64))
                    .is_err()
            );
            assert!(root.replace(&path, identity, b"hostile").is_err());
            assert_eq!(fs::read(fixture.0.join("file")).unwrap(), bytes);
        }
    }

    #[test]
    fn snapshot_copy_and_read_caps_are_exact_and_reads_can_share_a_snapshot() {
        let fixture = Fixture::new("limits");
        let bytes = vec![0; MAX_SNAPSHOT_BYTES as usize];
        let file = fixture.0.join("file");
        fs::write(&file, &bytes).unwrap();
        let root = fixture.root();
        let path = RelativePath::new("file").unwrap();
        let digest = Sha256::digest(&bytes).unwrap();
        let snapshot = root
            .snapshot(&path, root.identity(&path).unwrap(), digest)
            .unwrap();
        assert_eq!(snapshot.read(0, 1 << 20).unwrap(), bytes[..1 << 20]);
        assert!(snapshot.read(0, (1 << 20) + 1).is_err());
        assert!(snapshot.read(0, usize::MAX).is_err());
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let snapshot = &snapshot;
                    let fixture = &fixture;
                    scope.spawn(move || {
                        let offset = i * 257;
                        assert_eq!(snapshot.read(offset, 257).unwrap(), vec![0; 257]);
                        assert_eq!(
                            root_identity_roundtrip(fixture),
                            root_identity_roundtrip(fixture)
                        );
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
        });
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_len(MAX_SNAPSHOT_BYTES + 1)
            .unwrap();
        let identity = root.identity(&path).unwrap();
        assert!(matches!(
            root.snapshot(
                &path,
                identity,
                ContentDigest::sha256([0; 32], MAX_SNAPSHOT_BYTES + 1)
            ),
            Err(PlatformError::ResourceLimit)
        ));
    }

    fn root_identity_roundtrip(fixture: &Fixture) -> dctx_plugin_platform::linux::ObjectIdentity {
        fixture
            .root()
            .identity(&RelativePath::new("file").unwrap())
            .unwrap()
    }

    #[test]
    fn roots_reject_wrong_objects_symlinks_duplicates_and_missing_names() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new("objects");
        fs::write(fixture.0.join("file"), b"x").unwrap();
        assert!(RootHandle::new(File::open(fixture.0.join("file")).unwrap()).is_err());
        let root = fixture.root();
        let path = RelativePath::new("file").unwrap();
        let identity = root.identity(&path).unwrap();
        for name in ["missing", "directory"] {
            if name == "directory" {
                fs::create_dir(fixture.0.join(name)).unwrap();
            }
            assert!(root.identity(&RelativePath::new(name).unwrap()).is_err());
        }
        symlink("file", fixture.0.join("link")).unwrap();
        assert!(root.identity(&RelativePath::new("link").unwrap()).is_err());
        fs::hard_link(fixture.0.join("file"), fixture.0.join("alias")).unwrap();
        for name in ["file", "alias"] {
            assert!(root.identity(&RelativePath::new(name).unwrap()).is_err());
        }
        assert!(
            root.snapshot(&path, identity, Sha256::digest(b"x").unwrap())
                .is_err()
        );
    }

    #[test]
    fn restricted_entry_and_clean_environment_are_exercised_in_a_disposable_process() {
        let fixture = Fixture::new("probe");
        let input_path = fixture.0.join("input");
        fs::write(&input_path, b"private-fixture").unwrap();
        for mode in ["environment", "compute", "landlock"] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_dctx-platform-probe"));
            NoSecretEnvironment.apply(&mut command);
            let mut child = command
                .arg(mode)
                .arg(&input_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            if let Some(mut input) = child.stdin.take() {
                use std::io::Write as _;
                let _ = input.write_all(b"ping");
            }
            let output = child.wait_with_output().unwrap();
            if mode == "environment" {
                assert!(output.status.success());
                assert_eq!(output.stdout, b"clean\n");
            } else if output.status.code() == Some(125) {
                let error = String::from_utf8(output.stderr).unwrap();
                assert!(error.contains("Landlock") || error.contains("Seccomp"));
                assert!(output.stdout.is_empty());
            } else {
                assert_eq!(
                    output.stdout,
                    if mode == "landlock" {
                        b"landlock-denied\n".as_slice()
                    } else {
                        b"pong\n".as_slice()
                    }
                );
                assert!(output.status.success());
            }
        }
    }
}

#[test]
fn every_platform_failure_names_a_next_step_and_keeps_its_wire_category() {
    use dctx_plugin_protocol::ErrorCode;
    for (error, code) in [
        (PlatformError::InvalidPath, ErrorCode::InvalidMessage),
        (PlatformError::Conflict, ErrorCode::Conflict),
        (PlatformError::ResourceLimit, ErrorCode::ResourceLimit),
        (PlatformError::Unavailable, ErrorCode::Denied),
        (PlatformError::Io, ErrorCode::Denied),
    ] {
        assert_eq!(error.code(), code);
        assert!(error.to_string().contains("; next: "));
    }
    for unsupported in [
        Unsupported::Target,
        Unsupported::OneShotExec,
        Unsupported::BundledLaunch,
        Unsupported::ProgramRecipe,
        Unsupported::ConditionalReplace,
        Unsupported::PathResolution,
        Unsupported::Landlock,
        Unsupported::Seccomp,
        Unsupported::Bootstrap,
        Unsupported::Lifecycle,
        Unsupported::Seatbelt,
    ] {
        let error = PlatformError::Unsupported(unsupported);
        assert_eq!(error.code(), ErrorCode::UnsupportedPlatform);
        assert!(error.to_string().contains("; next: "));
    }
}

#[cfg(unix)]
#[test]
fn invalid_utf8_configured_environment_is_removed_without_reading_or_spawning() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let mut command = Command::new("unused");
    command.env(
        OsString::from_vec(vec![0xff]),
        OsString::from_vec(vec![0xfe]),
    );
    NoSecretEnvironment.apply(&mut command);
    assert_eq!(command.get_envs().count(), 2);
    assert!(
        command
            .get_envs()
            .all(|(k, _)| k == "LANG" || k == "LC_ALL")
    );
}
