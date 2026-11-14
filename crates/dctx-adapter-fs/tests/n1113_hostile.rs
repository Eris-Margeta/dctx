// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for policy, confinement, source sessions and filesystem ports.
//! Byte APIs accept arbitrary binary content; text APIs are exercised at UTF-8 boundaries.
#![forbid(unsafe_code)]

use dctx_adapter_fs::domain::{ItemContext, is_inside, join_lexical, normalize, parse_rule};
use dctx_adapter_fs::{
    DirEntry, EntryMeta, FileHandlingDecision as Decision, FreshnessMode, FsKind, FsSource,
    SelectionPolicy, SourceError, SourceFs, SourceOptions, SpyFs, StdFs, Validation,
};
use std::{
    io,
    path::{Path, PathBuf},
};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(std::fs::canonicalize(std::env::temp_dir()).unwrap()).unwrap()
}

fn decision(pattern: &str, relative: &str) -> Decision {
    SelectionPolicy::from_lines(&[pattern], &[]).decide(&ItemContext {
        relative_path: relative,
        name: relative.rsplit('/').next().unwrap(),
        is_directory: false,
        debug_mode: false,
    })
}

#[test]
fn globstar_requires_a_directory_boundary() {
    for pattern in ["**/secret", "**/s?cret", "src/**/secret"] {
        let prefix = if pattern.starts_with("src/") {
            "src/"
        } else {
            ""
        };
        for tail in ["secret", "nested/secret", "a/b/secret"] {
            assert_eq!(
                decision(pattern, &format!("{prefix}{tail}")),
                Decision::Exclude
            );
        }
        for tail in ["nosecret", "nested/nosecret", "ssecret"] {
            assert_eq!(
                decision(pattern, &format!("{prefix}{tail}")),
                Decision::Include,
                "pattern {pattern}, tail {tail}"
            );
        }
    }
}

#[test]
fn policy_empty_single_byte_prefixes_duplicates_and_order_are_stable() {
    for sample in [
        "",
        "!",
        "*",
        "?",
        "#",
        "!src/**/sécret/ # comment",
        "[literal]",
        "\0",
    ] {
        for cut in 0..=sample.len() {
            if let Some(prefix) = sample.get(..cut) {
                assert_eq!(parse_rule(prefix), parse_rule(prefix));
                let first = decision(prefix, "src/sécret");
                assert_eq!(first, decision(prefix, "src/sécret"));
            }
        }
    }
    let context = ItemContext {
        relative_path: "a",
        name: "a",
        is_directory: false,
        debug_mode: false,
    };
    let policy = SelectionPolicy::from_lines(&["a", "a", "!a"], &[]);
    assert_eq!(policy.decide(&context), Decision::Include);
    assert_eq!(
        SelectionPolicy::from_lines(&["!a", "a"], &[]).decide(&context),
        Decision::Exclude
    );
    assert_eq!(
        policy.decide_with_parent(&context, Decision::Exclude),
        Decision::Exclude
    );
    assert_eq!(
        SelectionPolicy::from_lines(&[], &["a", "a"]).decide(&context),
        Decision::OmitContent
    );
    for n in [4096, 4097] {
        let rule = "x".repeat(n);
        assert_eq!(decision(&rule, &rule), Decision::Exclude);
    }
    for d in [
        Decision::Include,
        Decision::Exclude,
        Decision::OmitContent,
        Decision::DebugVisible,
    ] {
        assert_eq!(d.should_read_content(), d == Decision::Include);
        assert_eq!(d.show_in_tree(), d != Decision::Exclude);
        assert_eq!(d.needs_metadata(), d != Decision::Exclude);
        assert_eq!(d.should_recurse(), d != Decision::Exclude);
    }
}

#[test]
fn binary_content_every_prefix_and_validation_are_repeatable() {
    let dir = temp();
    let path = dir.path().join("a");
    let sample = b"a\0\xff\xc3\xa9\n";
    for cut in 0..=sample.len() {
        std::fs::write(&path, &sample[..cut]).unwrap();
        let source = FsSource::open(dir.path(), SourceOptions::default()).unwrap();
        let first = source.read("a").unwrap();
        assert_eq!(first.bytes, sample[..cut]);
        assert_eq!(source.read("a").unwrap(), first);
        assert_eq!(source.inventory().unwrap(), source.inventory().unwrap());
        for mode in [FreshnessMode::Strict, FreshnessMode::Metadata] {
            let result = source.validate("a", &first.identity(), mode).unwrap();
            assert!(matches!(
                result,
                Validation::DigestMatch { .. } | Validation::MetadataMatch { .. }
            ));
            assert_eq!(
                result,
                source.validate("a", &first.identity(), mode).unwrap()
            );
        }
        assert!(matches!(
            source
                .validate("missing", &first.identity(), FreshnessMode::Strict)
                .unwrap(),
            Validation::Stale { .. }
        ));
        std::thread::scope(|scope| {
            let joins: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| source.read("a").unwrap()))
                .collect();
            for join in joins {
                assert_eq!(join.join().unwrap(), first);
            }
        });
    }
}

#[test]
fn bounded_reads_cover_zero_one_max_and_over_max_for_content_and_policy() {
    let dir = temp();
    let path = dir.path().join("a");
    for n in [0, 1, 16, 17] {
        let bytes = vec![0xff; n];
        std::fs::write(&path, &bytes).unwrap();
        let result = StdFs.read_file_limited(&path, 16);
        if n <= 16 {
            assert_eq!(result.unwrap(), bytes);
        } else {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
        }
    }
    for (cap, policy) in [(1024 * 1024, true), (8 * 1024 * 1024, false)] {
        for n in [0, 1, cap, cap + 1] {
            let bytes = vec![b'#'; n];
            std::fs::write(&path, &bytes).unwrap();
            if policy {
                let result = StdFs.read_policy(&path);
                if n <= cap {
                    assert_eq!(result.unwrap().unwrap().as_bytes(), bytes);
                } else {
                    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
                }
            } else {
                let result = StdFs.read_file(&path);
                if n <= cap {
                    assert_eq!(result.unwrap(), bytes);
                } else {
                    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::FileTooLarge);
                }
            }
        }
    }
    for bytes in [&b"\xff"[..], &b"ok\n\xc3"[..]] {
        std::fs::write(dir.path().join(".dctxignore"), bytes).unwrap();
        assert!(matches!(
            FsSource::open(dir.path(), SourceOptions::default()),
            Err(SourceError::Rule { .. })
        ));
    }
}

#[test]
fn relative_path_refusals_happen_before_content_io() {
    let dir = temp();
    std::fs::write(dir.path().join("a"), b"x").unwrap();
    let source = FsSource::load(
        dir.path().to_path_buf(),
        SourceOptions::default(),
        SpyFs::new(StdFs),
    )
    .unwrap();
    let identity = source.read("a").unwrap().identity();
    source.filesystem().clear();
    let long = "a".repeat(4097);
    for path in [
        "",
        ".",
        "..",
        "/a",
        "a/",
        "a//b",
        "a/../b",
        "a\\b",
        "a:b",
        "a\0b",
        "a.",
        "a ",
        long.as_str(),
    ] {
        assert!(
            matches!(source.read(path), Err(SourceError::InvalidPath { .. })),
            "{path:?}"
        );
        assert!(matches!(
            source.validate(path, &identity, FreshnessMode::Strict),
            Err(SourceError::InvalidPath { .. })
        ));
    }
    assert!(source.filesystem().read_files().is_empty());
    assert!(source.filesystem().operations().is_empty());
}

// In-memory metadata avoids OS path ceilings and clock-dependent stamps.
#[derive(Clone)]
struct ListingFs {
    root: PathBuf,
    names: Vec<std::ffi::OsString>,
    depth: usize,
}
impl SourceFs for ListingFs {
    fn metadata(&self, path: &Path) -> io::Result<EntryMeta> {
        Ok(EntryMeta {
            kind: if path == self.root {
                FsKind::Directory
            } else {
                FsKind::File
            },
            size: 1,
            modified_ns: 0,
            inode: Some(1),
        })
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<DirEntry>> {
        let level = path.strip_prefix(&self.root).unwrap().components().count();
        if self.depth > 0 {
            return Ok(vec![DirEntry {
                name: "a".repeat(255).into(),
                kind: if level < self.depth {
                    FsKind::Directory
                } else {
                    FsKind::File
                },
            }]);
        }
        Ok(self
            .names
            .iter()
            .map(|name| DirEntry {
                name: name.clone(),
                kind: FsKind::File,
            })
            .collect())
    }
    fn read_link(&self, _: &Path) -> io::Result<PathBuf> {
        Err(io::ErrorKind::NotFound.into())
    }
    fn read_file(&self, _: &Path) -> io::Result<Vec<u8>> {
        Ok(b"x".to_vec())
    }
    fn read_policy(&self, _: &Path) -> io::Result<Option<String>> {
        Ok(None)
    }
}
fn listing(names: &[&str], depth: usize) -> FsSource<ListingFs> {
    let root = std::path::absolute("fixture").unwrap();
    FsSource::load(
        root.clone(),
        SourceOptions::default(),
        ListingFs {
            root,
            names: names.iter().map(Into::into).collect(),
            depth,
        },
    )
    .unwrap()
}

#[test]
fn inventory_order_and_duplicate_entries_are_consistent() {
    let a = listing(&["b", "a"], 0).inventory().unwrap();
    let b = listing(&["a", "b"], 0).inventory().unwrap();
    assert_eq!(a, b);
    let duplicate = listing(&["a", "a"], 0).inventory().unwrap();
    // A port may repeat entries; repeated calls must still produce identical records.
    assert_eq!(duplicate, listing(&["a", "a"], 0).inventory().unwrap());
}

#[test]
fn inventory_refuses_paths_that_read_cannot_accept() {
    assert!(listing(&[], 15).inventory().is_ok()); // 16 * 255 + 15 separators = 4095.
    assert!(matches!(
        listing(&[], 16).inventory(),
        Err(SourceError::InvalidPath { .. })
    ));
    let root = std::path::absolute("fixture").unwrap();
    for path in ["", "a", "a/../b", "../escape", "a/../../escape"] {
        let normalized = normalize(Path::new(path));
        assert_eq!(normalize(&normalized), normalized);
        let joined = join_lexical(&root, Path::new(path));
        assert_eq!(is_inside(&root, &joined), !path.contains("escape"));
    }
}

#[cfg(unix)]
#[test]
fn invalid_utf8_directory_names_fail_closed() {
    use std::os::unix::ffi::OsStringExt;
    let mut source = listing(&[], 0);
    source = FsSource::load(
        source.filesystem().root.clone(),
        SourceOptions::default(),
        ListingFs {
            names: vec![std::ffi::OsString::from_vec(vec![0xff])],
            ..source.filesystem().clone()
        },
    )
    .unwrap();
    assert!(matches!(
        source.inventory(),
        Err(SourceError::InvalidPath { .. })
    ));
}

#[test]
fn wildcard_heavy_nonmatches_finish_without_an_exponential_search() {
    let pattern = format!("{}z", "*a".repeat(64));
    assert_eq!(decision(&pattern, &"a".repeat(96)), Decision::Include);
    assert_eq!(
        decision(&pattern, &format!("{}z", "a".repeat(96))),
        Decision::Exclude
    );
}

#[test]
fn relative_normalization_preserves_unresolved_parents() {
    for (input, expected) in [
        ("..", ".."),
        ("../a", "../a"),
        ("../../a", "../../a"),
        ("a/../../b", "../b"),
        ("a/../b", "b"),
    ] {
        assert_eq!(normalize(Path::new(input)), Path::new(expected));
    }
}

#[test]
fn stamps_cover_scalar_extremes_and_optional_identity() {
    use dctx_adapter_fs::FileStamp;
    for size in [0, 1, u64::MAX] {
        for modified in [0, 1, u128::MAX] {
            let first = FileStamp::new(size, modified, Some(u64::MAX));
            assert!(first.metadata_matches(&first));
            assert!(first.metadata_matches(&FileStamp::new(size, modified, None)));
            assert!(!first.metadata_matches(&FileStamp::new(size, modified, Some(0))));
            assert!(!first.metadata_matches(&FileStamp::new(size ^ 1, modified, Some(u64::MAX))));
        }
    }
}

#[test]
fn direct_path_limit_is_in_utf8_bytes_and_inventory_uses_the_same_bound() {
    for name in ["a".repeat(4096), "é".repeat(2048)] {
        let source = listing(&[&name], 0);
        assert_eq!(source.read(&name).unwrap().bytes, b"x");
        assert_eq!(source.inventory().unwrap()[0].relative, name);
    }
    for name in ["a".repeat(4097), "é".repeat(2049)] {
        let source = listing(&[&name], 0);
        assert!(matches!(
            source.read(&name),
            Err(SourceError::InvalidPath { .. })
        ));
        assert!(matches!(
            source.inventory(),
            Err(SourceError::InvalidPath { .. })
        ));
    }
}

#[test]
fn question_mark_before_a_trailing_star_is_still_a_wildcard() {
    for path in ["src/a/file", "src/z/file"] {
        assert_eq!(decision("src/?/*", path), Decision::Exclude);
    }
    for path in ["src/ab/file", "src//file"] {
        assert_eq!(decision("src/?/*", path), Decision::Include);
    }
}

#[cfg(unix)]
#[test]
fn symlink_ports_inventory_and_lookup_never_read_an_escape() {
    let dir = temp();
    let root = dir.path().join("source");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(dir.path().join("outside"), b"secret").unwrap();
    std::os::unix::fs::symlink("../outside", root.join("link")).unwrap();
    assert_eq!(
        StdFs.read_link(&root.join("link")).unwrap(),
        Path::new("../outside")
    );
    assert_eq!(
        StdFs.metadata(&root.join("link")).unwrap().kind,
        FsKind::Symlink
    );
    assert!(StdFs.read_file(&root.join("link")).is_err());
    assert!(StdFs.read_policy(&root.join("link")).is_err());
    let source = FsSource::load(root, SourceOptions::default(), SpyFs::new(StdFs)).unwrap();
    assert!(matches!(
        source.inventory(),
        Err(SourceError::Escape { .. })
    ));
    assert!(matches!(
        source.read("link"),
        Err(SourceError::Escape { .. })
    ));
    assert!(source.filesystem().read_files().is_empty());
}

#[test]
fn every_source_error_names_a_next_step() {
    let errors = [
        SourceError::Io {
            path: "a".into(),
            source: io::ErrorKind::PermissionDenied.into(),
        },
        SourceError::Escape {
            path: "a".into(),
            target: "../b".into(),
        },
        SourceError::Rule {
            path: "a".into(),
            source: io::ErrorKind::InvalidData.into(),
        },
        SourceError::Denied {
            relative: "a".into(),
        },
        SourceError::InvalidPath { path: "a".into() },
        SourceError::Stale {
            path: "a".into(),
            reason: dctx_adapter_fs::StaleReason::Race,
        },
        SourceError::Digest(dctx_hash::HashError::MessageTooLong),
    ];
    for error in errors {
        let message = error.to_string();
        assert!(
            !message
                .split_once(';')
                .expect("an actionable error")
                .1
                .trim()
                .is_empty()
        );
    }
}
