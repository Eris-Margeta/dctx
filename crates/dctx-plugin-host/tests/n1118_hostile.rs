// SPDX-License-Identifier: Apache-2.0

mod common;

use dctx_plugin_host::fakes::{FakeAudit, FakeClock, FakeFiles, FakeProcess};
use dctx_plugin_host::ports::Clock;
use dctx_plugin_host::{
    HostSession, LaunchFacts, Origin, ProcessEvidence, Reply, StopReason, Supervisor, Verdict,
    WorkerPhase,
};
use dctx_plugin_protocol::records::broker::FileStage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::capability::Liveness;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::scalars::RequestId;
use dctx_plugin_protocol::{
    DecodeLimits, ErrorCode, FrameReader, MAX_FRAME_CAP, Phase, Predicate, Refusal, Stage,
    StreamError, WireMessage, encode_frame,
};

const SAMPLE_MS: u64 = 100;
const GRACE_MS: u64 = 50;
const INVOKE_RESULT: &[u8] =
    include_bytes!("../../../docs/design/plugin-api/fixtures/valid/result-invoke.json");

type TestSession = HostSession<FakeAudit, FakeFiles, FakeProcess, FakeProcess>;

fn running(clock: &FakeClock) -> TestSession {
    let facts = LaunchFacts {
        origin: Origin::BundledGit,
        verified: true,
        compatible: true,
        confined: false,
    };
    let policy = Liveness::UnattendedFailClosed {
        sample_ms: SAMPLE_MS,
        cancel_grace_ms: GRACE_MS,
    };
    let mut supervisor = Supervisor::new(facts, policy, FakeProcess::default());
    supervisor.launch().unwrap();
    supervisor.negotiated().unwrap();
    supervisor.initialized().unwrap();
    supervisor.invoke(clock.now()).unwrap();
    HostSession::new(common::broker(), supervisor, 16)
}

fn tick(session: &mut TestSession, clock: &FakeClock, elapsed: u64) -> Verdict {
    clock.advance(elapsed);
    session.tick(clock.now())
}

#[test]
fn silent_worker_times_out_at_policy_boundaries() {
    let clock = FakeClock::default();
    let mut session = running(&clock);

    assert_eq!(tick(&mut session, &clock, SAMPLE_MS - 1), Verdict::Continue);
    assert_eq!(tick(&mut session, &clock, 1), Verdict::SendProbe);
    assert_eq!(tick(&mut session, &clock, SAMPLE_MS - 1), Verdict::Continue);
    assert!(session.supervisor().admits());
    assert_eq!(
        tick(&mut session, &clock, 1),
        Verdict::Cancel(StopReason::Stalled)
    );
    assert!(!session.supervisor().admits());

    assert_eq!(tick(&mut session, &clock, GRACE_MS - 1), Verdict::Continue);
    assert_eq!(session.supervisor().process().forced, 0);
    assert_eq!(tick(&mut session, &clock, 1), Verdict::Continue);
    assert_eq!(session.supervisor().process().forced, 1);
    assert_eq!(session.supervisor().process().reaped, 1);
    assert_eq!(
        session.supervisor().worker().phase(),
        WorkerPhase::Reaped(StopReason::Stalled)
    );
}

#[test]
fn mid_response_crash_is_typed_and_a_replacement_worker_can_serve_the_next_call() {
    let clock = FakeClock::default();
    let mut crashed = running(&clock);
    let frame = encode_frame(INVOKE_RESULT, MAX_FRAME_CAP).unwrap();

    let handle = common::file_handle(crashed.broker_mut());
    let id = RequestId::new("p:read").unwrap();
    crashed
        .receive(
            id.clone(),
            common::read_req(&handle, 0, 3, None),
            clock.now(),
        )
        .unwrap();
    assert_eq!(crashed.queued(), 1);

    crashed.supervisor_mut().process_mut().evidence = ProcessEvidence::Exited;
    let mut interrupted = FrameReader::new(&frame[..frame.len() - 1]);
    assert_eq!(
        interrupted.next_frame(),
        Err(StreamError::Protocol(Refusal::new(
            ErrorCode::InvalidMessage,
            Stage::Frame,
            Predicate::WFrame,
            "truncated-body"
        )))
    );
    assert_eq!(
        tick(&mut crashed, &clock, SAMPLE_MS),
        Verdict::Cancel(StopReason::Stalled)
    );
    assert_eq!(crashed.queued(), 0);
    assert!(crashed.serve_next(clock.now()).is_none());
    let refusal = crashed
        .receive(
            id.clone(),
            common::read_req(&handle, 0, 3, None),
            clock.now(),
        )
        .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.phase, refusal.reason),
        (ErrorCode::Cancelled, Phase::Broker, "admission-closed")
    );
    assert_eq!(tick(&mut crashed, &clock, GRACE_MS), Verdict::Continue);
    assert_eq!(crashed.supervisor().process().reaped, 1);
    assert_eq!(
        crashed.supervisor().worker().phase(),
        WorkerPhase::Reaped(StopReason::Stalled)
    );

    let mut next = running(&clock);
    let handle = common::file_handle(next.broker_mut());
    let mut response = FrameReader::new(frame.as_slice());
    next.receive(
        id.clone(),
        common::read_req(&handle, 0, 3, None),
        clock.now(),
    )
    .unwrap();
    assert!(matches!(
        next.serve_next(clock.now()),
        Some((served, Ok(Reply::Chunk { bytes, .. }))) if served == id && bytes == b"abc"
    ));
    let body = response.next_frame().unwrap().unwrap();
    let expected = WireMessage::decode(INVOKE_RESULT, DecodeLimits::WIRE).unwrap();
    assert_eq!(WireMessage::decode(&body, DecodeLimits::WIRE), Ok(expected));
    assert_eq!(response.next_frame(), Ok(None));
    next.supervisor_mut().finish().unwrap();
    assert_eq!(next.supervisor().worker().phase(), WorkerPhase::Ready);
}

#[test]
fn worker_response_over_the_negotiated_cap_is_refused() {
    const CAP: u32 = 512;
    let expected = WireMessage::decode(INVOKE_RESULT, DecodeLimits::WIRE).unwrap();
    for len in [CAP, CAP + 1] {
        let mut body = expected.encode();
        assert!(body.len() < CAP as usize);
        body.resize(len as usize, b' ');
        let frame = encode_frame(&body, MAX_FRAME_CAP).unwrap();
        let mut worker_output = FrameReader::new(frame.as_slice());
        worker_output.decoder_mut().set_cap(CAP).unwrap();

        if len == CAP {
            let body = worker_output.next_frame().unwrap().unwrap();
            assert_eq!(
                WireMessage::decode(&body, DecodeLimits::WIRE),
                Ok(expected.clone())
            );
        } else {
            let Err(StreamError::Protocol(refusal)) = worker_output.next_frame() else {
                panic!("oversized worker response was accepted");
            };
            assert_eq!(
                (refusal.code, refusal.stage, refusal.reason),
                (ErrorCode::ResourceLimit, Stage::Frame, "frame-too-large")
            );
        }
    }
}

#[test]
fn upload_refuses_gaps_duplicate_chunks_missing_final_and_data_after_final() {
    let mut broker = common::broker();
    let handle = common::file_handle(&mut broker);
    let precondition = broker.issue_precondition(&handle).unwrap();
    let stage = |upload, offset, bytes, final_chunk| {
        Request::FileStage(FileStage {
            resource: handle.clone(),
            precondition: precondition.clone(),
            upload,
            offset,
            bytes: Chunk::new(bytes).unwrap(),
            final_chunk,
        })
    };
    let Reply::Staged {
        upload,
        sealed: false,
    } = broker.handle(&stage(None, 0, b"a", false)).unwrap()
    else {
        panic!("expected unsealed upload");
    };

    let replace = common::replace_req(&handle, &precondition, &upload, "a:hostile");
    let refusal = broker.handle(&replace).unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::Conflict, "upload-not-sealed")
    );

    for (offset, bytes) in [(2, &b"c"[..]), (0, &b"a"[..])] {
        let refusal = broker
            .handle(&stage(Some(upload.clone()), offset, bytes, false))
            .unwrap_err();
        assert_eq!(
            (refusal.code, refusal.reason),
            (ErrorCode::Conflict, "upload-not-contiguous")
        );
    }

    assert_eq!(
        broker.handle(&stage(Some(upload.clone()), 1, b"b", true)),
        Ok(Reply::Staged {
            upload: upload.clone(),
            sealed: true
        })
    );
    let refusal = broker
        .handle(&stage(Some(upload), 2, b"c", false))
        .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::Conflict, "upload-not-contiguous")
    );
    assert!(matches!(
        broker.handle(&replace),
        Ok(Reply::Receipt {
            replayed: false,
            ..
        })
    ));
    assert_eq!(
        broker.files_mut().content(&common::path()),
        Some(&b"ab"[..])
    );
}

#[test]
fn interleaved_upload_ids_cannot_supply_chunks_for_another_resource() {
    let mut broker = common::broker();
    let [first, second] = std::array::from_fn(|_| {
        let resource = common::file_handle(&mut broker);
        let precondition = broker.issue_precondition(&resource).unwrap();
        let start = FileStage {
            resource,
            precondition,
            upload: None,
            offset: 0,
            bytes: Chunk::new(b"a").unwrap(),
            final_chunk: false,
        };
        let Reply::Staged {
            upload,
            sealed: false,
        } = broker.handle(&Request::FileStage(start.clone())).unwrap()
        else {
            panic!("expected unsealed upload");
        };
        FileStage {
            upload: Some(upload),
            offset: 1,
            bytes: Chunk::new(b"b").unwrap(),
            final_chunk: true,
            ..start
        }
    });
    let mismatched = FileStage {
        upload: first.upload.clone(),
        ..second.clone()
    };
    let refusal = broker.handle(&Request::FileStage(mismatched)).unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::Conflict, "upload-bound-elsewhere")
    );
    for stage in [second, first] {
        let upload = stage.upload.clone().unwrap();
        assert_eq!(
            broker.handle(&Request::FileStage(stage)),
            Ok(Reply::Staged {
                upload,
                sealed: true
            })
        );
    }
}
