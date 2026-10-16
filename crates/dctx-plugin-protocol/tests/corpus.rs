//! Catalog-driven conformance for every frame, decode, schema and semantic wire case of the PL-C corpus.
//! Implements fixtures/README.md: frame descriptors decoded from hex, bodies framed as exact bytes, expected
//! stage/code/predicate compared, valid fixtures round-tripped canonically. Session-stage cases are listed
//! here and exercised in tests/session.rs and tests/negotiation.rs; an unknown stage fails the test.

mod common;

use common::*;
use dctx_plugin_protocol::records::method::Method;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::session::check_result;
use dctx_plugin_protocol::{
    DecodeLimits, ErrorCode, FrameDecoder, Json, Outcome, Refusal, SessionBody, WireMessage, decode_object,
    encode_frame, to_canonical,
};

const WIRE: &str = "urn:dctx:plugin-api:v1:wire";

/// Session-stage wire cases; each is exercised by name in tests/session.rs or tests/negotiation.rs.
const SESSION_CASES: [&str; 14] = [
    "hello",
    "hello-selected",
    "hello-error",
    "initialize",
    "wire-wrong-session",
    "wire-wrong-id-direction",
    "wire-before-ready",
    "wire-unnegotiated-extension",
    "wire-response-wrong-method",
    "wire-unknown-response",
    "wire-duplicate-response",
    "wire-progress-regression",
    "wire-progress-unknown-wait",
    "wire-second-invoke",
];

fn frame_outcome(descriptor: &Json) -> Result<WireMessage, Refusal> {
    assert_eq!(text(descriptor, "encoding"), "hex");
    assert_eq!(descriptor.get("negotiated"), Some(&Json::Bool(false)));
    let mut decoder = FrameDecoder::new();
    let bodies = decoder.push_all(&unhex(text(descriptor, "bytes")))?;
    decoder.finish()?;
    assert_eq!(bodies.len(), 1, "one frame per descriptor");
    WireMessage::decode(&bodies[0], DecodeLimits::BOOTSTRAP)
}

fn request_for(method: Method, hook: &str) -> Request {
    let stem = match method {
        Method::Initialize => "initialize".to_owned(),
        Method::Invoke if hook == "command" => "invoke-command".to_owned(),
        Method::Invoke => "invoke-before-pack-output-written".to_owned(),
        other => format!("request-{}", other.as_str().replace('.', "-")),
    };
    match session_message(&stem).body {
        SessionBody::Request { request, .. } => request,
        _ => panic!("{stem} is not a request"),
    }
}

/// Decodes a body; when the case names a pending method, resolves and checks the result against it.
fn body_outcome(case: &Json, bytes: &[u8]) -> Result<WireMessage, Refusal> {
    let message = WireMessage::decode(bytes, DecodeLimits::WIRE)?;
    let pending = case.get("context").map(|context| text(context, "pending_method")).unwrap_or("");
    let hook = case.get("context").map(|context| text(context, "hook")).unwrap_or("");
    if let (Some(method), WireMessage::Session(session)) = (Method::parse(pending), &message) {
        if let SessionBody::Response { outcome: Outcome::Result(result), .. } = &session.body {
            let body = result.resolve(method)?;
            check_result(&request_for(method, hook), &body)?;
            assert!(body.encode().same_value(result.as_json()), "typed result re-encodes exactly");
        }
    }
    Ok(message)
}

fn assert_expectation(id: &str, case: &Json, outcome: Result<WireMessage, Refusal>) {
    let valid = expect_field(case, "valid") == Some(&Json::Bool(true));
    match outcome {
        Ok(_) if valid => {}
        Ok(_) => panic!("{id}: accepted but expected refusal"),
        Err(refusal) if valid => panic!("{id}: refused {refusal}"),
        Err(refusal) => {
            let expect = |key| expect_field(case, key).and_then(Json::as_str).unwrap_or("");
            assert_eq!(refusal.code.as_str(), expect("code"), "{id}: {refusal}");
            assert_eq!(refusal.stage.as_str(), expect("stage"), "{id}: {refusal}");
            assert_eq!(refusal.predicate.as_str(), expect("predicate"), "{id}: {refusal}");
        }
    }
}

#[test]
fn every_wire_catalog_case_meets_its_expectation() {
    let mut checked = 0;
    let mut session_cases = Vec::new();
    for case in catalog() {
        let id = text(&case, "id").to_owned();
        let stage = expect_field(&case, "stage").and_then(Json::as_str).unwrap_or("").to_owned();
        let is_wire = text(&case, "schema") == WIRE;
        let outcome = match stage.as_str() {
            "frame" => frame_outcome(&document(text(&case, "path"))),
            "decode" | "schema" | "semantic" if is_wire => body_outcome(&case, &case_bytes(&id)),
            "session" if is_wire => {
                session_cases.push(id);
                continue;
            }
            "session" | "decode" | "schema" | "semantic" | "artifact" | "dts-source" | "scenario" => continue,
            other => panic!("{id}: unknown stage {other}"),
        };
        assert_expectation(&id, &case, outcome);
        checked += 1;
    }
    session_cases.sort();
    let mut expected: Vec<String> = SESSION_CASES.iter().map(|id| (*id).to_owned()).collect();
    expected.sort();
    assert_eq!(session_cases, expected, "session cases must all be covered by name");
    assert!(checked >= 60, "checked {checked}");
}

#[test]
fn decode_stage_bodies_also_fail_through_the_stream_codec() {
    for case in catalog().iter().filter(|case| expect_field(case, "stage").and_then(Json::as_str) == Some("decode")) {
        let body = case_bytes(text(case, "id"));
        let frame = encode_frame(&body, dctx_plugin_protocol::MAX_FRAME_CAP).expect("frames");
        let mut decoder = FrameDecoder::new();
        decoder.set_cap(dctx_plugin_protocol::MAX_FRAME_CAP).expect("cap");
        let bodies = decoder.push_all(&frame).expect("framing is valid");
        let refusal = WireMessage::decode(&bodies[0], DecodeLimits::WIRE).unwrap_err();
        assert_eq!(Some(refusal.code.as_str()), expect_field(case, "code").and_then(Json::as_str));
    }
}

#[test]
fn depth_and_member_fixtures_are_refused_at_decode() {
    for (path, reason) in [("fixtures/invalid/json-depth.txt", "depth-limit"), ("fixtures/invalid/json-members.txt", "member-limit")] {
        let refusal = WireMessage::decode(&bytes(path), DecodeLimits::WIRE).unwrap_err();
        assert_eq!((refusal.stage.as_str(), refusal.reason), ("decode", reason), "{path}");
    }
}

fn wire_fixture_stems() -> Vec<String> {
    let dir = api_dir().join("fixtures/valid");
    let prefixes = ["error-", "event-", "result-", "request-", "invoke-", "hello", "initialize", "command-result", "file-"];
    let mut stems: Vec<String> = std::fs::read_dir(dir)
        .expect("valid dir")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
        .filter(|stem| prefixes.iter().any(|prefix| stem.starts_with(prefix)))
        .collect();
    stems.sort();
    stems
}

#[test]
fn every_valid_wire_fixture_round_trips_canonically() {
    let stems = wire_fixture_stems();
    assert!(stems.len() >= 60, "{} fixtures", stems.len());
    for stem in &stems {
        let original = decode_object(&fixture_bytes(stem), DecodeLimits::WIRE).expect("json");
        let message = WireMessage::from_json(&original).unwrap_or_else(|refusal| panic!("{stem}: {refusal}"));
        assert_eq!(message.encode(), to_canonical(&original), "{stem}");
        assert_eq!(WireMessage::decode(&message.encode(), DecodeLimits::WIRE).as_ref(), Ok(&message), "{stem}");
    }
}

#[test]
fn all_eighteen_error_codes_have_round_tripping_fixtures() {
    let mut codes: Vec<ErrorCode> = Vec::new();
    for stem in wire_fixture_stems().iter().filter(|stem| stem.starts_with("error-code-")) {
        if let SessionBody::Response { outcome: Outcome::Error(error), .. } = session_message(stem).body {
            assert!(!error.retryable);
            codes.push(error.code);
        }
    }
    codes.sort();
    codes.dedup();
    assert_eq!(codes.len(), ErrorCode::ALL.len());
}
