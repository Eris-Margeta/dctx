// SPDX-License-Identifier: Apache-2.0
//! Standalone protocol conformance: the real worker loop against a host tracker from dctx-plugin-protocol.
//! Implements PLUGINS.md §7.2 PL-G "standalone protocol conformance" and G-DISTRIBUTION "same corpus
//! through standalone/same-binary workers": the parity corpus over frames, negotiation goldens, lifecycle
//! refusals with positive controls, probe/cancel during a broker wait, and the command surface.

mod common;

use common::cases::{Observed, cases, check, setup};
use common::host::Host;
use common::{FakeBroker, bytes, initialize, invocation, token};
use dctx_plugin_git::adapters::WorkerError;
use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
use dctx_plugin_protocol::records::common::{CommandRecord, ConfigValue, Scalar};
use dctx_plugin_protocol::records::envelope::{Outcome, SessionBody};
use dctx_plugin_protocol::records::events::Event;
use dctx_plugin_protocol::records::invocation::HookPayload;
use dctx_plugin_protocol::records::requests::{CancelReason, Probe, Request, ShutdownReason};
use dctx_plugin_protocol::records::results::{ProbeState, ResultBody};
use dctx_plugin_protocol::records::scalars::{Name, Text};
use dctx_plugin_protocol::{Accepted, DecodeLimits, ErrorCode, MAX_FRAME_CAP, WireMessage};

fn ready(host: &mut Host, init: dctx_plugin_protocol::records::requests::Initialize) {
    let id = host.request(Request::Initialize(Box::new(init)));
    let mut idle = FakeBroker::new(b"");
    assert!(matches!(host.serve_until(&id, &mut idle), Accepted::Result { body: ResultBody::Ready, .. }));
}

fn shutdown(mut host: Host) {
    let id = host.request(Request::Shutdown { reason: ShutdownReason::SessionEnded });
    let mut idle = FakeBroker::new(b"");
    assert!(matches!(host.serve_until(&id, &mut idle), Accepted::Result { body: ResultBody::Closing, .. }));
    assert_eq!(host.finish(), Ok(()));
}

fn observed(accepted: Accepted) -> Observed {
    match accepted {
        Accepted::Result { body: ResultBody::Invoke { status, reason, receipts, diagnostics, command_output }, .. } => {
            assert!(command_output.is_empty(), "hooks return no command output");
            Observed {
                status: Some(status.as_str().to_owned()),
                reason: Some(reason.as_str().to_owned()),
                code: None,
                diagnostics: diagnostics.iter().map(|d| d.code.as_str().to_owned()).collect(),
                receipts: receipts.len(),
            }
        }
        Accepted::Error { error, .. } => {
            Observed { code: Some(error.code.as_str().to_owned()), receipts: error.receipt_ids.len(), ..Observed::default() }
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_parity_corpus_passes_over_the_wire() {
    for case in &cases() {
        let (mut broker, _, invocation) = setup(case);
        let mut init = initialize();
        if case.input.is_none() && !case.raw.get("file_exists").and_then(dctx_plugin_protocol::Json::as_bool).unwrap_or(false) {
            init.resources.retain(|resource| resource.handle.as_str() != "file:ignore");
        }
        if broker.nested.contains_key("file:nested") {
            init.resources.push(dctx_plugin_protocol::records::common::Resource {
                handle: token("file:nested"),
                kind: dctx_plugin_protocol::records::common::ResourceKind::File,
                role: Some(dctx_plugin_protocol::records::scalars::Role::new("nested-ignore").expect("role")),
                bytes: None,
            });
        }
        let mut host = Host::spawn();
        host.negotiate();
        ready(&mut host, init);
        let id = host.request(Request::Invoke(Box::new(invocation)));
        let accepted = host.serve_until(&id, &mut broker);
        check(case, &observed(accepted), &broker);
        shutdown(host);
    }
}

#[test]
fn handshake_matches_the_selected_fixture() {
    let mut host = Host::spawn();
    let answer = host.negotiate();
    let fixture = WireMessage::decode(&bytes("fixtures/valid/hello-selected.json"), DecodeLimits::BOOTSTRAP).expect("fixture");
    assert_eq!(WireMessage::Bootstrap(answer), fixture);
    shutdown_after_ready(host);
}

fn shutdown_after_ready(mut host: Host) {
    ready(&mut host, initialize());
    shutdown(host);
}

#[test]
fn no_common_version_or_missing_required_api_refuses_before_any_session() {
    let mut host = Host::spawn();
    let hello = String::from_utf8(bytes("fixtures/valid/hello.json")).expect("utf8");
    let future = hello.replacen("\"major\": 1", "\"major\": 2", 1);
    let answer = host.hello(future.as_bytes());
    let fixture = WireMessage::decode(&bytes("fixtures/valid/hello-error.json"), DecodeLimits::BOOTSTRAP).expect("fixture");
    assert_eq!(WireMessage::Bootstrap(answer), fixture);
    assert_eq!(host.finish(), Err(WorkerError::Unsupported(ErrorCode::UnsupportedVersion)));

    let mut host = Host::spawn();
    let answer = host.hello(hello.replace("\"file.replace\",", "").as_bytes());
    assert!(matches!(answer, BootstrapMessage::Refused { error, .. } if error.code == ErrorCode::UnsupportedCapability));
    assert_eq!(host.finish(), Err(WorkerError::Unsupported(ErrorCode::UnsupportedCapability)));
}

#[test]
fn lifecycle_violations_end_the_session_without_effects() {
    for body in [
        bytes("fixtures/valid/invoke-before-pack-output-written.json"),
        bytes("fixtures/invalid/wire-before-ready.json"),
        String::from_utf8(bytes("fixtures/valid/initialize.json")).expect("utf8").replace("fixture-key-1", "other-key").into_bytes(),
    ] {
        let mut host = Host::spawn();
        host.negotiate();
        host.send_raw(&body, MAX_FRAME_CAP);
        assert!(matches!(host.finish(), Err(WorkerError::Protocol(_))));
    }
}

#[test]
fn configuration_outside_the_profile_is_refused_at_initialize() {
    let mut host = Host::spawn();
    host.negotiate();
    let mut init = initialize();
    init.config.insert(Name::new("create_missing").expect("name"), ConfigValue::Scalar(Scalar::Bool(true)));
    let id = host.request(Request::Initialize(Box::new(init)));
    let mut idle = FakeBroker::new(b"");
    assert!(matches!(host.serve_until(&id, &mut idle), Accepted::Error { error, .. } if error.code == ErrorCode::InvalidConfig));
    assert_eq!(host.finish(), Err(WorkerError::InitializeRefused(ErrorCode::InvalidConfig)));
}

#[test]
fn probe_is_answered_and_cancel_ends_the_wait_with_a_cancelled_event() {
    let mut host = Host::spawn();
    host.negotiate();
    ready(&mut host, initialize());
    let invoke = host.request(Request::Invoke(Box::new(common::hook("0.26.0", true, true))));
    let (message, _) = host.next();
    let SessionBody::Request { id: pending, request: Request::FileRead(read) } = message.body else { panic!("first broker request") };
    let probe = Probe { invocation: token("op:1"), challenge: token("challenge:1"), last_progress_seq: 0, wait: None };
    let probe_id = host.request(Request::Probe(probe));
    let (message, accepted) = host.next();
    assert!(matches!(&message.body, SessionBody::Response { id, .. } if *id == probe_id));
    let Accepted::Result { body: ResultBody::Probe { state, wait, .. }, .. } = accepted else { panic!("probe result") };
    assert_eq!(state, ProbeState::Waiting);
    assert_eq!(wait, Some(dctx_plugin_protocol::records::common::Wait::BrokerRequest(pending.clone())));
    let cancel = host.request(Request::Cancel { invocation: token("op:1"), reason: CancelReason::User });
    let (_, accepted) = host.next();
    assert!(matches!(accepted, Accepted::Result { body: ResultBody::CancelAccepted, .. }));
    let (message, _) = host.next();
    assert!(matches!(message.body, SessionBody::Event { event: Event::Cancelled { .. }, .. }));
    let (message, accepted) = host.next();
    assert!(matches!(&message.body, SessionBody::Response { id, .. } if *id == invoke));
    assert!(matches!(accepted, Accepted::Error { error, .. } if error.code == ErrorCode::Cancelled));
    let mut broker = FakeBroker::new(b"notes\n");
    let late = broker_answer(&mut broker, &read);
    host.send_raw(&late_response(pending, late), MAX_FRAME_CAP);
    assert_eq!(broker.writes, 0);
    assert_eq!(cancel.as_str(), "h:4");
    shutdown(host);
}

fn broker_answer(broker: &mut FakeBroker, read: &dctx_plugin_protocol::records::broker::ChunkRead) -> ResultBody {
    use dctx_plugin_git::ports::Broker;
    ResultBody::Chunk(broker.file_read(read).expect("read"))
}

#[allow(clippy::needless_pass_by_value, reason = "The fixture builder consumes its typed response body to form one encoded late message.")]
fn late_response(id: dctx_plugin_protocol::records::scalars::RequestId, body: ResultBody) -> Vec<u8> {
    let message = dctx_plugin_protocol::records::envelope::SessionMessage {
        session: token("s:1"),
        extensions: None,
        body: SessionBody::Response { id, outcome: Outcome::Result(dctx_plugin_protocol::PendingResult::from_body(&body)) },
    };
    WireMessage::Session(message).encode()
}

#[test]
fn commands_return_typed_records_and_unknown_verbs_refuse() {
    let mut host = Host::spawn();
    host.negotiate();
    ready(&mut host, initialize());
    let mut status = invocation("fixtures/valid/invoke-command.json");
    let id = host.request(Request::Invoke(Box::new(status.clone())));
    let mut broker = FakeBroker::new(b"notes\n");
    let Accepted::Result { body: ResultBody::Invoke { command_output, .. }, .. } = host.serve_until(&id, &mut broker) else {
        panic!("status result")
    };
    assert_eq!(broker.writes + broker.git_spawns, 0, "status never writes or stages");
    assert!(command_output.iter().any(|record| matches!(record,
        CommandRecord::Record { name, fields } if name.as_str() == "protection"
            && fields.get(&Name::new("protected").expect("name")) == Some(&ConfigValue::Scalar(Scalar::Bool(false))))));
    if let HookPayload::Command { verb, args, .. } = &mut status.payload {
        *verb = Name::new("add").expect("name");
        *args = vec![Text::new(".").expect("text")];
    }
    status.invocation = token("op:2");
    let id = host.request(Request::Invoke(Box::new(status)));
    assert!(matches!(host.serve_until(&id, &mut broker), Accepted::Error { error, .. } if error.code == ErrorCode::InvalidConfig));
    shutdown(host);
}
