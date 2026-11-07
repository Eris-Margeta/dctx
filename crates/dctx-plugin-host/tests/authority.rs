// SPDX-License-Identifier: Apache-2.0
//! PP1 authority, PP2 privacy and PP4 binding through the broker with fake ports.
//! Each refusal is paired with its positive control; counts are spy observations (properties.md).
//! Traces to Authority.tla `Admit`, `Commit`, `Read`, `Narrow`, `ChangeSelection`, `Halt`, `Consent`.

mod common;

use common::*;
use dctx_plugin_host::{Decision, HandleKind, Layer, ModelAction, Reply, Selection};
use dctx_plugin_protocol::ErrorCode;
use dctx_plugin_protocol::records::broker::ChunkRead;
use dctx_plugin_protocol::records::capability::OutputOperation;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::{ProjectPath, Role};

#[test]
fn included_read_returns_abc_control() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let reply = broker
        .handle(&read_req(&handle, 0, 64, None))
        .expect("read");
    assert!(
        matches!(reply, Reply::Chunk { ref bytes, size: 3, precondition: Some(_) } if bytes == b"abc")
    );
    let trace = broker.authority().trace();
    assert!(trace.precedes(ModelAction::AuthorityAdmit, ModelAction::AuthorityCommit));
    assert!(trace.precedes(ModelAction::AuthorityCommit, ModelAction::AuthorityRead));
}

#[test]
fn excluded_omitted_and_debug_sources_are_never_opened() {
    for decision in [
        Decision::Exclude,
        Decision::OmitContent,
        Decision::DebugVisible,
    ] {
        let mut broker = broker_with(
            layers(all_caps()),
            Selection::new([(path(), decision)]),
            budget(1 << 20, 8),
        );
        let handle = file_handle(&mut broker);
        let refused = broker
            .handle(&read_req(&handle, 0, 3, None))
            .expect_err("excluded");
        assert_eq!(
            (refused.code, refused.reason),
            (ErrorCode::Denied, "source-not-included"),
            "{decision:?}"
        );
        assert_eq!(
            (broker.files().identities, broker.files().reads),
            (0, 0),
            "{decision:?}"
        );
        assert!(broker.issue_precondition(&handle).is_err());
        assert_eq!(broker.files().identities, 0);
    }
}

#[test]
fn organization_intersection_denies_write_with_control() {
    let mut narrow = broker_with(
        layers(vec![common::read_cap()]),
        included(),
        budget(1 << 20, 8),
    );
    let handle = file_handle(&mut narrow);
    let pre = first_read(&mut narrow, &handle);
    let stage = Request::FileStage(dctx_plugin_protocol::records::broker::FileStage {
        resource: handle.clone(),
        precondition: pre,
        upload: None,
        offset: 0,
        bytes: dctx_plugin_protocol::records::bytes::Chunk::new(b"abc\nx").expect("chunk"),
        final_chunk: true,
    });
    assert_eq!(
        narrow.handle(&stage).map_err(|e| e.reason),
        Err("not-granted")
    );
    assert_eq!(narrow.files().replaces, 0);
    let mut full = broker();
    assert!(matches!(
        replace_with(&mut full, b"abc\nx", "a:1"),
        Ok(Reply::Receipt {
            replayed: false,
            ..
        })
    ));
    assert_eq!(full.files().content(&path()), Some(&b"abc\nx"[..]));
}

#[test]
fn revocation_after_staging_refuses_replace_with_zero_effects() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    let upload = stage(&mut broker, &handle, &pre, b"abc\nx");
    broker
        .authority_mut()
        .narrow(Layer::Consent, &[common::read_cap()]);
    let refused = broker
        .handle(&replace_req(&handle, &pre, &upload, "a:1"))
        .expect_err("revoked");
    assert_eq!(refused.reason, "not-granted");
    assert_eq!(broker.files().replaces, 0);
    broker.authority_mut().approve(Layer::Consent, all_caps());
    assert!(
        broker
            .handle(&replace_req(&handle, &pre, &upload, "a:1"))
            .is_ok()
    );
    assert_eq!(broker.files().replaces, 1);
}

#[test]
fn selection_change_revokes_later_chunks() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    assert!(
        broker
            .handle(&read_req(&handle, 1, 2, Some(pre.clone())))
            .is_ok()
    );
    broker
        .authority_mut()
        .change_selection(Selection::new([(path(), Decision::Exclude)]));
    assert_eq!(
        broker
            .handle(&read_req(&handle, 1, 2, Some(pre)))
            .map_err(|e| e.reason),
        Err("source-not-included")
    );
    assert_eq!(broker.files().reads, 2);
}

#[test]
fn changed_file_refuses_bound_chunk_with_control() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    let pre = first_read(&mut broker, &handle);
    broker.files_mut().put(&path(), b"abd");
    assert_eq!(
        broker
            .handle(&read_req(&handle, 0, 3, Some(pre)))
            .map_err(|e| e.reason),
        Err("file-changed")
    );
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_ok());
}

#[test]
fn foreign_handles_are_refused_for_every_bound_field() {
    let mut broker = broker();
    let mut foreign = Vec::new();
    for field in 0..7 {
        let mut bound = binding("op:1");
        match field {
            0 => {
                bound.plugin =
                    dctx_plugin_protocol::records::scalars::PluginId::new("evil/git").expect("id")
            }
            1 => {
                bound.artifact = dctx_plugin_protocol::records::scalars::Digest::new(&format!(
                    "sha256:{}",
                    "b".repeat(64)
                ))
                .expect("d")
            }
            2 => bound.root = token("project:2"),
            3 => bound.invocation = token("op:2"),
            4 => bound.session = token("s:2"),
            5 => bound.generation = 2,
            _ => bound.namespace = token("ns:other"),
        }
        foreign.push(
            broker
                .register_bound(bound, HandleKind::File { path: path() })
                .expect("handle"),
        );
    }
    for handle in &foreign {
        assert_eq!(
            broker
                .handle(&read_req(handle, 0, 3, None))
                .map_err(|e| e.reason),
            Err("binding-mismatch")
        );
    }
    let own = file_handle(&mut broker);
    assert!(broker.handle(&read_req(&own, 0, 3, None)).is_ok());
}

#[test]
fn output_provenance_is_rechecked_before_serving_cached_bytes() {
    let mut broker = broker();
    let source = ProjectPath::new("src/a.rs").expect("p");
    let output = broker
        .register(HandleKind::Output {
            role: Role::new("pack-text").expect("role"),
            operation: OutputOperation::Pack,
            provenance: vec![source.clone()],
            bytes: b"packed".to_vec(),
        })
        .expect("output");
    let read = Request::OutputRead(ChunkRead {
        handle: output.clone(),
        offset: 0,
        length: 6,
        precondition: None,
    });
    assert!(
        matches!(broker.handle(&read), Ok(Reply::Chunk { ref bytes, .. }) if bytes == b"packed")
    );
    broker.authority_mut().change_selection(Selection::new([
        (path(), Decision::Include),
        (source, Decision::Exclude),
    ]));
    assert_eq!(
        broker.handle(&read).map_err(|e| e.reason),
        Err("provenance-not-included")
    );
}

#[test]
fn halt_closes_admission_and_revokes_handles() {
    let mut broker = broker();
    let handle = file_handle(&mut broker);
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_ok());
    broker.halt();
    assert!(broker.handle(&read_req(&handle, 0, 3, None)).is_err());
    assert!(broker.authority().halted());
    assert!(
        broker
            .authority()
            .trace()
            .contains(ModelAction::AuthorityHalt)
    );
}

#[test]
fn refusals_are_audited_without_source_bytes() {
    let mut broker = broker_with(
        layers(all_caps()),
        Selection::new([(path(), Decision::Exclude)]),
        budget(1 << 20, 8),
    );
    let handle = file_handle(&mut broker);
    let _ = broker.handle(&read_req(&handle, 0, 3, None));
    let refusals = broker
        .audit()
        .count(dctx_plugin_host::ports::AuditKind::Refusal);
    assert_eq!(refusals, 1);
    assert!(broker.audit().records.iter().all(|r| r.reason != "abc"));
}
