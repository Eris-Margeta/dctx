// SPDX-License-Identifier: Apache-2.0
//! Shared fixture access for the PL-C contract corpus under docs/design/plugin-api (exact file bytes).
//! Implements the corpus rules of fixtures/README.md: catalog-driven cases, bodies framed as-is.
//! Depends on dctx-plugin-protocol's own strict decoder; used by every integration test in this crate.

#![allow(dead_code)]

use std::path::PathBuf;

use dctx_plugin_protocol::records::envelope::{SessionBody, SessionMessage};
use dctx_plugin_protocol::records::scalars::RequestId;
use dctx_plugin_protocol::{DecodeLimits, Json, WireMessage, decode_object};

/// Generous limits for the corpus's own index documents (not wire frames).
pub const CORPUS: DecodeLimits = DecodeLimits {
    max_bytes: 4 << 20,
    max_depth: 64,
    max_members: 1 << 20,
};

pub fn api_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/plugin-api")
}

pub fn bytes(relative: &str) -> Vec<u8> {
    let path = api_dir().join(relative);
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

pub fn document(relative: &str) -> Json {
    decode_object(&bytes(relative), CORPUS)
        .unwrap_or_else(|refusal| panic!("{relative}: {refusal}"))
}

pub fn catalog() -> Vec<Json> {
    document("fixtures/catalog.json")
        .get("cases")
        .and_then(Json::as_array)
        .expect("cases")
        .to_vec()
}

pub fn case(id: &str) -> Json {
    catalog()
        .into_iter()
        .find(|case| text(case, "id") == id)
        .unwrap_or_else(|| panic!("no case {id}"))
}

pub fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value.get(key).and_then(Json::as_str).unwrap_or("")
}

pub fn expect_field<'a>(case: &'a Json, key: &str) -> Option<&'a Json> {
    case.get("expect").and_then(|expect| expect.get(key))
}

/// Exact bytes of a catalog case's input file.
pub fn case_bytes(id: &str) -> Vec<u8> {
    bytes(text(&case(id), "path"))
}

/// Exact bytes of a fixture by file stem under valid/ or invalid/.
pub fn fixture_bytes(stem: &str) -> Vec<u8> {
    let valid = api_dir().join(format!("fixtures/valid/{stem}.json"));
    if valid.exists() {
        bytes(&format!("fixtures/valid/{stem}.json"))
    } else {
        bytes(&format!("fixtures/invalid/{stem}.json"))
    }
}

pub fn message(stem: &str) -> WireMessage {
    WireMessage::decode(&fixture_bytes(stem), DecodeLimits::WIRE)
        .unwrap_or_else(|refusal| panic!("{stem}: {refusal}"))
}

pub fn session_message(stem: &str) -> SessionMessage {
    match message(stem) {
        WireMessage::Session(message) => message,
        WireMessage::Bootstrap(_) => panic!("{stem} is a bootstrap message"),
    }
}

/// Rewrites a request or response ID (for injecting the catalog's prior-state context).
pub fn with_id(mut message: SessionMessage, id: &str) -> SessionMessage {
    let new = RequestId::new(id).expect("request id");
    match &mut message.body {
        SessionBody::Request { id, .. } | SessionBody::Response { id, .. } => *id = new,
        SessionBody::Event { .. } => panic!("events have no id"),
    }
    message
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

pub fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}
