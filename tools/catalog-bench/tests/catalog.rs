// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: 2026 Eris Margeta
//! Format, corruption and filesystem-boundary regressions; scratch stays in this project.

use catalog_bench::{Catalog, Entry, Metadata, Tier, walk, write_catalog};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scratch")
            .join(format!(
                "test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn entry(path: &[u8], inode: u64) -> Entry {
    Entry {
        path: path.to_vec(),
        metadata: Metadata {
            size: u64::MAX,
            mtime_seconds: -42,
            mtime_nanos: 999_999_999,
            inode,
            device: 2049,
            tier: Tier::Code,
        },
    }
}

#[test]
fn round_trip_metadata_and_queries() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    let mut entries = vec![
        entry(b"src/test_parser.rs", 1),
        entry(b"README.md", u64::MAX),
        entry(b"src/Parser-test.rs", 3),
    ];
    entries[1].metadata.tier = Tier::Markdown;
    let mut dirs = vec![(b"src".to_vec(), false), (vec![], true)];
    write_catalog(&output, &mut entries, &mut dirs).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    assert_eq!(catalog.len(), 3);
    assert_eq!(catalog.git_roots(), 1);
    assert_eq!(catalog.shards(), 1);
    for (id, expected) in entries.iter().enumerate() {
        assert_eq!(&catalog.entry(id).unwrap(), expected);
    }
    assert_eq!(catalog.find(b"PARSER"), vec![1, 2]);
    assert_eq!(catalog.find(b"rs"), vec![1, 2]);
    assert_eq!(catalog.find(b"parse"), Vec::<usize>::new());
    assert_eq!(catalog.find(b""), Vec::<usize>::new());
    assert_eq!(catalog.find(b"test-parser"), Vec::<usize>::new());
    assert!(catalog.entry(3).is_err());
    assert!(write_catalog(&output, &mut entries, &mut dirs).is_err());
}

#[test]
fn parent_front_coding_and_nested_shards() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    let prefix = b"a_very_long_parent_directory_name_that_must_only_be_stored_once";
    let mut dirs = vec![
        (vec![], false),
        (prefix.to_vec(), false),
        ([prefix.as_slice(), b"nested"].join(&b'/'), true),
    ];
    let mut entries: Vec<_> = (0..100)
        .map(|i| {
            entry(
                &[prefix.as_slice(), format!("file{i}.rs").as_bytes()].join(&b'/'),
                i,
            )
        })
        .collect();
    entries.push(entry(
        &[prefix.as_slice(), b"nested/inner.rs"].join(&b'/'),
        100,
    ));
    entries.push(entry(b"root.txt", 101));
    write_catalog(&output, &mut entries, &mut dirs).unwrap();
    let raw = fs::read(&output).unwrap();
    assert_eq!(
        raw.windows(prefix.len())
            .filter(|bytes| *bytes == prefix)
            .count(),
        1
    );
    let catalog = Catalog::open(&output).unwrap();
    assert_eq!(catalog.git_roots(), 1);
    assert_eq!(catalog.shards(), 3); // root fallback, top-level fallback, nested git root
    for (id, expected) in entries.iter().enumerate() {
        assert_eq!(&catalog.entry(id).unwrap(), expected);
    }
}

#[test]
fn unicode_and_non_utf8_paths() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    let mut dirs = vec![
        (vec![], true),
        ("日本語🦀".as_bytes().to_vec(), false),
        (b"raw\xff".to_vec(), false),
    ];
    let mut entries = vec![
        entry("日本語🦀/café.md".as_bytes(), 1),
        entry(b"raw\xff/name\xfe.rs", 2),
    ];
    write_catalog(&output, &mut entries, &mut dirs).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    for (id, expected) in entries.iter().enumerate() {
        assert_eq!(&catalog.entry(id).unwrap(), expected);
    }
    assert_eq!(catalog.find("café".as_bytes()).len(), 1);
    assert_eq!(catalog.find(b"name\xfe").len(), 1);
}

#[test]
fn rejects_truncation_and_corrupt_sections_records() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    write_catalog(&output, &mut [entry(b"a.rs", 5)], &mut [(vec![], false)]).unwrap();
    let raw = fs::read(&output).unwrap();
    let broken = scratch.0.join("broken");
    for len in 0..raw.len() {
        fs::write(&broken, &raw[..len]).unwrap();
        assert!(Catalog::open(&broken).is_err(), "length {len}");
    }
    for at in [0, 8, 16, 24, 32, 40, 48, 56, 64, 65, 66, raw.len() - 1] {
        let mut copy = raw.clone();
        copy[at] = 255;
        fs::write(&broken, copy).unwrap();
        assert!(Catalog::open(&broken).is_err(), "offset {at}");
    }
}

#[test]
fn empty_catalog_and_reject_invalid_input() {
    let scratch = Scratch::new();
    let output = scratch.0.join("empty");
    write_catalog(&output, &mut [], &mut [(vec![], false)]).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    assert!(catalog.is_empty());
    assert_eq!(catalog.shards(), 0);
    assert_eq!(catalog.find(b"rs"), Vec::<usize>::new());
    assert!(write_catalog(&scratch.0.join("no-root"), &mut [], &mut []).is_err());
    assert!(
        write_catalog(
            &scratch.0.join("traversal"),
            &mut [entry(b"../escape.rs", 0)],
            &mut [(vec![], false)]
        )
        .is_err()
    );
}

#[test]
fn walker_ignores_defaults_preserves_bytes_and_never_follows_links() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let root = scratch.0.join("root");
    fs::create_dir_all(root.join("nested/.git")).unwrap();
    fs::create_dir_all(scratch.0.join("outside")).unwrap();
    fs::write(scratch.0.join("outside/secret.rs"), "do not catalog").unwrap();
    fs::write(root.join(".git"), "gitdir: not-followed").unwrap();
    fs::write(root.join(".gitignore"), "ignored.rs\n").unwrap();
    fs::write(root.join("nested/.gitignore"), "nested-ignored.rs\n").unwrap();
    fs::write(root.join("nested/nested-ignored.rs"), "ignored").unwrap();
    fs::write(root.join("ignored.rs"), "ignored").unwrap();
    fs::write(root.join("keep.rs"), "fn test() {}").unwrap();
    fs::write(root.join("readme.md"), "# hello").unwrap();
    fs::write(root.join("image.png"), [0, 1, 2]).unwrap();
    fs::write(root.join("unknown"), "no sniff").unwrap();
    let raw = std::ffi::OsStr::from_bytes(b"bytes\xff.rs");
    fs::write(root.join(raw), "raw").unwrap();
    for skip in ["target", "node_modules", ".git"] {
        fs::create_dir_all(root.join("nested").join(skip)).unwrap();
        fs::write(root.join("nested").join(skip).join("skip.rs"), "skip").unwrap();
    }
    symlink(scratch.0.join("outside"), root.join("external")).unwrap();
    symlink(
        scratch.0.join("outside/secret.rs"),
        root.join("external-file.rs"),
    )
    .unwrap();
    symlink(&root, root.join("loop")).unwrap();
    let snapshot = walk(&root, 4, None).unwrap();
    assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    assert_eq!(snapshot.symlinks_skipped, 3);
    assert_eq!(snapshot.directories.iter().filter(|d| d.1).count(), 2);
    let paths: Vec<_> = snapshot
        .entries
        .iter()
        .map(|entry| entry.path.as_slice())
        .collect();
    for expected in [
        b"keep.rs".as_slice(),
        b".gitignore",
        b"nested/.gitignore",
        b"bytes\xff.rs",
    ] {
        assert!(paths.contains(&expected));
    }
    assert_eq!(paths.len(), 7);
    for (path, tier) in [
        (b"keep.rs".as_slice(), Tier::Code),
        (b"readme.md", Tier::Markdown),
        (b"image.png", Tier::Binary),
        (b"unknown", Tier::Other),
    ] {
        assert_eq!(
            snapshot
                .entries
                .iter()
                .find(|e| e.path == path)
                .unwrap()
                .metadata
                .tier,
            tier
        );
    }
    assert!(walk(&root.join("external"), 4, None).is_err());
    assert!(walk(&root, 0, None).is_err());
}

#[test]
fn refuses_implicit_ignore_symlink_escape() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let root = scratch.0.join("root");
    fs::create_dir_all(root.join("unsafe")).unwrap();
    fs::write(scratch.0.join("outside"), "*.rs\n").unwrap();
    fs::write(root.join("keep.rs"), "included").unwrap();
    symlink(scratch.0.join("outside"), root.join("unsafe/.gitignore")).unwrap();
    let snapshot = walk(&root, 2, None).unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.errors.len(), 1);
    symlink(scratch.0.join("outside"), root.join(".gitignore")).unwrap();
    assert!(walk(&root, 2, None).is_err());
}

#[test]
fn unreadable_content_is_not_needed_and_cli_returns_raw_paths() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let root = scratch.0.join("root");
    fs::create_dir_all(&root).unwrap();
    let filename = std::ffi::OsStr::from_bytes(b"file\xff.rs");
    let path = root.join(filename);
    fs::write(&path, "content must never be read").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let mut snapshot = walk(&root, 2, None).unwrap();
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.errors, Vec::<String>::new());
    assert_eq!(snapshot.entries[0].metadata.size, 26);
    let output = scratch.0.join("catalog");
    write_catalog(&output, &mut snapshot.entries, &mut snapshot.directories).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_catalog-bench"))
        .arg("query")
        .arg(&output)
        .arg("rs")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout, b"file\xff.rs\0");
}

#[test]
fn signed_mtime_extremes_round_trip() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    let mut entries = vec![entry(b"a", 0), entry(b"b", 0)];
    entries[0].metadata.mtime_seconds = i64::MIN;
    entries[1].metadata.mtime_seconds = i64::MAX;
    write_catalog(&output, &mut entries, &mut [(vec![], false)]).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    assert_eq!(catalog.entry(0).unwrap(), entries[0]);
    assert_eq!(catalog.entry(1).unwrap(), entries[1]);
}

#[test]
fn index_matches_scan_oracle_and_pages_deduplicate() {
    use catalog_bench::QueryMode;
    let scratch = Scratch::new();
    let output = scratch.0.join("indexed");
    let mut entries: Vec<_> = (0..250)
        .map(|id| entry(format!("Parser{id:03}_parser_test_TEST.rs").as_bytes(), id))
        .collect();
    entries.push(entry(b"\xffABC\xfe_aaaaaaa.a", 300));
    entries.push(entry(b"!!!", 301));
    write_catalog(&output, &mut entries, &mut [(vec![], false)]).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    assert!(catalog.name_index_size() > 0);
    for query in [
        b"parser".as_slice(),
        b"PARS",
        b"ar",
        b"a",
        b"rs",
        b"ser12",
        b"aaa",
        b"\xffabc",
        b"missing",
        b"",
        b"test-rs",
    ] {
        for mode in [QueryMode::Exact, QueryMode::Prefix, QueryMode::Substring] {
            let mut expected = Vec::new();
            if !query.is_empty()
                && query
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b >= 0x80)
            {
                for (id, entry) in entries.iter().enumerate() {
                    let q = query.to_ascii_lowercase();
                    if entry
                        .path
                        .split(|b| !(b.is_ascii_alphanumeric() || *b >= 0x80))
                        .any(|t| {
                            let t = t.to_ascii_lowercase();
                            match mode {
                                QueryMode::Exact => t == q,
                                QueryMode::Prefix => t.starts_with(&q),
                                QueryMode::Substring => t.windows(q.len()).any(|w| w == q),
                            }
                        })
                    {
                        expected.push(id);
                    }
                }
            }
            let full = catalog.search(query, mode, usize::MAX);
            let mut sorted = full.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, expected, "{query:?} {mode:?}");
            for limit in [0, 1, 3, 100, 500] {
                let page = catalog.search(query, mode, limit);
                assert_eq!(page, full[..full.len().min(limit)]);
            }
        }
    }
}

#[test]
fn index_rejects_corruption_without_panicking() {
    let scratch = Scratch::new();
    let output = scratch.0.join("catalog");
    write_catalog(
        &output,
        &mut [entry(b"abc_ABC.rs", 1), entry(b"abcd.rs", 2)],
        &mut [(vec![], false)],
    )
    .unwrap();
    let raw = fs::read(&output).unwrap();
    let start = usize::try_from(u64::from_le_bytes(raw[56..64].try_into().unwrap())).unwrap();
    let broken = scratch.0.join("broken");
    // Every index byte sees arbitrary mutations. Some changes remain valid;
    // all must be handled without an out-of-bounds access or a panic.
    for at in start..raw.len() {
        for value in [0, 127, 128, 255] {
            let mut copy = raw.clone();
            copy[at] = value;
            fs::write(&broken, copy).unwrap();
            let _ = Catalog::open(&broken);
        }
    }
    let mut copy = raw.clone();
    copy[start..start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    fs::write(&broken, copy).unwrap();
    assert!(Catalog::open(&broken).is_err());
    // Bad token offset, token case, posting file ID, gram key and gram token ID.
    let word = |at| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap()) as usize;
    let dictionary = start + word(start + 8);
    let gram_directory = start + word(start + 12);
    let gram_data = start + word(start + 16);
    for at in [
        start + 24,
        dictionary + 1,
        dictionary + 5,
        gram_directory,
        gram_data,
    ] {
        let mut copy = raw.clone();
        copy[at] = 255;
        fs::write(&broken, copy).unwrap();
        assert!(Catalog::open(&broken).is_err(), "{at}");
    }
}

#[test]
fn parallel_walk_tokens_survive_file_id_merge() {
    use catalog_bench::{QueryMode, write_snapshot};
    let scratch = Scratch::new();
    let root = scratch.0.join("root");
    for dir in 0..20 {
        let path = root.join(format!("d{dir}"));
        fs::create_dir_all(&path).unwrap();
        for file in 0..10 {
            fs::write(path.join(format!("Parser{file}_TEST.rs")), []).unwrap();
        }
    }
    let output = scratch.0.join("catalog");
    let snapshot = walk(&root, 8, None).unwrap();
    let expected = snapshot.entries.clone();
    write_snapshot(&output, snapshot).unwrap();
    let catalog = Catalog::open(&output).unwrap();
    for (id, entry) in expected.iter().enumerate() {
        assert_eq!(&catalog.entry(id).unwrap(), entry);
    }
    assert_eq!(catalog.find(b"test").len(), 200);
    assert_eq!(
        catalog
            .search(b"ser3", QueryMode::Substring, usize::MAX)
            .len(),
        20
    );
    for id in catalog.search(b"parser3", QueryMode::Exact, usize::MAX) {
        assert!(catalog.basename(id).unwrap().starts_with(b"Parser3_"));
    }
}
