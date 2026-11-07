// SPDX-License-Identifier: Apache-2.0
use super::*;
use serde_json::{json, Value};

fn session(input: &[u8]) -> (Vec<Value>, Vec<String>) {
    let calls = std::cell::RefCell::new(Vec::new());
    let exec = |request: &Request| {
        calls.borrow_mut().push(format!("{:?}", request.id));
        Ok(CommandOutput {
            stdout: request.args.join(" "),
            failure: None,
        })
    };
    let mut output = Vec::new();
    serve(input, &mut output, &exec).expect("serve");
    let replies = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("response"))
        .collect();
    (replies, calls.into_inner())
}

#[test]
fn handshake_ordering_unknown_fields_and_recovery() {
    let input = concat!(
        "{\"id\":0,\"verb\":\"find\"}\n",
        "bad json\n",
        "{\"id\":1,\"verb\":\"hello\",\"future\":true}\n",
        "{\"id\":\"a\",\"verb\":\"find\",\"args\":[\"first\"]}\n",
        "{\"id\":2,\"verb\":\"unknown\"}\n",
        "{\"id\":3,\"verb\":\"read\",\"args\":17}\n",
        "{\"verb\":\"find\"}\n",
        "{\"id\":false,\"verb\":\"find\"}\n",
        "\n",
        "{\"id\":4,\"verb\":\"find\",\"args\":[\"last\"]}"
    );
    let (replies, calls) = session(input.as_bytes());
    assert_eq!(replies.len(), 10);
    assert_eq!(calls, ["String(\"a\")", "Integer(4)"]);
    for reply in &replies {
        assert_eq!(reply["engine_version"], env!("CARGO_PKG_VERSION"));
        assert_ne!(reply.get("error").is_some(), reply.get("result").is_some());
    }
    assert_eq!(replies[2]["result"]["protocol_version"], PROTOCOL_VERSION);
    assert_eq!(replies[2]["result"]["verbs"], json!(Verb::ALL));
    assert_eq!(replies[3]["id"], "a");
    assert_eq!(replies[3]["result"]["output"], "first");
    assert_eq!(replies[4]["id"], 2);
    assert_eq!(replies[5]["id"], 3);
    for index in [0, 1, 4, 5, 6, 7, 8] {
        assert_eq!(replies[index]["error"]["code"], 2);
    }
    assert_eq!(replies[9]["result"]["output"], "last");
}

#[test]
fn invalid_utf8_and_oversized_frames_are_drained_and_session_continues() {
    let mut input = vec![b'x'; MAX_LINE_BYTES + 1];
    input.extend_from_slice(b"\n\xff\n{\"id\":1,\"verb\":\"hello\"}\r\n");
    let (replies, calls) = session(&input);
    assert!(calls.is_empty());
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["error"]["code"], 2);
    assert_eq!(replies[1]["error"]["code"], 2);
    assert_eq!(replies[2]["result"]["protocol_version"], PROTOCOL_VERSION);
}

#[test]
fn no_in_band_cancellation_and_eof_drains_the_ordered_input() {
    let (replies, calls) = session(
        concat!(
            "{\"id\":1,\"verb\":\"hello\"}\n",
            "{\"id\":2,\"verb\":\"find\"}\n",
            "{\"id\":3,\"verb\":\"cancel\",\"args\":[\"2\"]}\n",
            "{\"id\":4,\"verb\":\"read\"}\n"
        )
        .as_bytes(),
    );
    assert_eq!(calls, ["Integer(2)", "Integer(4)"]);
    assert_eq!(replies[2]["error"]["code"], 2);
    assert!(replies[3].get("result").is_some());
}

#[test]
fn transport_has_no_filesystem_cache_selection_or_page_processing() {
    let source = include_str!("mod.rs");
    for forbidden in [
        "std::fs",
        "std::process",
        "Command::",
        "read_dir",
        "walkdir",
        "FsSource",
        "cache_root_dir",
        "load_ledger",
        "VecSink",
        "MapRequest",
        "PackRequest",
        "sort_by",
        ".sort(",
        ".retain(",
        ".truncate(",
        ".split_whitespace(",
        "estimate_tokens",
        "encode_answer",
        "render_",
        "serde_json::from_str",
    ] {
        assert!(
            !source.contains(forbidden),
            "RPC transport contains {forbidden}"
        );
    }
}

#[test]
fn response_readers_ignore_future_fields_and_handshake_refusals_do_not_initialize() {
    let (replies, _) = session(
        concat!(
            "{\"id\":1,\"verb\":\"hello\",\"args\":[\"bad\"]}\n",
            "{\"id\":2,\"verb\":\"find\"}\n",
            "{\"id\":3,\"verb\":\"hello\"}\n",
            "{\"id\":4,\"verb\":\"hello\"}\n"
        )
        .as_bytes(),
    );
    assert_eq!(replies[0]["error"]["code"], 2);
    assert_eq!(replies[1]["error"]["code"], 2);
    for mut value in replies {
        value["future"] = json!({"nested":true});
        let _: Response = serde_json::from_value(value).expect("ignore unknown response field");
    }
}
