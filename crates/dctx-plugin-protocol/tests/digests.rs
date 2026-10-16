//! Canonical-byte digest goldens recomputed with the public dctx_hash::Sha256 API (one-shot and incremental).
//! Implements wire.md §Bytes canonical inputs and S-REPLAY argument digests against fixtures/digests.json.
//! Depends on dctx-hash (dev-only) and the corpus; the protocol crate itself stays std-only.

mod common;

use common::*;
use dctx_hash::Sha256;
use dctx_plugin_protocol::records::method::Method;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::{Json, SessionBody, to_canonical};

fn sha256(bytes: &[u8]) -> (String, u64) {
    let digest = Sha256::digest(bytes).expect("digest");
    let mut incremental = Sha256::new();
    for byte in bytes {
        incremental.update(std::slice::from_ref(byte)).expect("update");
    }
    let streamed = incremental.finalize();
    assert_eq!(streamed.as_bytes(), digest.as_bytes(), "incremental equals one-shot");
    assert_eq!(streamed.byte_length(), digest.byte_length());
    (hex(digest.as_bytes()), digest.byte_length())
}

fn digests() -> Json {
    document("fixtures/digests.json")
}

fn expected(entry: &Json) -> (String, u64) {
    let bytes = entry.get("bytes").and_then(Json::as_int).and_then(|n| u64::try_from(n).ok()).expect("bytes");
    (text(entry, "sha256").to_owned(), bytes)
}

#[test]
fn action_argument_digest_excludes_action_id() {
    let fixture = digests();
    let argument = fixture.get("argument_fixture").expect("argument fixture");
    assert_eq!(text(argument, "encoding"), "canonical-json");
    let SessionBody::Request { request, .. } = session_message("request-file-replace").body else {
        panic!("request");
    };
    assert_eq!(request.method(), Method::FileReplace);
    let (action, input) = request.action_input().expect("replay key");
    assert_eq!(action.as_str(), "action:replace");
    let preimage = Json::Object(vec![
        ("method".to_owned(), argument.get("method").cloned().expect("method")),
        ("params".to_owned(), argument.get("params").cloned().expect("params")),
    ]);
    assert_eq!(input, to_canonical(&preimage));
    assert_eq!(sha256(&input), expected(argument));
    let changed = Request::decode(
        Method::FileReplace,
        &dctx_plugin_protocol::decode_object(
            br#"{"resource":"file:ignore","precondition":"pre:2","upload":"upload:1","action_id":"action:replace"}"#,
            dctx_plugin_protocol::DecodeLimits::WIRE,
        )
        .expect("json"),
    )
    .expect("request");
    assert_ne!(changed.action_input().expect("key").1, input, "different arguments, different digest");
}

#[test]
fn plan_digest_is_the_canonical_compiled_plan() {
    let fixture = digests();
    let entries = fixture.get("entries").and_then(Json::as_array).expect("entries");
    let plan = entries.iter().find(|e| text(e, "path") == "fixtures/valid/dts-plan.json").expect("plan entry");
    assert_eq!(text(plan, "encoding"), "canonical-json");
    let canonical = to_canonical(&document("fixtures/valid/dts-plan.json"));
    let (hex_digest, length) = sha256(&canonical);
    assert_eq!((hex_digest.clone(), length), expected(plan));
    let SessionBody::Request { request: Request::Initialize(init), .. } = session_message("initialize").body else {
        panic!("initialize");
    };
    assert_eq!(init.plan_digest.as_str(), format!("sha256:{hex_digest}"));
}

#[test]
fn exact_bytes_and_injected_identities() {
    let fixture = digests();
    let entries = fixture.get("entries").and_then(Json::as_array).expect("entries");
    let manifest = entries.iter().find(|e| text(e, "encoding") == "exact-file-bytes").expect("manifest entry");
    assert_eq!(sha256(&bytes(text(manifest, "path"))), expected(manifest));
    let selection = fixture.get("selection_fixture").expect("selection");
    assert_eq!(sha256(b""), expected(selection));
    let SessionBody::Request { request: Request::Invoke(invocation), .. } =
        session_message("invoke-before-index").body
    else {
        panic!("invoke");
    };
    let digest = invocation.selection_digest.expect("selection digest");
    assert_eq!(digest.as_str(), format!("sha256:{}", text(selection, "sha256")));
}
