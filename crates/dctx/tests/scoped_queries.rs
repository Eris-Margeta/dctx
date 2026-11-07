// SPDX-License-Identifier: Apache-2.0
//! Direct queries share parsing, policy and byte output with admitted snapshots.

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use dctx::memory::{CancelFlag, MemorySource};
use dctx::{
    DctxError, HostGrants, MapRequest, OperationLimits, RuleTexts, SelectionPolicy,
    SnapshotRequest, StoragePolicy, VecSink, Verb,
};

fn fixture() -> MemorySource {
    let mut source = MemorySource::new();
    for (path, body) in [
        ("src/a.rs", "fn alpha() {}\nfn beta() {}\n"),
        (
            "src/b.py",
            "def alpha():\n    return 'needle'\n\ndef alpha():\n    return 'NEEDLE'\n",
        ),
        ("docs/guide.md", "# Guide\n\n## Details\n\nneedle\n"),
        ("config.toml", "[server]\nport = 8080\n"),
        ("unrelated/other.py", "def unrelated():\n    return 3\n"),
        ("src/.env", "needle=PRIVATE\n"),
    ] {
        source.insert_file(path, body.as_bytes(), 0).expect("file");
    }
    source
}

fn request(verb: Verb, target: &str, within: Option<&str>) -> MapRequest {
    let mut request = MapRequest::new(verb);
    if matches!(verb, Verb::Find | Verb::Grep) {
        request.query = target.into();
    } else {
        request.target = target.into();
    }
    request.within = within.map(str::to_owned);
    request
}

#[test]
fn direct_and_snapshot_pages_are_identical_in_both_formats_and_across_pages() {
    for (verb, target, within) in [
        (Verb::Read, "s:src/a.rs#alpha", None),
        (Verb::Read, "s:src/b.py#alpha~2", None),
        (Verb::Read, "m:docs/guide.md", None),
        (Verb::Map, "m:src/a.rs", None),
        (Verb::Skeleton, "m:src/b.py", None),
        (Verb::Explain, "s:src/a.rs#alpha", None),
        (Verb::Find, "alpha", Some("src")),
        (Verb::Find, "needle", Some("src/b.py")),
        (Verb::Find, "server.port", Some("config.toml")),
        (Verb::Grep, "needle", Some("src")),
        (Verb::Grep, "needle", Some("docs/guide.md")),
        (Verb::Grep, "absent", Some("missing")),
        (Verb::Read, "m:missing.rs", None),
    ] {
        for format in [1, 2] {
            for meta in [false, true] {
                let mut direct = common::open_session(fixture());
                let mut full = common::open_session(fixture());
                let mut ask = request(verb, target, within);
                ask.output_format = format;
                ask.meta = meta;
                ask.limits.records = Some(1);
                let snapshot = SnapshotRequest::strict();
                let direct_handle = direct
                    .snapshot_for_query(&snapshot, &ask, "")
                    .expect("scope");
                let full_handle = full.snapshot(&snapshot).expect("snapshot");
                for _ in 0..10 {
                    let mut left = VecSink::new();
                    let mut right = VecSink::new();
                    let first = direct
                        .query(&direct_handle, &ask, &mut left)
                        .expect("direct query");
                    let second = full
                        .query(&full_handle, &ask, &mut right)
                        .expect("snapshot query");
                    assert_eq!(
                        left.as_bytes(),
                        right.as_bytes(),
                        "{verb:?} {target} {within:?} format={format} meta={meta}"
                    );
                    if target != "absent" && target != "m:missing.rs" {
                        assert!(
                            !first.records.is_empty(),
                            "known target {verb:?} {target} {within:?}"
                        );
                    }
                    assert_eq!(first.next, second.next);
                    if first.next.is_none() || first.outcome.is_some() {
                        break;
                    }
                    ask.continuation = first.next;
                }
            }
        }
    }
}

#[test]
fn a_file_read_never_lists_the_place_or_admits_cache_bytes() {
    let mut source = fixture();
    source.fail_listing_of("");
    source.fail_listing_of("src");
    let shared = common::SharedSource::new(source);
    let log = shared.handle();
    let mut session = common::session_parts(
        shared,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 1 },
        SelectionPolicy::from_texts(&RuleTexts::default()),
    );
    let ask = request(Verb::Read, "s:src/b.py#alpha", None);
    let handle = session
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "unused")
        .expect("no cache admission");
    let mut sink = VecSink::new();
    let answer = session.query(&handle, &ask, &mut sink).expect("read");
    assert!(
        answer.records[0]
            .source_utf8
            .as_deref()
            .expect("body")
            .contains("needle")
    );
    assert_eq!(session.cache_status().committed_bytes, 0);
    assert!(
        session
            .export_graph(&handle, "json", &MapRequest::new(Verb::Map))
            .is_err()
    );
    let source = log.lock().expect("source");
    assert!(source.listed().is_empty());
    assert_eq!(source.reads(), ["src/b.py", "src/b.py"]);
    assert!(
        source
            .metadata_requests()
            .iter()
            .all(|path| path == "src/b.py")
    );
}

#[test]
fn scoped_search_never_visits_a_sibling_and_policy_blocks_ancestor_content() {
    let shared = common::SharedSource::new(fixture());
    let log = shared.handle();
    let selection = SelectionPolicy::from_texts(&RuleTexts {
        local_ignore: Some("src/a.rs\n"),
        global_omit: Some("src/b.py\n"),
        ..RuleTexts::default()
    });
    let mut session = common::session_parts(
        shared,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 1 },
        selection,
    );
    let ask = request(Verb::Grep, "needle", Some("src"));
    let handle = session
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("scope");
    let mut sink = VecSink::new();
    assert!(
        session
            .query(&handle, &ask, &mut sink)
            .expect("grep")
            .records
            .is_empty()
    );
    let source = log.lock().expect("source");
    assert!(source.listed().iter().all(|dir| dir == "src"));
    assert!(source.reads().is_empty());
}

#[test]
fn scoped_read_checks_the_digest_and_scoped_cursors_reject_edits() {
    let shared = common::SharedSource::new(fixture());
    let state = shared.handle();
    let mut session = common::open_session(shared);
    let ask = request(Verb::Read, "s:src/a.rs#alpha", None);
    let handle = session
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("scope");
    state
        .lock()
        .expect("source")
        .insert_file("src/a.rs", b"fn changed() {}\n", 0)
        .expect("edit");
    let mut sink = VecSink::new();
    assert_eq!(
        session
            .query(&handle, &ask, &mut sink)
            .expect_err("changed"),
        DctxError::SourceChanged
    );
    assert!(sink.is_empty());

    let mut first = common::open_session(fixture());
    let mut ask = request(Verb::Map, "m:src/a.rs", None);
    ask.limits.records = Some(1);
    let handle = first
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("scope");
    let answer = first
        .query(&handle, &ask, &mut VecSink::new())
        .expect("page");
    ask.continuation = Some(answer.next.expect("cursor"));
    let mut source = fixture();
    source
        .insert_file("src/a.rs", b"fn alpha() {}\nfn charlie() {}\n", 0)
        .expect("edit");
    let mut second = common::open_session(source);
    let handle = second
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("new scope");
    assert_eq!(
        second
            .query(&handle, &ask, &mut VecSink::new())
            .expect_err("old cursor"),
        DctxError::StaleContinuation
    );
}

#[test]
fn invalid_paths_and_scope_limits_fail_without_admitting_a_generation() {
    for path in ["s:../outside.py#x", "s:/outside.py#x", "s:src/../a.rs#x"] {
        let mut session = common::open_session(fixture());
        assert!(
            session
                .snapshot_for_query(
                    &SnapshotRequest::strict(),
                    &request(Verb::Read, path, None),
                    ""
                )
                .is_err()
        );
        assert_eq!(session.generation_count(), 0);
    }
    let mut session = common::open_session(fixture());
    let mut snapshot = SnapshotRequest::strict();
    snapshot.limits = OperationLimits {
        source_bytes: Some(1),
        ..OperationLimits::default()
    };
    assert!(matches!(
        session.snapshot_for_query(&snapshot, &request(Verb::Grep, "needle", Some("src")), ""),
        Err(DctxError::ResourceLimit { .. })
    ));
    assert_eq!(session.generation_count(), 0);
}

#[test]
fn binary_invalid_utf8_unreadable_and_oversize_files_keep_snapshot_outcomes() {
    for (body, unreadable) in [
        (vec![0, b'a', b'\n'], false),
        (vec![0xff, b'\n'], false),
        (b"fn private() {}\n".to_vec(), true),
        (vec![b'x'; 8 * 1024 * 1024 + 1], false),
    ] {
        let source = || {
            let mut source = MemorySource::new();
            source.insert_file("target.rs", &body, 0).expect("target");
            if unreadable {
                source.fail_reads_of("target.rs");
            }
            source
        };
        let mut full = common::open_session(source());
        let mut direct = common::open_session(source());
        let ask = request(Verb::Read, "m:target.rs", None);
        let snapshot = SnapshotRequest::strict();
        let left = full.snapshot(&snapshot).expect("full");
        let right = direct
            .snapshot_for_query(&snapshot, &ask, "")
            .expect("direct");
        let mut left_sink = VecSink::new();
        let mut right_sink = VecSink::new();
        let left_answer = full.query(&left, &ask, &mut left_sink);
        let right_answer = direct.query(&right, &ask, &mut right_sink);
        assert_eq!(left_answer.as_ref().err(), right_answer.as_ref().err());
        assert_eq!(left_sink.as_bytes(), right_sink.as_bytes());
    }
}

#[test]
fn excluded_or_omitted_ancestors_never_release_source() {
    for rules in [
        RuleTexts {
            local_ignore: Some("src/\n"),
            ..RuleTexts::default()
        },
        RuleTexts {
            global_omit: Some("src/\n"),
            ..RuleTexts::default()
        },
    ] {
        let shared = common::SharedSource::new(fixture());
        let log = shared.handle();
        let mut session = common::session_parts(
            shared,
            Arc::new(CancelFlag::new()),
            HostGrants::PermitAll,
            StoragePolicy::Ephemeral { max_bytes: 1 },
            SelectionPolicy::from_texts(&rules),
        );
        let ask = request(Verb::Read, "s:src/b.py#alpha", None);
        let handle = session
            .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
            .expect("scope");
        let answer = session
            .query(&handle, &ask, &mut VecSink::new())
            .expect("gone");
        assert!(answer.records.is_empty());
        assert!(log.lock().expect("source").reads().is_empty());
    }
}

#[test]
fn directory_only_ignores_keep_files_and_never_list_excluded_directories() {
    let source = common::SharedSource::new(fixture());
    let log = source.handle();
    let mut session = common::session_parts(
        source,
        Arc::new(CancelFlag::new()),
        HostGrants::PermitAll,
        StoragePolicy::Ephemeral { max_bytes: 1 },
        SelectionPolicy::from_texts(&RuleTexts {
            local_ignore: Some("src/b.py/\ndocs/\n"),
            ..RuleTexts::default()
        }),
    );
    let ask = request(Verb::Grep, "needle", Some("src/b.py"));
    let handle = session
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("file");
    assert!(
        !session
            .query(&handle, &ask, &mut VecSink::new())
            .expect("grep")
            .records
            .is_empty()
    );
    let ask = request(Verb::Grep, "needle", Some("docs"));
    let handle = session
        .snapshot_for_query(&SnapshotRequest::strict(), &ask, "")
        .expect("excluded dir");
    assert!(
        session
            .query(&handle, &ask, &mut VecSink::new())
            .expect("grep")
            .records
            .is_empty()
    );
    assert!(log.lock().expect("source").listed().is_empty());
}

#[cfg(unix)]
#[test]
fn unrepresentable_names_label_the_directory_partial_but_do_not_taint_a_file() {
    let fixture = tempfile::tempdir().expect("fixture");
    let root = fixture.path();
    std::fs::create_dir(root.join("src")).expect("src");
    std::fs::write(
        root.join("src/kept.py"),
        "def target():\n    return 'needle'\n",
    )
    .expect("source");
    std::fs::write(root.join("src/bad\\name.py"), "needle").expect("unrepresentable name");
    for scope in ["src", "src/kept.py"] {
        let mut full = common::open_session(dctx::FsSource::open(root).expect("full source"));
        let mut direct = common::open_session(dctx::FsSource::open(root).expect("direct source"));
        let ask = request(Verb::Grep, "needle", Some(scope));
        let snapshot = SnapshotRequest::strict();
        let full = full
            .snapshot(&snapshot)
            .map(|handle| (full, handle))
            .expect("snapshot");
        let mut direct_sink = VecSink::new();
        let handle = direct
            .snapshot_for_query(&snapshot, &ask, "")
            .expect("scope");
        let answer = direct
            .query(&handle, &ask, &mut direct_sink)
            .expect("direct");
        assert_eq!(
            answer.precision.completion,
            if scope == "src" {
                "partial"
            } else {
                "complete"
            }
        );
        let (mut full, handle) = full;
        let mut full_sink = VecSink::new();
        full.query(&handle, &ask, &mut full_sink).expect("full");
        assert_eq!(full_sink.as_bytes(), direct_sink.as_bytes());
    }
}

#[test]
fn a_direct_scan_enforces_elapsed_time_even_with_a_frozen_host_clock() {
    use dctx::Source;
    use dctx_core::{
        datatypes::NodeType,
        error::DctxError as CoreError,
        facts::{EntryMeta, ReadGrant, SourceEntry},
        ids::RelativePath,
    };
    struct SlowSource(MemorySource);
    impl Source for SlowSource {
        fn root_kind(&self) -> Result<NodeType, CoreError> {
            self.0.root_kind()
        }
        fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
            self.0.entries(dir)
        }
        fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
            self.0.metadata(path)
        }
        fn read(&self, grant: &ReadGrant, cap: u64) -> Result<Vec<u8>, CoreError> {
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.0.read(grant, cap)
        }
    }
    let mut session = common::open_session(SlowSource(fixture()));
    let mut snapshot = SnapshotRequest::strict();
    snapshot.limits.deadline_ms = Some(1);
    assert!(matches!(
        session.snapshot_for_query(
            &snapshot,
            &request(Verb::Read, "s:src/a.rs#alpha", None),
            ""
        ),
        Err(DctxError::ResourceLimit {
            resource: dctx::Resource::Deadline,
            ..
        })
    ));
    assert_eq!(session.generation_count(), 0);
}
