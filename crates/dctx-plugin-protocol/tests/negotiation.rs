//! Version negotiation goldens: hello → hello-selected and the incompatible-offer hello-error, byte for byte.
//! Implements W-NEGOTIATE (wire.md §Bootstrap) with refusal paths each next to a positive control.
//! Depends on the corpus fixtures and the crate's negotiate module.

mod common;

use common::*;
use dctx_plugin_protocol::records::bootstrap::{BootstrapMessage, Hello, HelloSelected};
use dctx_plugin_protocol::records::common::VersionRange;
use dctx_plugin_protocol::records::scalars::{Name, RequestId};
use dctx_plugin_protocol::{
    DecodeLimits, ErrorCode, FrameDecoder, Json, Negotiated, SUPPORTED_VERSIONS, WireMessage, WorkerProfile,
    accept_answer, answer_hello, decode_object, to_canonical,
};

fn bootstrap(stem: &str) -> BootstrapMessage {
    match message(stem) {
        WireMessage::Bootstrap(message) => message,
        WireMessage::Session(_) => panic!("{stem} is not bootstrap"),
    }
}

fn hello() -> (RequestId, Hello) {
    match bootstrap("hello") {
        BootstrapMessage::Hello { id, hello } => (id, *hello),
        _ => panic!("hello fixture"),
    }
}

fn selected() -> HelloSelected {
    match bootstrap("hello-selected") {
        BootstrapMessage::Selected { selected, .. } => *selected,
        _ => panic!("hello-selected fixture"),
    }
}

fn profile() -> WorkerProfile {
    let selected = selected();
    WorkerProfile {
        supported: SUPPORTED_VERSIONS.to_vec(),
        identity: selected.identity,
        required_apis: selected.required_apis,
        optional_apis: selected.optional_apis,
    }
}

fn canonical_fixture(stem: &str) -> Vec<u8> {
    to_canonical(&decode_object(&fixture_bytes(stem), DecodeLimits::WIRE).expect("json"))
}

#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
/// # Panics
/// Panics if the checked-in hello/selection fixtures no longer negotiate.
pub fn negotiated() -> Negotiated {
    let (id, hello) = hello();
    accept_answer(&id, &hello, &bootstrap("hello-selected"), &selected().identity, &[]).expect("negotiates")
}

#[test]
fn hello_selected_golden() {
    let (id, hello) = hello();
    let answer = answer_hello(&id, &hello, &profile()).expect("answer");
    assert_eq!(WireMessage::Bootstrap(answer).encode(), canonical_fixture("hello-selected"));
    let negotiated = negotiated();
    let apis: Vec<&str> = negotiated.apis.iter().map(Name::as_str).collect();
    assert_eq!(apis, ["file.read", "file.stage", "file.replace", "program.run"]);
    assert_eq!(negotiated.unavailable, [] as [dctx_plugin_protocol::records::scalars::Name; 0]);
    assert_eq!(negotiated.frame_cap, 1_048_576);
}

#[test]
fn hello_error_golden_for_incompatible_offer() {
    let (id, mut hello) = hello();
    let context = case("hello-error");
    let ranges = context.get("context").and_then(|c| c.get("host_versions")).and_then(Json::as_array).expect("ctx");
    hello.versions = ranges
        .iter()
        .map(|range| {
            let field = |key| range.get(key).and_then(Json::as_int).and_then(|v| u64::try_from(v).ok()).expect("int");
            VersionRange { major: field("major"), min_minor: field("min_minor"), max_minor: field("max_minor") }
        })
        .collect();
    let answer = answer_hello(&id, &hello, &profile()).expect("answer");
    assert_eq!(WireMessage::Bootstrap(answer.clone()).encode(), canonical_fixture("hello-error"));
    let refusal = accept_answer(&id, &hello, &answer, &selected().identity, &[]).unwrap_err();
    assert_eq!((refusal.code, refusal.reason), (ErrorCode::UnsupportedVersion, "worker-refused"));
}

#[test]
fn frame_hello_carries_the_hello_fixture() {
    let descriptor = document("fixtures/valid/frame-hello.json");
    let mut decoder = FrameDecoder::new();
    let bodies = decoder.push_all(&unhex(common::text(&descriptor, "bytes"))).expect("frame");
    let framed = decode_object(&bodies[0], DecodeLimits::BOOTSTRAP).expect("json");
    let fixture = decode_object(&fixture_bytes("hello"), DecodeLimits::WIRE).expect("json");
    assert!(framed.same_value(&fixture));
}

#[test]
fn acceptance_refusals_have_positive_controls() {
    let (id, hello) = hello();
    let answer = bootstrap("hello-selected");
    let install = selected().identity;
    assert!(accept_answer(&id, &hello, &answer, &install, &[]).is_ok());

    let mut other = install.clone();
    other.publisher_key = dctx_plugin_protocol::records::scalars::Token::new("other-key").expect("token");
    let refusal = accept_answer(&id, &hello, &answer, &other, &[]).unwrap_err();
    assert_eq!(refusal.code, ErrorCode::IntegrityFailure);

    let mut narrow = hello.clone();
    narrow.apis.retain(|api| api.as_str() != "file.stage");
    let refusal = accept_answer(&id, &narrow, &answer, &install, &[]).unwrap_err();
    assert_eq!(refusal.code, ErrorCode::UnsupportedCapability);

    let mut no_program = hello.clone();
    no_program.apis.retain(|api| api.as_str() != "program.run");
    let degraded = accept_answer(&id, &no_program, &answer, &install, &[]).expect("optional api may be absent");
    assert_eq!(degraded.unavailable.iter().map(Name::as_str).collect::<Vec<_>>(), ["program.run"]);

    let ext = Name::new("fixture-ext").expect("name");
    let mut with_ext = selected();
    with_ext.extensions = vec![ext.clone()];
    let answer_ext = BootstrapMessage::Selected { id: id.clone(), selected: Box::new(with_ext) };
    let mut offered = hello.clone();
    offered.extensions = vec![ext.clone()];
    let refusal = accept_answer(&id, &offered, &answer_ext, &install, &[]).unwrap_err();
    assert_eq!(refusal.reason, "unnegotiated-extension");
    assert!(accept_answer(&id, &offered, &answer_ext, &install, std::slice::from_ref(&ext)).is_ok());
    assert!(accept_answer(&id, &hello, &answer_ext, &install, std::slice::from_ref(&ext)).is_err());

    let wrong = RequestId::new("h:other").expect("id");
    assert_eq!(accept_answer(&wrong, &hello, &answer, &install, &[]).unwrap_err().reason, "unknown-response");
}
