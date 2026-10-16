//! Session-stage corpus cases plus replay, out-of-order and refusal positive controls.
//! Implements W-STATE, W-BINDING, W-RESULT, W-PROGRESS, W-LIMITS (wire.md §Bootstrap and lifecycle) by
//! driving a Session with the real fixture messages; each catalog context is injected by prior messages.
//! Depends on the corpus fixtures and the crate's negotiate and session modules.

mod common;

use common::*;
use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
use dctx_plugin_protocol::records::bytes::Chunk;
use dctx_plugin_protocol::records::common::Wait;
use dctx_plugin_protocol::records::events::Event;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::results::{ChunkResult, ResultBody};
use dctx_plugin_protocol::records::scalars::{RequestId, Sender, Token};
use dctx_plugin_protocol::{
    Accepted, ErrorCode, Outcome, PendingResult, Predicate, Refusal, Session, SessionBody, SessionMessage,
    SessionState, Stage, WireMessage, accept_answer,
};

const HOST: Sender = Sender::Host;
const PLUGIN: Sender = Sender::Plugin;

fn session() -> Session {
    let (WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }), WireMessage::Bootstrap(answer)) =
        (message("hello"), message("hello-selected"))
    else {
        panic!("bootstrap fixtures");
    };
    let BootstrapMessage::Selected { selected, .. } = &answer else { panic!("selected") };
    let negotiated = accept_answer(&id, &hello, &answer, &selected.identity, &[]).expect("negotiated");
    Session::new(Token::new("s:1").expect("token"), negotiated)
}

fn ok(session: &mut Session, sender: Sender, message: &SessionMessage) -> Accepted {
    session.observe(sender, message).unwrap_or_else(|refusal| panic!("refused: {refusal}"))
}

fn refused(session: &mut Session, sender: Sender, message: &SessionMessage) -> Refusal {
    let before = session.state();
    let refusal = session.observe(sender, message).expect_err("must refuse");
    assert_eq!(session.state(), before, "a refusal leaves state unchanged");
    refusal
}

fn ready() -> Session {
    let mut session = session();
    assert_eq!(ok(&mut session, HOST, &session_message("initialize")), Accepted::Request(dctx_plugin_protocol::records::method::Method::Initialize));
    assert_eq!(session.state(), SessionState::Initializing);
    ok(&mut session, PLUGIN, &session_message("result-initialize"));
    assert_eq!(session.state(), SessionState::Ready);
    session
}

fn active() -> Session {
    let mut session = ready();
    ok(&mut session, HOST, &session_message("invoke-before-pack-output-written"));
    assert_eq!(session.state(), SessionState::Active);
    session
}

fn expect_case(id: &str, refusal: Refusal) {
    let case = case(id);
    let expect = |key| expect_field(&case, key).and_then(|v| v.as_str()).unwrap_or("").to_owned();
    assert_eq!(refusal.code.as_str(), expect("code"), "{id}: {refusal}");
    assert_eq!(refusal.stage.as_str(), expect("stage"), "{id}: {refusal}");
    assert_eq!(refusal.predicate.as_str(), expect("predicate"), "{id}: {refusal}");
}

fn result(id: &str, body: &ResultBody) -> SessionMessage {
    SessionMessage {
        session: Token::new("s:1").expect("token"),
        extensions: None,
        body: SessionBody::Response {
            id: RequestId::new(id).expect("id"),
            outcome: Outcome::Result(PendingResult::from_body(body)),
        },
    }
}

#[test]
fn initialize_and_before_ready_cases() {
    let mut session = session();
    let early = refused(&mut session, PLUGIN, &session_message("request-file-read"));
    assert_eq!(early.predicate, Predicate::WState);
    ok(&mut session, HOST, &session_message("initialize"));
    expect_case("wire-before-ready", refused(&mut session, PLUGIN, &session_message("wire-before-ready")));
    ok(&mut session, PLUGIN, &session_message("result-initialize"));
    let again = with_id(session_message("initialize"), "h:again");
    assert_eq!(refused(&mut session, HOST, &again).predicate, Predicate::WState);
    ok(&mut session, HOST, &session_message("invoke-before-pack-output-written"));
    ok(&mut session, PLUGIN, &session_message("request-file-read"));
}

#[test]
fn binding_cases_have_positive_controls() {
    let mut session = active();
    expect_case("wire-wrong-session", refused(&mut session, PLUGIN, &session_message("wire-wrong-session")));
    expect_case("wire-wrong-id-direction", refused(&mut session, PLUGIN, &session_message("wire-wrong-id-direction")));
    let extension = session_message("wire-unnegotiated-extension");
    expect_case("wire-unnegotiated-extension", refused(&mut session, PLUGIN, &extension));
    ok(&mut session, PLUGIN, &session_message("request-file-read"));
    let wrong_type = session_message("wire-response-wrong-method");
    expect_case("wire-response-wrong-method", refused(&mut session, HOST, &wrong_type));
    expect_case("wire-unknown-response", refused(&mut session, HOST, &session_message("wire-unknown-response")));
    let eof = refused(&mut session, HOST, &session_message("wire-chunk-eof"));
    assert_eq!((eof.stage, eof.predicate), (Stage::Semantic, Predicate::WBytes));
    let from_plugin = refused(&mut session, PLUGIN, &session_message("result-file-read"));
    assert_eq!(from_plugin.reason, "wrong-id-direction");
    assert!(matches!(ok(&mut session, HOST, &session_message("result-file-read")), Accepted::Result { .. }));
    expect_case("wire-duplicate-response", refused(&mut session, HOST, &session_message("wire-duplicate-response")));
}

#[test]
fn replayed_request_ids_are_refused_even_after_completion() {
    let mut session = active();
    let read = session_message("request-file-read");
    ok(&mut session, PLUGIN, &read);
    assert_eq!(refused(&mut session, PLUGIN, &read).reason, "duplicate-request-id");
    ok(&mut session, HOST, &session_message("result-file-read"));
    assert_eq!(refused(&mut session, PLUGIN, &read).reason, "duplicate-request-id");
    ok(&mut session, PLUGIN, &session_message("file-read-continuation"));
    let host_reuse = with_id(session_message("request-probe"), "h:invoke");
    assert_eq!(refused(&mut session, HOST, &host_reuse).reason, "duplicate-request-id");
}

#[test]
fn out_of_order_responses_complete_exactly_once() {
    let mut session = active();
    ok(&mut session, PLUGIN, &session_message("request-file-read"));
    ok(&mut session, PLUGIN, &session_message("file-read-continuation"));
    assert_eq!(session.outstanding(PLUGIN), 2);
    let second = ResultBody::Chunk(ChunkResult {
        bytes: Chunk::new(b"bc").expect("chunk"),
        offset: 1,
        size: 3,
        eof: true,
        precondition: Some(Token::new("pre:1").expect("token")),
        receipt: Token::new("receipt:read2").expect("token"),
    });
    ok(&mut session, HOST, &result("p:2", &second));
    assert_eq!(refused(&mut session, HOST, &result("p:2", &second)).reason, "duplicate-response");
    ok(&mut session, HOST, &session_message("result-file-read"));
    assert_eq!(session.outstanding(PLUGIN), 0);
    assert_eq!(refused(&mut session, HOST, &session_message("result-file-read")).reason, "duplicate-response");
}

#[test]
fn progress_cases_with_positive_controls() {
    let mut session = active();
    expect_case("wire-progress-unknown-wait", refused(&mut session, PLUGIN, &session_message("wire-progress-unknown-wait")));
    ok(&mut session, PLUGIN, &session_message("event-progress"));
    let regression = session_message("wire-progress-regression");
    expect_case("wire-progress-regression", refused(&mut session, PLUGIN, &regression));
    let mut advancing = regression.clone();
    if let SessionBody::Event { event: Event::Progress(progress), .. } = &mut advancing.body {
        progress.completed = 3;
    }
    ok(&mut session, PLUGIN, &advancing);
    assert_eq!(refused(&mut session, PLUGIN, &advancing).reason, "event-sequence");
    ok(&mut session, PLUGIN, &session_message("request-file-read"));
    let mut waiting = session_message("wire-progress-unknown-wait");
    if let SessionBody::Event { seq, event: Event::Progress(progress) } = &mut waiting.body {
        *seq = 3;
        progress.seq = 3;
        progress.phase = dctx_plugin_protocol::records::scalars::Name::new("stage").expect("name");
        progress.wait = Some(Wait::BrokerRequest(RequestId::new("p:1").expect("id")));
    }
    ok(&mut session, PLUGIN, &waiting);
    let host_event = refused(&mut session, HOST, &session_message("event-cancelled"));
    assert_eq!(host_event.reason, "host-event");
}

#[test]
fn second_invoke_is_busy_until_the_first_completes() {
    let mut session = ready();
    let mut first = with_id(session_message("invoke-before-pack-output-written"), "h:first");
    if let SessionBody::Request { request: Request::Invoke(invocation), .. } = &mut first.body {
        invocation.invocation = Token::new("op:previous").expect("token");
    }
    ok(&mut session, HOST, &first);
    assert_eq!(session.active_invocation().map(Token::as_str), Some("op:previous"));
    let second = session_message("wire-second-invoke");
    expect_case("wire-second-invoke", refused(&mut session, HOST, &second));
    ok(&mut session, PLUGIN, &with_id(session_message("result-invoke"), "h:first"));
    assert_eq!(session.state(), SessionState::Ready);
    ok(&mut session, HOST, &second);
}

#[test]
fn broker_refusals_after_cancel_and_at_the_outstanding_limit() {
    let mut session = active();
    for index in 1..=16 {
        ok(&mut session, PLUGIN, &with_id(session_message("request-file-read"), &format!("p:{index}")));
    }
    let over = refused(&mut session, PLUGIN, &with_id(session_message("request-file-read"), "p:17"));
    assert_eq!((over.code, over.predicate), (ErrorCode::ResourceLimit, Predicate::WLimits));
    let state_read = refused(&mut session, PLUGIN, &with_id(session_message("request-state-read"), "p:18"));
    assert_eq!(state_read.code, ErrorCode::UnsupportedCapability);
    let unsolicited = refused(&mut session, PLUGIN, &session_message("event-cancelled"));
    assert_eq!(unsolicited.reason, "unsolicited-event");
    ok(&mut session, HOST, &with_id(session_message("request-cancel"), "h:cancel"));
    assert_eq!(session.state(), SessionState::Cancelling);
    let after = refused(&mut session, PLUGIN, &with_id(session_message("request-file-read"), "p:19"));
    assert_eq!(after.code, ErrorCode::Cancelled);
    ok(&mut session, PLUGIN, &with_id(session_message("result-cancel"), "h:cancel"));
    ok(&mut session, PLUGIN, &session_message("event-cancelled"));
    let mut twice = session_message("event-cancelled");
    if let SessionBody::Event { seq, .. } = &mut twice.body {
        *seq = 9;
    }
    assert_eq!(refused(&mut session, PLUGIN, &twice).reason, "duplicate-cancelled");
    ok(&mut session, PLUGIN, &with_id(session_message("error-code-cancelled"), "h:invoke"));
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.outstanding(PLUGIN), 0, "broker requests do not survive the invocation");
    assert_eq!(refused(&mut session, HOST, &session_message("result-file-read")).reason, "duplicate-response");
}

#[test]
fn probe_echo_and_hook_output_rules() {
    let mut session = active();
    ok(&mut session, HOST, &with_id(session_message("request-probe"), "h:probe"));
    let mut wrong = with_id(session_message("result-probe"), "h:probe");
    if let SessionBody::Response { outcome: Outcome::Result(result), .. } = &mut wrong.body {
        let mut body = result.resolve(dctx_plugin_protocol::records::method::Method::Probe).expect("probe");
        if let ResultBody::Probe { challenge, .. } = &mut body {
            *challenge = Token::new("challenge:2").expect("token");
        }
        *result = PendingResult::from_body(&body);
    }
    assert_eq!(refused(&mut session, PLUGIN, &wrong).reason, "probe-echo-mismatch");
    ok(&mut session, PLUGIN, &with_id(session_message("result-probe"), "h:probe"));
    let output = refused(&mut session, PLUGIN, &with_id(session_message("command-result"), "h:invoke"));
    assert_eq!(output.predicate, Predicate::SResult);
    ok(&mut session, PLUGIN, &with_id(session_message("invoke-result-warning"), "h:invoke"));
    assert_eq!(session.state(), SessionState::Ready);
}

#[test]
fn shutdown_and_failed_initialize_end_the_session() {
    let mut session = ready();
    let shutdown = with_id(session_message("request-shutdown"), "h:shutdown");
    ok(&mut session, HOST, &shutdown);
    ok(&mut session, PLUGIN, &with_id(session_message("result-shutdown"), "h:shutdown"));
    assert_eq!(session.state(), SessionState::Closed);
    assert_eq!(refused(&mut session, HOST, &session_message("request-probe")).reason, "session-ended");

    let mut failing = session_message("initialize");
    let mut session = self::session();
    ok(&mut session, HOST, &failing);
    let error = with_id(session_message("error-code-invalid-config"), "h:1");
    assert!(matches!(ok(&mut session, PLUGIN, &error), Accepted::Error { .. }));
    assert_eq!(session.state(), SessionState::Failed);
    failing = with_id(failing, "h:retry");
    assert_eq!(refused(&mut session, HOST, &failing).reason, "session-ended");
}
