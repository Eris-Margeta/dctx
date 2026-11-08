// SPDX-License-Identifier: Apache-2.0
//! Engine context identity, delivery and refusal contracts over the golden repository.
#![cfg(all(feature = "map", feature = "segments"))]
#![forbid(unsafe_code)]
mod common;

use dctx::context::{ContextSessions, NamedContextRequest, chunk_id};
use dctx::{ErrorCode, SnapshotRequest};

#[test]
fn named_context_matches_committed_golden_in_both_formats() {
    let mut session = common::open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    for (format, golden) in [
        (1, include_str!("fixtures/context/ping.txt")),
        (2, include_str!("fixtures/context/ping.json")),
    ] {
        for name in ["ping", "s:src/lib.rs#ping"] {
            let mut request = NamedContextRequest::new(name);
            request.output_format = format;
            let report = sessions.context(&mut session, &handle, &request).unwrap();
            assert_eq!(report.text, golden);
            assert_eq!(report.chunks.len(), 1);
        }
    }
}

#[test]
fn ids_are_domain_separated_and_cover_every_payload_byte() {
    let id = chunk_id("source", "s:a#f", "abc").unwrap();
    assert_eq!(
        id,
        "chunk:v1:sha256:f017cd059eb93c4d2ad7f3d8867522a9cfc4fe006156f0d4b889f6c37ccc22f4:3"
    );
    for (role, target, content) in [
        ("related", "s:a#f", "abc"),
        ("source", "s:a#g", "abc"),
        ("source", "s:a#f", "abc\n"),
        ("source", "s:a#f", "abç"),
    ] {
        assert_ne!(id, chunk_id(role, target, content).unwrap());
    }
    assert_ne!(
        chunk_id("a", "bc", "d").unwrap(),
        chunk_id("ab", "c", "d").unwrap()
    );
}

#[test]
fn delivery_is_opt_in_isolated_resettable_and_cross_format() {
    let mut session = common::open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    let mut request = NamedContextRequest::new("ping");
    request.session_id = Some("a".into());
    let full = sessions.context(&mut session, &handle, &request).unwrap();
    assert_eq!(
        full.text,
        sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .text
    );
    request.dedupe = true;
    assert_eq!(
        full.text,
        sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .text
    );
    let referenced = sessions.context(&mut session, &handle, &request).unwrap();
    assert!(referenced.chunks[0].reference);
    assert!(!referenced.text.contains("pub fn"));
    assert!(referenced.text.contains(&full.chunks[0].id));
    request.output_format = 2;
    assert!(
        sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
    request.session_id = Some("b".into());
    assert!(
        !sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
    sessions.reset("a");
    request.session_id = Some("a".into());
    assert!(
        !sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
}

#[test]
fn stale_and_budget_refusals_never_mark_chunks_delivered() {
    let shared = common::SharedSource::new(common::repository());
    let source = shared.handle();
    let original = format!(
        "/// Respond.\npub fn ping() {{ let text = \"{}\"; }}\n",
        "x".repeat(200)
    );
    source
        .lock()
        .unwrap()
        .insert_file("src/lib.rs", original.as_bytes(), 0)
        .unwrap();
    let mut session = common::open_session(shared);
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    let mut request = NamedContextRequest::new("ping");
    request.session_id = Some("a".into());
    request.dedupe = true;
    request.output_format = 2;
    request.budget = 100;
    assert_eq!(
        sessions
            .context(&mut session, &handle, &request)
            .unwrap_err()
            .code(),
        ErrorCode::BudgetTooSmall
    );
    request.budget = 3000;
    source
        .lock()
        .unwrap()
        .insert_file("src/lib.rs", common::PONG, 0)
        .unwrap();
    assert!(sessions.context(&mut session, &handle, &request).is_err());
    source
        .lock()
        .unwrap()
        .insert_file("src/lib.rs", original.as_bytes(), 0)
        .unwrap();
    assert!(
        !sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
    source
        .lock()
        .unwrap()
        .insert_file("src/lib.rs", common::PONG, 0)
        .unwrap();
    assert!(
        sessions.context(&mut session, &handle, &request).is_err(),
        "dedupe must still verify the source"
    );
}

#[test]
fn file_targets_missing_targets_and_invalid_sessions() {
    let mut session = common::open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    for file in ["src/lib.rs", "./src/lib.rs", "m:src/lib.rs"] {
        let report = sessions
            .context(&mut session, &handle, &NamedContextRequest::new(file))
            .unwrap();
        assert_eq!(report.target, "m:src/lib.rs");
        assert!(report.chunks[0].content.contains("pub fn ping() {}"));
    }
    let doc = sessions
        .context(
            &mut session,
            &handle,
            &NamedContextRequest::new("d:README.md#Fixture"),
        )
        .unwrap();
    assert_eq!(doc.target, "d:README.md#Fixture");
    assert!(doc.chunks[0].content.contains("# Fixture"));
    let missing = sessions
        .context(
            &mut session,
            &handle,
            &NamedContextRequest::new("missing_target"),
        )
        .unwrap();
    assert!(missing.chunks.is_empty());
    for target in ["", "../src/lib.rs", "/src/lib.rs"] {
        assert!(
            sessions
                .context(&mut session, &handle, &NamedContextRequest::new(target))
                .is_err()
        );
    }
    let mut request = NamedContextRequest::new("ping");
    request.dedupe = true;
    assert!(sessions.context(&mut session, &handle, &request).is_err());
}

#[test]
fn ranked_related_chunks_are_budgeted_without_poisoning_undelivered_ids() {
    let mut source = dctx::memory::MemorySource::new();
    source.insert_file("calls.py", b"def entry():\n    return helper()\n\ndef helper():\n    return 1\n\ndef caller():\n    entry()\n", 0).unwrap();
    let mut session = common::open_session(source);
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    let mut request = NamedContextRequest::new("entry");
    request.dedupe = true;
    request.session_id = Some("ranking".into());
    request.budget = 150;
    let small = sessions.context(&mut session, &handle, &request).unwrap();
    assert_eq!(small.chunks[0].target, "s:calls.py#entry");
    assert!(small.more > 0, "{}", small.text);
    assert!(dctx_core::budget::estimate_tokens(small.text.len() as u64) <= 150);
    request.budget = 3000;
    let large = sessions.context(&mut session, &handle, &request).unwrap();
    assert!(large.chunks.len() > small.chunks.len());
    for chunk in &large.chunks {
        assert_eq!(
            chunk.reference,
            small.chunks.iter().any(|prior| prior.id == chunk.id)
        );
    }
    let related: Vec<_> = large
        .chunks
        .iter()
        .skip(1)
        .map(|chunk| chunk.target.as_str())
        .collect();
    assert!(related.contains(&"s:calls.py#helper"));
    assert!(related.contains(&"s:calls.py#caller"));
}

#[test]
fn unrelated_snapshot_changes_keep_ids_and_source_edits_change_ids() {
    let source = common::SharedSource::new(common::repository());
    let shared = source.handle();
    let mut session = common::open_session(source);
    let mut sessions = ContextSessions::default();
    let mut request = NamedContextRequest::new("ping");
    request.dedupe = true;
    request.session_id = Some("snapshots".into());
    let first_handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let first = sessions
        .context(&mut session, &first_handle, &request)
        .unwrap();
    shared
        .lock()
        .unwrap()
        .insert_file("other.txt", b"unrelated", 0)
        .unwrap();
    let next_handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let next = sessions
        .context(&mut session, &next_handle, &request)
        .unwrap();
    assert_eq!(next.chunks[0].id, first.chunks[0].id);
    assert!(next.chunks[0].reference);
    shared
        .lock()
        .unwrap()
        .insert_file(
            "src/lib.rs",
            b"/// Respond.\npub fn ping() { pong(); }\n",
            0,
        )
        .unwrap();
    let edited_handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let edited = sessions
        .context(&mut session, &edited_handle, &request)
        .unwrap();
    assert_ne!(edited.chunks[0].id, first.chunks[0].id);
    assert!(!edited.chunks[0].reference);
}

#[test]
fn delivery_capacity_refuses_without_eviction_and_reset_frees_a_slot() {
    let mut session = common::open_session(common::repository());
    let handle = session.snapshot(&SnapshotRequest::immutable()).unwrap();
    let mut sessions = ContextSessions::default();
    let mut request = NamedContextRequest::new("ping");
    request.dedupe = true;
    for index in 0..128 {
        request.session_id = Some(index.to_string());
        assert!(
            !sessions
                .context(&mut session, &handle, &request)
                .unwrap()
                .chunks[0]
                .reference
        );
    }
    request.session_id = Some("overflow".into());
    assert!(sessions.context(&mut session, &handle, &request).is_err());
    request.session_id = Some("0".into());
    assert!(
        sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
    sessions.reset("0");
    request.session_id = Some("overflow".into());
    assert!(
        !sessions
            .context(&mut session, &handle, &request)
            .unwrap()
            .chunks[0]
            .reference
    );
}
