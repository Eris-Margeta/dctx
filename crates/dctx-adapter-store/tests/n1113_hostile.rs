// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile publication, pointer/manifest recovery, pin, and port tests.
//! Private codecs are driven through publication and checksum-valid recovery fixtures.
#![forbid(unsafe_code)]

mod common;
use common::{open, req, segment};
use dctx_adapter_store::ports::StoreFs;
use dctx_adapter_store::{
    Boundary, DurabilityMode, GcBudget, MarkMode, MemoryFs, Revisions, SegmentBlob, SegmentRole,
    SegmentStore, StdFs, StoreError, StoreOptions,
};
use dctx_hash::Sha256;

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn files(fs: &MemoryFs) -> Vec<(String, Vec<u8>)> {
    fs.list_prefix("")
        .unwrap()
        .into_iter()
        .map(|path| {
            let bytes = fs.read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}
fn resign_manifest(bytes: &mut [u8]) {
    let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    for i in 0..usize::try_from(count).unwrap() {
        let at = 80 + i * 64;
        let start = usize::try_from(u64::from_le_bytes(
            bytes[at + 8..at + 16].try_into().unwrap(),
        ))
        .unwrap();
        let len = usize::try_from(u64::from_le_bytes(
            bytes[at + 16..at + 24].try_into().unwrap(),
        ))
        .unwrap();
        let sum = Sha256::digest(&bytes[start..start + len]).unwrap();
        bytes[at + 32..at + 64].copy_from_slice(sum.as_bytes());
    }
    let sum = dctx_map_segment::envelope_checksum(bytes).unwrap();
    bytes[48..80].copy_from_slice(&sum);
}
fn install_manifest(fs: &MemoryFs, bytes: &[u8], generation: u64) {
    let mut framed = b"DCTX\0".to_vec();
    framed.extend_from_slice(&10u16.to_le_bytes());
    framed.extend_from_slice(&1u16.to_le_bytes());
    framed.extend_from_slice(&u64::try_from(bytes.len()).unwrap().to_le_bytes());
    framed.extend_from_slice(bytes);
    let digest = Sha256::digest(&framed).unwrap();
    let path = format!("manifests/{}.man", hex(digest.as_bytes()));
    if !fs.exists(&path).unwrap() {
        fs.create(&path).unwrap();
    }
    fs.write(&path, bytes).unwrap();
    let mut pointer = fs.read("roots/main/current").unwrap();
    pointer[12..20].copy_from_slice(&generation.to_le_bytes());
    pointer[20..52].copy_from_slice(digest.as_bytes());
    pointer[52..60].copy_from_slice(&u64::try_from(bytes.len()).unwrap().to_le_bytes());
    let sum = Sha256::digest(&pointer[..60]).unwrap();
    pointer[60..92].copy_from_slice(sum.as_bytes());
    fs.write("roots/main/current", &pointer).unwrap();
}

#[test]
fn every_segment_prefix_empty_one_byte_and_invalid_utf8_are_refused_atomically() {
    let valid = segment("small");
    for cut in 0..valid.len() {
        let fs = MemoryFs::posix();
        let store = open(fs.clone());
        assert!(
            store
                .publish(req("main", 1, valid[..cut].to_vec()))
                .is_err(),
            "cut {cut}"
        );
        assert_eq!(store.committed("main").unwrap(), None);
        assert!(fs.list_prefix("tmp/").unwrap().is_empty());
    }
    for bytes in [vec![], vec![0], vec![0xff], vec![0xff; 80]] {
        let store = open(MemoryFs::posix());
        assert!(store.publish(req("main", 1, bytes)).is_err());
    }
}

#[test]
fn duplicate_reordered_segments_publish_byte_identical_objects() {
    let a = segment("a");
    let b = segment("b");
    let build = |blobs: Vec<Vec<u8>>| {
        let fs = MemoryFs::posix();
        let store = open(fs.clone());
        let mut request = req("main", 1, a.clone());
        request.segments = blobs
            .into_iter()
            .map(|bytes| SegmentBlob {
                role: SegmentRole::Fact,
                bytes,
            })
            .collect();
        let published = store.publish(request).unwrap();
        (published, files(&fs))
    };
    assert_eq!(
        build(vec![a.clone(), b.clone()]),
        build(vec![b.clone(), a.clone(), a.clone(), b])
    );
    let store = open(MemoryFs::posix());
    let mut request = req("main", 1, a);
    request.segments.clear();
    store.publish(request).unwrap();
    assert!(store.pin("main").unwrap().segments().is_empty());
}

#[test]
fn root_names_reject_dot_components_and_cover_limits_on_every_root_api() {
    for root in ["", ".", "..", ".git", "a/b", "a\\b", "a:b", "a\0b", "é"] {
        let store = open(MemoryFs::posix());
        assert!(
            store.publish(req(root, 1, segment("a"))).is_err(),
            "publish {root:?}"
        );
        assert!(store.committed(root).is_err(), "committed {root:?}");
        assert!(store.current_pointer_closed(root).is_err());
        assert!(store.raw_current_generation(root).is_err());
        assert!(store.pin(root).is_err());
        assert!(store.drop_previous(root).is_err());
        assert!(store.retire_root(root).is_err());
        assert!(store.publish_unvalidated_pointer(root, 1).is_err());
    }
    for len in [1, 64, 65] {
        let store = open(MemoryFs::posix());
        assert_eq!(
            store
                .publish(req(&"a".repeat(len), 1, segment("a")))
                .is_ok(),
            len <= 64
        );
    }
}

#[test]
fn store_options_and_revision_text_cover_empty_max_overmax_and_nul() {
    for cap in [0, 1, 4096, u64::MAX] {
        assert_eq!(
            SegmentStore::open(MemoryFs::posix(), StoreOptions::capped(cap)).is_ok(),
            cap != 0
        );
    }
    for namespace in [
        String::new(),
        "x".into(),
        "x".repeat(64),
        "x".repeat(65),
        "a\0b".into(),
        "é".into(),
    ] {
        let valid = !namespace.is_empty()
            && namespace.len() <= 64
            && namespace.bytes().all(|b| b.is_ascii_alphanumeric());
        let options = StoreOptions {
            namespace,
            ..StoreOptions::capped(1_000_000)
        };
        assert_eq!(
            SegmentStore::open(MemoryFs::posix(), options).is_ok(),
            valid
        );
    }
    for text in [
        String::new(),
        "x".into(),
        "é".repeat(2048),
        "x".repeat(4097),
        "é".repeat(2049),
        "a\0b".into(),
    ] {
        for field in 0..3 {
            let store = open(MemoryFs::posix());
            let mut request = req("main", 1, segment("a"));
            let valid = text.len() <= 4096 && !text.contains('\0');
            match field {
                0 => request.revisions.resolver = text.clone(),
                1 => request.revisions.extraction = text.clone(),
                _ => request.revisions.parser = text.clone(),
            }
            assert_eq!(store.publish(request).is_ok(), valid);
        }
    }
}

#[test]
fn pointer_every_prefix_and_overmax_recovery_are_stable() {
    for size in 0..=93 {
        let fs = MemoryFs::posix();
        let store = open(fs.clone());
        store.publish(req("main", 1, segment("a"))).unwrap();
        let pointer = fs.read("roots/main/current").unwrap();
        let bytes = if size <= 92 {
            pointer[..size].to_vec()
        } else {
            [pointer.as_slice(), &[0]].concat()
        };
        fs.write("roots/main/current", &bytes).unwrap();
        if size == 92 {
            assert_eq!(store.recover().unwrap().generation, Some(1));
            let before = files(&fs);
            assert_eq!(store.recover().unwrap().generation, Some(1));
            assert_eq!(files(&fs), before);
        } else {
            assert!(store.recover().is_err(), "size {size}");
            assert_eq!(
                fs.read("roots/main/current").unwrap_or_default(),
                if size > 92 { Vec::new() } else { bytes }
            );
        }
    }
}

#[test]
fn checksum_valid_manifest_truncations_and_invalid_utf8_never_become_committed() {
    let source_fs = MemoryFs::posix();
    let source = open(source_fs.clone());
    let published = source.publish(req("main", 1, segment("a"))).unwrap();
    let original = source_fs
        .read(&format!("manifests/{}.man", hex(&published.manifest)))
        .unwrap();
    for cut in 0..original.len() {
        let fs = MemoryFs::posix();
        let store = open(fs.clone());
        store.publish(req("main", 1, segment("a"))).unwrap();
        install_manifest(&fs, &original[..cut], 1);
        assert!(store.recover().is_err(), "cut {cut}");
    }
    let mut invalid = original.clone();
    let identity =
        usize::try_from(u64::from_le_bytes(invalid[88..96].try_into().unwrap())).unwrap();
    invalid[identity + 4] = 0xff;
    resign_manifest(&mut invalid);
    install_manifest(&source_fs, &invalid, 1);
    assert!(source.recover().is_err());
}

#[test]
fn exhausted_generation_cannot_be_published_again() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    let published = store.publish(req("main", 1, segment("a"))).unwrap();
    let mut manifest = fs
        .read(&format!("manifests/{}.man", hex(&published.manifest)))
        .unwrap();
    let identity =
        usize::try_from(u64::from_le_bytes(manifest[88..96].try_into().unwrap())).unwrap();
    let generation = identity + 4 + 5 + 4 + 4; // namespace "local", root "main".
    manifest[generation..generation + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    resign_manifest(&mut manifest);
    install_manifest(&fs, &manifest, u64::MAX);
    assert_eq!(store.committed("main").unwrap(), Some(u64::MAX));
    let before = files(&fs);
    assert!(matches!(
        store.publish(req("main", u64::MAX, segment("b"))),
        Err(StoreError::InvalidRequest { .. })
    ));
    assert_eq!(files(&fs), before);
}

#[test]
fn pins_from_another_store_cannot_read_or_release_local_leases() {
    let fs = MemoryFs::posix();
    let a = open(fs.clone());
    a.publish(req("main", 1, segment("a"))).unwrap();
    let b = open(fs);
    let foreign = a.pin("main").unwrap();
    let local = b.pin("main").unwrap();
    assert_eq!(foreign.id(), local.id());
    let digest = *local.segments()[0].digest();
    assert_eq!(local.root(), "main");
    assert_eq!(local.generation(), 1);
    assert_eq!(local.segments()[0].role(), SegmentRole::Fact);
    assert!(b.read_segment(&foreign, &digest).is_err());
    assert!(b.release(foreign).is_err());
    assert_eq!(b.read_segment(&local, &digest).unwrap(), segment("a"));
    b.release(local).unwrap();
}

#[test]
fn concurrent_pins_read_identical_bytes_and_gc_preserves_them() {
    let store = open(MemoryFs::posix());
    let bytes = segment("a");
    store.publish(req("main", 1, bytes.clone())).unwrap();
    std::thread::scope(|scope| {
        let readers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let pin = store.pin("main").unwrap();
                    let digest = *pin.segments()[0].digest();
                    assert_eq!(store.read_segment(&pin, &digest).unwrap(), bytes);
                    assert!(store.read_segment(&pin, &[0; 32]).is_err());
                    let value = store.read_segment(&pin, &digest).unwrap();
                    store.release(pin).unwrap();
                    value
                })
            })
            .collect();
        assert!(store.gc(GcBudget { objects: u32::MAX }).unwrap().done);
        for reader in readers {
            assert_eq!(reader.join().unwrap(), bytes);
        }
    });
}

#[test]
fn cancellation_capabilities_and_zero_gc_budget_have_repeatable_results() {
    let fs = MemoryFs::posix();
    let store = open(fs.clone());
    for boundary in [
        Boundary::AfterLock,
        Boundary::AfterReserve,
        Boundary::AfterSegmentWrite,
    ] {
        store.cancel_at(boundary);
        assert_eq!(
            store.publish(req("main", 1, segment("a"))).unwrap_err(),
            StoreError::Cancelled
        );
        assert!(fs.held_roots().is_empty());
        assert!(fs.list_prefix("tmp/").unwrap().is_empty());
    }
    store.request_cancel();
    assert_eq!(
        store.publish(req("main", 1, segment("a"))).unwrap_err(),
        StoreError::Cancelled
    );
    store.publish(req("main", 1, segment("a"))).unwrap();
    let before = files(&fs);
    for _ in 0..2 {
        assert!(store.current_pointer_closed("main").unwrap());
        assert_eq!(store.raw_current_generation("main").unwrap(), Some(1));
        assert!(
            store
                .gc_with_mark(GcBudget { objects: 0 }, MarkMode::Reachability)
                .unwrap()
                .done
        );
    }
    assert_eq!(files(&fs), before);
    let store = open(MemoryFs::no_sync());
    assert!(matches!(
        store.publish(req("main", 1, segment("a"))),
        Err(StoreError::UnsupportedDurability { .. })
    ));
    let store = SegmentStore::open(
        MemoryFs::no_sync(),
        StoreOptions {
            mode: DurabilityMode::Ephemeral,
            ..StoreOptions::capped(1_000_000)
        },
    )
    .unwrap();
    store.publish(req("main", 1, segment("a"))).unwrap();
    assert_eq!(Revisions::structural(), Revisions::structural());
}

#[test]
fn filesystem_ports_refuse_hostile_paths_and_preserve_immutable_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let stdfs = StdFs::open(dir.path()).unwrap();
    let memory = MemoryFs::posix();
    for fs in [&stdfs as &dyn StoreFs, &memory as &dyn StoreFs] {
        for path in [
            "", ".", "..", "../x", "a/../b", "a//b", "/x", "a\\b", ".git/x", "a:b", "a\0b",
            "a\u{1b}b", "a.", "a ",
        ] {
            assert!(fs.create(path).is_err(), "{path:?}");
            assert!(fs.read(path).is_err());
            assert!(fs.write(path, b"x").is_err());
            assert!(fs.flush(path).is_err());
            assert!(fs.sync_file(path).is_err());
            assert!(fs.exists(path).is_err());
            assert!(fs.len(path).is_err());
            assert!(fs.unlink(path).is_err());
            assert!(fs.rename_new(path, "tmp/a").is_err());
            assert!(fs.rename_replace("tmp/a", path).is_err());
        }
        for (name, bytes) in [("tmp/a", &b""[..]), ("tmp/b", &b"\xff"[..])] {
            fs.create(name).unwrap();
            fs.write(name, bytes).unwrap();
            fs.flush(name).unwrap();
            fs.sync_file(name).unwrap();
            assert_eq!(fs.read(name).unwrap(), bytes);
            assert_eq!(fs.read(name).unwrap(), bytes);
            assert_eq!(fs.len(name).unwrap(), u64::try_from(bytes.len()).unwrap());
        }
        fs.rename_new("tmp/a", "objects/a").unwrap();
        assert!(fs.rename_new("tmp/b", "objects/a").is_err());
        assert!(fs.read("objects/a").unwrap().is_empty());
        fs.rename_replace("tmp/b", "objects/a").unwrap();
        fs.sync_dir().unwrap();
        assert_eq!(fs.read("objects/a").unwrap(), [0xff]);
    }
    for len in [512, 513] {
        let path = "a".repeat(len);
        assert_eq!(memory.create(&path).is_ok(), len <= 512);
    }
}

#[test]
fn directory_record_counts_match_the_manifest_payloads() {
    for section in [0, 1, 2] {
        let fs = MemoryFs::posix();
        let store = open(fs.clone());
        let published = store.publish(req("main", 1, segment("a"))).unwrap();
        let mut bytes = fs
            .read(&format!("manifests/{}.man", hex(&published.manifest)))
            .unwrap();
        let at = 80 + section * 64 + 24;
        bytes[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        resign_manifest(&mut bytes);
        install_manifest(&fs, &bytes, 1);
        assert!(store.recover().is_err(), "section {section}");
    }
}

#[test]
fn lock_ports_validate_root_names_before_changing_lock_state() {
    let dir = tempfile::tempdir().unwrap();
    let stdfs = StdFs::open(dir.path()).unwrap();
    let memory = MemoryFs::posix();
    for fs in [&stdfs as &dyn StoreFs, &memory as &dyn StoreFs] {
        for root in ["", ".", "..", ".git", "a.", "a/b", "a:b", "a\0b"] {
            assert!(fs.try_lock(root).is_err(), "{root:?}");
            assert!(fs.unlock(root).is_err(), "{root:?}");
            assert!(!fs.lock_held(root));
            assert!(fs.held_roots().is_empty());
        }
        fs.try_lock("a").unwrap();
        assert_eq!(fs.try_lock("a").unwrap_err(), StoreError::Busy);
        assert!(fs.lock_held("a"));
        fs.unlock("a").unwrap();
        fs.unlock("a").unwrap();
    }
}

#[test]
fn immutable_rename_has_exactly_one_winner_under_concurrent_use() {
    let dir = tempfile::tempdir().unwrap();
    let fs = StdFs::open(dir.path()).unwrap();
    let barrier = std::sync::Barrier::new(8);
    for round in 0..16 {
        let paths: Vec<_> = (0..8).map(|id| format!("tmp/{round}-{id}")).collect();
        for (id, path) in paths.iter().enumerate() {
            fs.create(path).unwrap();
            fs.write(path, &[u8::try_from(id).unwrap()]).unwrap();
        }
        let destination = format!("objects/{round}");
        std::thread::scope(|scope| {
            let writers: Vec<_> = paths
                .iter()
                .enumerate()
                .map(|(id, path)| {
                    let fs = &fs;
                    let barrier = &barrier;
                    let destination = &destination;
                    scope.spawn(move || {
                        barrier.wait();
                        fs.rename_new(path, destination).map(|()| id)
                    })
                })
                .collect();
            let winners: Vec<_> = writers
                .into_iter()
                .filter_map(|writer| writer.join().unwrap().ok())
                .collect();
            assert_eq!(winners.len(), 1);
            assert_eq!(
                fs.read(&destination).unwrap(),
                [u8::try_from(winners[0]).unwrap()]
            );
        });
    }
}

#[test]
fn memory_profiles_handle_disk_limits_holds_crashes_and_capabilities() {
    for fs in [MemoryFs::posix(), MemoryFs::windows()] {
        assert!(fs.capabilities().sync);
        fs.create("tmp/a").unwrap();
        for n in [0, 1, 16, 17] {
            fs.set_disk_budget(Some(16));
            let bytes = vec![0xff; n];
            let result = fs.write("tmp/a", &bytes);
            assert_eq!(result.is_ok(), n <= 16);
            fs.set_disk_budget(None);
        }
        fs.write("tmp/a", b"a").unwrap();
        fs.sync_file("tmp/a").unwrap();
        fs.sync_dir().unwrap();
        let hold = fs.hold("tmp/a").unwrap();
        assert_eq!(fs.read_hold(hold).unwrap(), b"a");
        fs.release_hold(hold).unwrap();
        assert!(fs.release_hold(hold).is_err());
        assert!(fs.read_hold(0).is_err());
        assert!(fs.read_hold(u64::MAX).is_err());
        fs.write("tmp/a", b"dirty").unwrap();
        fs.crash();
        assert_eq!(fs.read("tmp/a").unwrap(), b"a");
        fs.stick_lock("main");
        fs.crash();
        assert!(fs.lock_held("main"));
        assert_eq!(fs.try_lock("main").unwrap_err(), StoreError::Busy);
    }
    for boundary in Boundary::ALL {
        assert!(!boundary.as_str().is_empty());
        assert_eq!(
            boundary.publishes_new_generation(),
            matches!(
                boundary,
                Boundary::AfterPointerDirSync | Boundary::BeforeUnlock
            )
        );
        let fs = MemoryFs::posix();
        fs.arm_crash(Some(boundary));
        assert!(matches!(
            fs.checkpoint(boundary),
            Err(StoreError::Crashed { .. })
        ));
        fs.arm_crash(None);
        fs.checkpoint(boundary).unwrap();
    }
}

#[test]
fn segment_size_boundary_refusals_are_bounded_and_leave_an_empty_store() {
    let limit =
        usize::try_from(dctx_map_segment::DecoderLimits::standard().max_segment_bytes).unwrap();
    for len in [limit, limit + 1] {
        let store = open(MemoryFs::posix());
        assert!(store.publish(req("main", 1, vec![0xff; len])).is_err());
        assert_eq!(store.committed("main").unwrap(), None);
        assert!(!store.contains_segment(&[0; 32]).unwrap());
    }
}

#[test]
fn io_details_are_bounded_and_cannot_inject_terminal_controls() {
    for text in [
        String::new(),
        "x".into(),
        "x".repeat(160),
        "é".repeat(81),
        "\u{1b}[31m\n\0".into(),
    ] {
        let StoreError::Io { detail } = StoreError::io(&text) else {
            unreachable!()
        };
        assert!(detail.len() <= 160);
        assert!(!detail.chars().any(char::is_control));
        assert_eq!(StoreError::io(&text), StoreError::io(&text));
    }
    // Display must stop formatting as soon as the output is full.
    struct Long;
    impl std::fmt::Display for Long {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for _ in 0..161 {
                f.write_str("x")?;
            }
            panic!("formatting continued beyond the detail cap");
        }
    }
    let StoreError::Io { detail } = StoreError::io(Long) else {
        unreachable!()
    };
    assert_eq!(detail, "x".repeat(160));
}

#[test]
fn every_store_error_names_a_next_step() {
    let errors = [
        StoreError::CacheLimit {
            requested: 1,
            available: 0,
        },
        StoreError::DiskFull,
        StoreError::Busy,
        StoreError::Cancelled,
        StoreError::CorruptIndex { reason: "checksum" },
        StoreError::UnsupportedDurability { capability: "sync" },
        StoreError::InvalidRequest { reason: "pin" },
        StoreError::UnsupportedVersion { found: u16::MAX },
        StoreError::VcsRefused,
        StoreError::SharingViolation,
        StoreError::io("read"),
        StoreError::Crashed { boundary: "store" },
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
