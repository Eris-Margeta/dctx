// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile boundaries. Byte parsers receive every truncation and one-byte value;
//! typed APIs receive invalid shapes, exact bounds, duplicates and reordered valid records.

mod common;

use dctx_plugin_protocol::json::encode::write_canonical;
use dctx_plugin_protocol::negotiate::validate_ranges;
use dctx_plugin_protocol::records::bytes::{Chunk, MAX_CHUNK_BYTES, MAX_CHUNK_CHARS};
use dctx_plugin_protocol::records::common::VersionRange;
use dctx_plugin_protocol::records::envelope::SessionBody;
use dctx_plugin_protocol::records::method::Method;
use dctx_plugin_protocol::records::requests::Request;
use dctx_plugin_protocol::records::results::ResultBody;
use dctx_plugin_protocol::records::scalars::*;
use dctx_plugin_protocol::*;

const READ: &[u8] = br#"{"v":[1,0],"session":"s:1","kind":"request","id":"p:1","method":"file.read","params":{"resource":"f","offset":0,"length":1}}"#;

#[test]
fn byte_entry_points_reject_empty_one_byte_and_invalid_utf8_without_panics() {
    for input in std::iter::once(Vec::new()).chain((0..=255).map(|b| vec![b])) {
        assert!(decode_object(&input, DecodeLimits::WIRE).is_err());
        assert!(WireMessage::decode(&input, DecodeLimits::WIRE).is_err());
        let mut decoder = FrameDecoder::new();
        assert!(decoder.push_all(&input).unwrap().is_empty());
        assert_eq!(decoder.finish().is_ok(), input.is_empty());
        assert_eq!(
            FrameReader::new(input.as_slice()).next_frame().is_ok(),
            input.is_empty()
        );
    }
    for input in [
        b"{\"x\":\"\xff\"}".as_slice(),
        b"\xef\xbb\xbf{}",
        b"{\"x\":\"\xc0\x80\"}",
    ] {
        assert!(decode_object(input, DecodeLimits::WIRE).is_err());
        assert!(WireMessage::decode(input, DecodeLimits::WIRE).is_err());
    }
}

#[test]
fn every_wire_and_stream_truncation_is_safe_and_splits_reassemble() {
    let body = WireMessage::decode(READ, DecodeLimits::WIRE)
        .unwrap()
        .encode();
    let frame = encode_frame(&body, MAX_FRAME_CAP).unwrap();
    for end in 0..body.len() {
        assert!(
            WireMessage::decode(&body[..end], DecodeLimits::WIRE).is_err(),
            "offset {end}"
        );
    }
    for end in 0..=frame.len() {
        let mut decoder = FrameDecoder::new();
        let mut found = decoder.push_all(&frame[..end]).unwrap();
        assert_eq!(decoder.finish().is_ok(), end == 0 || end == frame.len());
        found.extend(decoder.push_all(&frame[end..]).unwrap());
        assert_eq!(found, vec![body.clone()]);
        assert!(decoder.finish().is_ok());
        let mut reader = FrameReader::new(&frame[..end]);
        match end {
            0 => assert_eq!(reader.next_frame(), Ok(None)),
            n if n == frame.len() => assert_eq!(reader.next_frame(), Ok(Some(body.clone()))),
            _ => assert!(reader.next_frame().is_err()),
        }
    }
}

#[test]
fn frames_accept_exact_caps_and_refuse_over_caps_before_body_allocation() {
    for cap in [1, BOOTSTRAP_FRAME_CAP, MAX_FRAME_CAP] {
        let body = vec![0xff; cap as usize];
        let frame = encode_frame(&body, cap).unwrap();
        let mut decoder = FrameDecoder::new();
        decoder.set_cap(cap).unwrap();
        assert_eq!(decoder.push_all(&frame).unwrap(), vec![body]);
        assert!(encode_frame(&vec![0; cap as usize + 1], cap).is_err());
        let mut decoder = FrameDecoder::new();
        decoder.set_cap(cap).unwrap();
        assert!(decoder.push(&(cap + 1).to_le_bytes()).is_err());
        assert!(decoder.push_all(&[]).is_err());
        assert!(decoder.push(&[]).is_err());
        assert!(decoder.finish().is_err());
    }
    assert!(encode_frame(&[], MAX_FRAME_CAP).is_err());
    assert!(encode_frame(b"a", 0).is_err());
    let mut writer = Vec::new();
    write_frame(&mut writer, b"a", 1).unwrap();
    assert_eq!(writer, encode_frame(b"a", 1).unwrap());
}

#[test]
fn negotiated_cap_cannot_be_raised_or_lowered_below_a_buffered_body() {
    let mut decoder = FrameDecoder::new();
    decoder.set_cap(MAX_FRAME_CAP).unwrap();
    decoder.set_cap(8).unwrap();
    assert!(decoder.set_cap(9).is_err());
    assert_eq!(decoder.cap(), 8);
    decoder.set_cap(8).unwrap();
    let frame = encode_frame(b"12345678", 8).unwrap();
    decoder.push(&frame[..5]).unwrap();
    assert!(decoder.set_cap(7).is_err());
    assert_eq!(decoder.cap(), 8);
    assert_eq!(
        decoder.push_all(&frame[5..]).unwrap(),
        vec![b"12345678".to_vec()]
    );
    decoder.set_cap(1).unwrap();
}

#[test]
fn json_bounds_members_depth_and_reordered_output_are_exact() {
    for limits in [
        DecodeLimits::BOOTSTRAP,
        DecodeLimits::WIRE,
        DecodeLimits::MANIFEST,
    ] {
        let mut exact = b"{}".to_vec();
        exact.resize(limits.max_bytes, b' ');
        assert!(decode_object(&exact, limits).is_ok());
        exact.push(b' ');
        assert!(decode_object(&exact, limits).is_err());
        let nested = |n| format!("{{\"a\":{}0{}}}", "[".repeat(n), "]".repeat(n));
        assert!(decode_object(nested(31).as_bytes(), limits).is_ok());
        assert!(decode_object(nested(32).as_bytes(), limits).is_err());
        let members = |n| {
            format!(
                "{{{}}}",
                (0..n)
                    .map(|i| format!("\"k{i}\":0"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        assert!(decode_object(members(4096).as_bytes(), limits).is_ok());
        assert!(decode_object(members(4097).as_bytes(), limits).is_err());
    }
    for bad in [
        br#"{"a":1,"a":2}"#.as_slice(),
        br#"{"a":1,"\u0061":2}"#,
        br#"{"x":"\u0000"}"#,
        br#"{"x":"\ud800"}"#,
    ] {
        assert!(decode_object(bad, DecodeLimits::WIRE).is_err());
    }
    let a = decode_object(
        r#"{"z":[1,true],"a":{"b":"é"}}"#.as_bytes(),
        DecodeLimits::WIRE,
    )
    .unwrap();
    let b = decode_object(
        r#"{"a":{"b":"é"},"z":[1,true]}"#.as_bytes(),
        DecodeLimits::WIRE,
    )
    .unwrap();
    assert!(a.same_value(&b));
    assert_eq!(to_canonical(&a), to_canonical(&b));
    let mut appended = b"prefix".to_vec();
    write_canonical(&a, &mut appended);
    assert_eq!(&appended[6..], to_canonical(&a));
}

#[test]
fn signed_integer_extrema_round_trip_without_overflow() {
    for n in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX] {
        let value = Json::Object(vec![("n".into(), Json::Int(n))]);
        assert_eq!(
            decode_object(&to_canonical(&value), DecodeLimits::WIRE),
            Ok(value)
        );
    }
    for text in [
        "{\"n\":-9223372036854775809}",
        "{\"n\":9223372036854775808}",
        "{\"n\":-0}",
    ] {
        assert!(decode_object(text.as_bytes(), DecodeLimits::WIRE).is_err());
    }
}

macro_rules! scalar_bound {
    ($test:ident, $ty:ty, $valid:expr, $max:expr) => {
        #[test]
        fn $test() {
            let valid: String = $valid;
            assert_eq!(valid.len(), $max);
            let value = <$ty>::new(&valid).unwrap();
            assert_eq!(value.as_str(), valid);
            assert!(<$ty>::new(&(valid.clone() + "a")).is_err());
            for text in ["", "\0", "\n", "é", "../"] {
                assert!(<$ty>::new(text).is_err(), "{text:?}");
            }
            for end in 0..=valid.len() {
                let _ = <$ty>::new(&valid[..end]);
            }
            assert_eq!(<$ty>::new(&valid), <$ty>::new(&valid));
        }
    };
}
scalar_bound!(token_bounds, Token, "a".repeat(128), 128);
scalar_bound!(
    request_id_bounds,
    RequestId,
    format!("p:{}", "a".repeat(126)),
    128
);
scalar_bound!(
    plugin_id_bounds,
    PluginId,
    format!("p/{}", "a".repeat(126)),
    128
);
scalar_bound!(name_bounds, Name, "a".repeat(128), 128);
scalar_bound!(role_bounds, Role, "a".repeat(64), 64);
scalar_bound!(alias_bounds, Alias, "a".repeat(64), 64);
scalar_bound!(
    semver_bounds,
    SemVer,
    format!("1.0.0+{}", "a".repeat(122)),
    128
);
scalar_bound!(
    digest_bounds,
    Digest,
    format!("sha256:{}", "a".repeat(64)),
    71
);
scalar_bound!(project_path_bounds, ProjectPath, "a".repeat(256), 256);

#[test]
fn text_and_raw_chunk_boundaries_preserve_arbitrary_bytes() {
    for text in [String::new(), "a".into(), "é".repeat(2048)] {
        assert_eq!(Text::new(&text).unwrap().as_str(), text);
    }
    assert!(Text::new(&"x".repeat(4097)).is_err());
    for size in [0, 1, 2, 3, MAX_CHUNK_BYTES] {
        let bytes: Vec<_> = (0..=255).cycle().take(size).collect();
        let chunk = Chunk::new(&bytes).unwrap();
        assert_eq!(chunk.len(), size);
        assert_eq!(chunk.is_empty(), size == 0);
        let encoded = chunk.encode_base64();
        assert_eq!(Chunk::decode_base64(&encoded).unwrap().as_bytes(), bytes);
        assert_eq!(encoded, chunk.encode_base64());
    }
    assert!(Chunk::new(&vec![0; MAX_CHUNK_BYTES + 1]).is_err());
    for bad in ["A", "=", "====", "AA=A", "AB==", "AAB=", "é", "AA==\n"] {
        assert!(Chunk::decode_base64(bad).is_err(), "{bad}");
    }
    assert!(Chunk::decode_base64(&"A".repeat(MAX_CHUNK_CHARS)).is_err());
    assert!(Chunk::decode_base64(&"A".repeat(MAX_CHUNK_CHARS + 4)).is_err());
    for end in 0..=8 {
        let _ = Chunk::decode_base64(&"YWJjZA=="[..end]);
    }
}

#[test]
fn all_wire_fixtures_truncate_mutate_and_encode_identically() {
    let mut paths: Vec<_> = std::fs::read_dir(common::api_dir().join("fixtures/valid"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    paths.sort();
    let mut accepted = 0;
    for path in paths {
        let bytes = std::fs::read(path).unwrap();
        let Ok(message) = WireMessage::decode(&bytes, DecodeLimits::WIRE) else {
            continue;
        };
        accepted += 1;
        let encoded = message.encode();
        assert_eq!(encoded, message.encode());
        assert_eq!(
            WireMessage::from_json(&message.to_json()),
            Ok(message.clone())
        );
        assert_eq!(
            WireMessage::decode(&encoded, DecodeLimits::WIRE)
                .unwrap()
                .encode(),
            encoded
        );
        for end in 0..encoded.len() {
            assert!(WireMessage::decode(&encoded[..end], DecodeLimits::WIRE).is_err());
        }
        for at in 0..encoded.len() {
            for byte in [0, 0xff, b'"', b'{', b'}', b'[', b']', b','] {
                let mut mutated = encoded.clone();
                mutated[at] = byte;
                if let Ok(m) = WireMessage::decode(&mutated, DecodeLimits::WIRE) {
                    assert_eq!(WireMessage::decode(&m.encode(), DecodeLimits::WIRE), Ok(m));
                }
            }
        }
        if let WireMessage::Session(session) = message
            && let SessionBody::Request { request, .. } = session.body
        {
            assert_eq!(
                Request::decode(request.method(), &request.encode_params()),
                Ok(request.clone())
            );
            assert_eq!(request.action_input(), request.action_input());
        }
    }
    assert!(accepted >= 60);
}

#[test]
fn every_method_rejects_empty_and_scalar_shapes() {
    for &method in Method::ALL {
        for value in [
            Json::Null,
            Json::Int(0),
            Json::Str("x".into()),
            Json::Array(vec![]),
            Json::Object(vec![]),
        ] {
            assert!(Request::decode(method, &value).is_err());
            assert!(ResultBody::decode(method, &value).is_err());
        }
    }
}

#[test]
fn version_selection_is_order_independent_and_duplicates_refuse() {
    let a = VersionRange {
        major: 1,
        min_minor: 0,
        max_minor: u64::MAX,
    };
    let b = VersionRange {
        major: 2,
        min_minor: 0,
        max_minor: u64::MAX,
    };
    assert!(select_version(&[], &[a]).is_err());
    assert!(select_version(&[a], &[]).is_err());
    assert_eq!(
        select_version(&[a, b], &[b, a]),
        select_version(&[b, a], &[a, b])
    );
    assert!(validate_ranges(&[a, a]).is_err());
    assert!(
        validate_ranges(&[VersionRange {
            min_minor: 1,
            max_minor: 0,
            ..a
        }])
        .is_err()
    );
}

#[test]
fn concurrent_immutable_codecs_produce_byte_identical_results() {
    let expected = WireMessage::decode(READ, DecodeLimits::WIRE)
        .unwrap()
        .encode();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    for _ in 0..32 {
                        let actual = WireMessage::decode(READ, DecodeLimits::WIRE)
                            .unwrap()
                            .encode();
                        assert_eq!(actual, expected);
                        assert_eq!(
                            encode_frame(&actual, MAX_FRAME_CAP).unwrap(),
                            encode_frame(&expected, MAX_FRAME_CAP).unwrap()
                        );
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}

fn replace_member(value: &mut Json, key: &str, replacement: Json) {
    let Json::Object(members) = value else {
        panic!("object")
    };
    members.iter_mut().find(|(name, _)| name == key).unwrap().1 = replacement;
}

fn request_params(stem: &str) -> (Method, Json) {
    let WireMessage::Session(message) = common::message(stem) else {
        panic!("session")
    };
    let SessionBody::Request { request, .. } = message.body else {
        panic!("request")
    };
    (request.method(), request.encode_params())
}

macro_rules! enum_boundaries {
    ($test:ident, $ty:ty) => {
        #[test]
        fn $test() {
            for &value in <$ty>::ALL {
                assert_eq!(<$ty>::parse(value.as_str()), Some(value));
                assert!(<$ty>::parse(&(value.as_str().to_owned() + "x")).is_none());
                for end in 0..value.as_str().len() {
                    let _ = <$ty>::parse(&value.as_str()[..end]);
                }
            }
            for b in 0..=127 {
                assert!(<$ty>::parse(&char::from(b).to_string()).is_none());
            }
            for text in [
                "".into(),
                "é".into(),
                "\0".into(),
                "x".repeat(128),
                "x".repeat(129),
            ] {
                assert!(<$ty>::parse(&text).is_none());
            }
        }
    };
}
enum_boundaries!(method_spellings, Method);
enum_boundaries!(error_code_spellings, ErrorCode);
enum_boundaries!(phase_spellings, Phase);
enum_boundaries!(effect_spellings, EffectState);
enum_boundaries!(
    hook_spellings,
    dctx_plugin_protocol::records::common::HookName
);
enum_boundaries!(
    resource_kind_spellings,
    dctx_plugin_protocol::records::common::ResourceKind
);
enum_boundaries!(
    severity_spellings,
    dctx_plugin_protocol::records::common::Severity
);
enum_boundaries!(
    operation_spellings,
    dctx_plugin_protocol::records::capability::OutputOperation
);
enum_boundaries!(
    unit_spellings,
    dctx_plugin_protocol::records::events::ProgressUnit
);
enum_boundaries!(
    location_spellings,
    dctx_plugin_protocol::records::invocation::OutputLocation
);
enum_boundaries!(
    intent_spellings,
    dctx_plugin_protocol::records::invocation::IndexIntent
);
enum_boundaries!(
    freshness_spellings,
    dctx_plugin_protocol::records::invocation::Freshness
);
enum_boundaries!(
    cancel_reason_spellings,
    dctx_plugin_protocol::records::requests::CancelReason
);
enum_boundaries!(
    shutdown_reason_spellings,
    dctx_plugin_protocol::records::requests::ShutdownReason
);
enum_boundaries!(
    invoke_status_spellings,
    dctx_plugin_protocol::records::results::InvokeStatus
);
enum_boundaries!(
    probe_state_spellings,
    dctx_plugin_protocol::records::results::ProbeState
);
enum_boundaries!(
    program_status_spellings,
    dctx_plugin_protocol::records::results::ProgramStatus
);

#[test]
fn text_nul_cannot_construct_a_record_that_fails_its_own_wire_round_trip() {
    assert!(Text::new("\0").is_err());
    assert!(Text::new("prefix\0suffix").is_err());
    for text in ["", "a", "\n\r\t", "é"] {
        let value = Text::new(text).unwrap();
        let body = Json::Object(vec![("text".into(), Json::str(value.as_str()))]);
        assert_eq!(
            decode_object(&to_canonical(&body), DecodeLimits::WIRE),
            Ok(body)
        );
    }
}

#[test]
fn capability_public_codec_checks_bounds_shapes_duplicates_and_reordered_members() {
    use dctx_plugin_protocol::records::capability::Capability;
    for value in [
        Json::Null,
        Json::Str("x".into()),
        Json::Object(vec![]),
        Json::Array(vec![]),
    ] {
        assert!(Capability::decode(&value).is_err());
    }
    for cap in [
        Capability::FileRead {
            path: ProjectPath::new("a").unwrap(),
            max_bytes: 1,
        },
        Capability::FileWrite {
            path: ProjectPath::new("a").unwrap(),
            create: false,
            max_growth_bytes: 0,
            max_bytes: 1,
        },
        Capability::GitAddIgnore {
            max_output_bytes: 1,
        },
        Capability::OutputsRead {
            roles: vec![Role::new("x").unwrap()],
            operations: vec![dctx_plugin_protocol::records::capability::OutputOperation::Pack],
            max_bytes: 1,
        },
        Capability::State {
            read: true,
            write: false,
            reclaim: false,
            max_bytes: 1,
            source_content: false,
        },
    ] {
        let encoded = cap.encode();
        assert_eq!(Capability::decode(&encoded), Ok(cap.clone()));
        let Json::Object(mut members) = encoded else {
            unreachable!()
        };
        members.reverse();
        assert_eq!(Capability::decode(&Json::Object(members.clone())), Ok(cap));
        members.push(members[0].clone());
        assert!(Capability::decode(&Json::Object(members)).is_err());
    }
    for bound in [0, 1, 65_536, 65_537, 9_007_199_254_740_991] {
        let cap = Capability::GitAddIgnore {
            max_output_bytes: bound,
        };
        assert_eq!(
            Capability::decode(&cap.encode()).is_ok(),
            (1..=65_536).contains(&bound)
        );
    }
    for n in [0, 1, 32, 33] {
        let cap = Capability::OutputsRead {
            roles: (0..n)
                .map(|i| Role::new(&format!("r{i}")).unwrap())
                .collect(),
            operations: vec![dctx_plugin_protocol::records::capability::OutputOperation::Pack],
            max_bytes: 1,
        };
        assert_eq!(
            Capability::decode(&cap.encode()).is_ok(),
            (1..=32).contains(&n)
        );
    }
}

#[test]
fn request_vector_caps_and_numeric_extrema_are_enforced() {
    for (stem, field, cap) in [
        ("initialize", "grants", 64),
        ("initialize", "resources", 128),
        ("invoke-before-pack-output-written", "outputs", 64),
        ("invoke-command", "args", 64),
    ] {
        let (method, params) = request_params(stem);
        let item = params
            .get(field)
            .unwrap()
            .as_array()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_else(|| Json::str("argument"));
        for n in [0, 1, cap, cap + 1] {
            let mut params = params.clone();
            replace_member(&mut params, field, Json::Array(vec![item.clone(); n]));
            assert_eq!(
                Request::decode(method, &params).is_ok(),
                n <= cap,
                "{stem} {field} {n}"
            );
        }
    }
    for stem in [
        "request-file-read",
        "request-output-read",
        "request-state-read",
    ] {
        let (method, params) = request_params(stem);
        for length in [0, 1, 65_536, 65_537] {
            let mut params = params.clone();
            replace_member(&mut params, "length", Json::uint(length));
            assert_eq!(
                Request::decode(method, &params).is_ok(),
                (1..=65_536).contains(&length)
            );
        }
        for offset in [
            i64::MIN,
            -1,
            0,
            9_007_199_254_740_991,
            9_007_199_254_740_992,
            i64::MAX,
        ] {
            let mut params = params.clone();
            replace_member(&mut params, "offset", Json::Int(offset));
            assert_eq!(
                Request::decode(method, &params).is_ok(),
                (0..=9_007_199_254_740_991).contains(&offset)
            );
        }
    }
    let (method, params) = request_params("request-state-write");
    for n in [0, 1, 128, 129] {
        let mut params = params.clone();
        replace_member(
            &mut params,
            "provenance",
            Json::Array((0..n).map(|i| Json::str(&format!("h:{i}"))).collect()),
        );
        assert_eq!(Request::decode(method, &params).is_ok(), n <= 128);
    }
    let mut duplicate = params;
    replace_member(
        &mut duplicate,
        "provenance",
        Json::Array(vec![Json::str("h:1"); 2]),
    );
    assert!(Request::decode(method, &duplicate).is_err());
}

#[test]
fn result_and_error_public_codecs_have_exact_counts_and_pending_resolution() {
    for (method, stem) in [
        (Method::Invoke, "result-invoke"),
        (Method::ProgramRun, "result-program-run"),
        (Method::OutputList, "result-output-list"),
    ] {
        let value = common::message(stem)
            .to_json()
            .get("result")
            .unwrap()
            .clone();
        let field = if method == Method::OutputList {
            "outputs"
        } else {
            "diagnostics"
        };
        let item = value
            .get(field)
            .unwrap()
            .as_array()
            .unwrap()
            .first()
            .cloned()
            .unwrap_or_else(|| {
                decode_object(
                    br#"{"code":"fixture","severity":"info"}"#,
                    DecodeLimits::WIRE,
                )
                .unwrap()
            });
        for n in [0, 1, 64, 65] {
            let mut value = value.clone();
            replace_member(&mut value, field, Json::Array(vec![item.clone(); n]));
            assert_eq!(ResultBody::decode(method, &value).is_ok(), n <= 64);
            if let Ok(body) = ResultBody::decode(method, &value) {
                let pending = PendingResult::from_body(&body);
                assert_eq!(pending.resolve(method).unwrap(), body);
                assert!(ResultBody::matches_any(pending.as_json()));
                assert_eq!(
                    to_canonical(&body.encode()),
                    to_canonical(pending.as_json())
                );
            }
        }
    }
    for &code in ErrorCode::ALL {
        let mut error = WireError::refusal(code, Phase::Broker, Name::new("fixture").unwrap());
        for n in [0, 1, 64, 65] {
            error.receipt_ids = (0..n)
                .map(|i| Token::new(&format!("r:{i}")).unwrap())
                .collect();
            assert_eq!(WireError::decode(&error.encode()).is_ok(), n <= 64);
        }
        error.receipt_ids = vec![Token::new("r:1").unwrap(); 2];
        assert!(WireError::decode(&error.encode()).is_err());
        error.receipt_ids.clear();
        error.effect_state = EffectState::Unknown;
        error.retryable = true;
        assert!(WireError::decode(&error.encode()).is_err());
    }
}

#[test]
fn progress_tracker_extrema_and_phase_capacity_preserve_state_on_refusal() {
    use dctx_plugin_protocol::records::events::{Progress, ProgressUnit};
    use dctx_plugin_protocol::session::{MAX_PHASES, ProgressTrack};
    let sample = |seq, phase, completed| Progress {
        invocation: Token::new("op:1").unwrap(),
        seq,
        phase: Name::new(&format!("phase{phase}")).unwrap(),
        completed,
        total: Some(u64::MAX),
        unit: ProgressUnit::Bytes,
        wait: None,
    };
    let mut track = ProgressTrack::default();
    assert_eq!(track.last_seq(), 0);
    for i in 0..MAX_PHASES {
        track.accept(&sample(i as u64 + 1, i, 0)).unwrap();
    }
    let before = track.clone();
    assert!(track.accept(&sample(100, MAX_PHASES, 0)).is_err());
    assert_eq!(track, before);
    track.accept(&sample(u64::MAX, 0, u64::MAX)).unwrap();
    let before = track.clone();
    assert!(track.accept(&sample(u64::MAX, 0, u64::MAX)).is_err());
    assert_eq!(track, before);
}

fn negotiated_session() -> Session {
    use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
    let WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }) = common::message("hello")
    else {
        panic!("hello")
    };
    let WireMessage::Bootstrap(answer) = common::message("hello-selected") else {
        panic!("answer")
    };
    let BootstrapMessage::Selected { selected, .. } = &answer else {
        panic!("selection")
    };
    let negotiated = accept_answer(&id, &hello, &answer, &selected.identity, &[]).unwrap();
    Session::new(Token::new("s:1").unwrap(), negotiated)
}

#[test]
fn session_refusals_are_atomic_at_zero_exact_and_over_outstanding_capacity() {
    let mut session = negotiated_session();
    assert_eq!(session.state(), SessionState::Negotiated);
    assert!(session.active_invocation().is_none());
    let early = common::session_message("request-file-read");
    assert!(session.observe(Sender::Plugin, &early).is_err());
    assert_eq!(session.outstanding(Sender::Plugin), 0);
    for (sender, stem) in [
        (Sender::Host, "initialize"),
        (Sender::Plugin, "result-initialize"),
        (Sender::Host, "invoke-before-pack-output-written"),
    ] {
        session
            .observe(sender, &common::session_message(stem))
            .unwrap();
    }
    assert_eq!(session.state(), SessionState::Active);
    assert!(session.active_invocation().is_some());
    assert!(session.frame_cap() > 0);
    let mut last = early.clone();
    for i in 0..16 {
        let request = common::with_id(early.clone(), &format!("p:hostile-{i}"));
        session.observe(Sender::Plugin, &request).unwrap();
        assert!(session.observe(Sender::Plugin, &request).is_err());
        last = request;
    }
    assert_eq!(session.outstanding(Sender::Plugin), 16);
    assert!(
        session
            .observe(Sender::Plugin, &common::with_id(early, "p:over"))
            .is_err()
    );
    let before = session.outstanding(Sender::Plugin);
    let mut foreign = last;
    foreign.session = Token::new("foreign").unwrap();
    assert!(session.observe(Sender::Plugin, &foreign).is_err());
    assert_eq!(session.outstanding(Sender::Plugin), before);
    session.register_dependency(Token::new("dep:1").unwrap());
    session.register_dependency(Token::new("dep:1").unwrap());
}

#[test]
fn negotiation_answers_repeat_canonically_and_refuse_unbound_or_invalid_offers() {
    use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
    let WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }) = common::message("hello")
    else {
        panic!("hello")
    };
    let WireMessage::Bootstrap(BootstrapMessage::Selected { selected, .. }) =
        common::message("hello-selected")
    else {
        panic!("selection")
    };
    let worker = WorkerProfile {
        supported: SUPPORTED_VERSIONS.to_vec(),
        identity: selected.identity.clone(),
        required_apis: selected.required_apis.clone(),
        optional_apis: selected.optional_apis.clone(),
    };
    let answer = answer_hello(&id, &hello, &worker).unwrap();
    assert_eq!(
        WireMessage::Bootstrap(answer.clone()).encode(),
        WireMessage::Bootstrap(answer_hello(&id, &hello, &worker).unwrap()).encode()
    );
    assert!(
        accept_answer(
            &RequestId::new("h:foreign").unwrap(),
            &hello,
            &answer,
            &selected.identity,
            &[]
        )
        .is_err()
    );
    let mut no_common = hello.clone();
    no_common.versions.clear();
    assert!(matches!(
        answer_hello(&id, &no_common, &worker).unwrap(),
        BootstrapMessage::Refused { .. }
    ));
    let mut duplicate = *hello;
    duplicate.versions.push(duplicate.versions[0]);
    assert!(answer_hello(&id, &duplicate, &worker).is_err());
}

#[test]
fn stream_short_io_interruption_and_failures_are_typed_and_repeatable() {
    use std::io::{self, Read, Write};
    struct Short {
        bytes: Vec<u8>,
        interrupt: bool,
    }
    impl Write for Short {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let n = bytes.len().min(1);
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct Zero;
    impl Write for Zero {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let mut writer = Short {
        bytes: vec![],
        interrupt: false,
    };
    write_frame(&mut writer, READ, MAX_FRAME_CAP).unwrap();
    assert_eq!(writer.bytes, encode_frame(READ, MAX_FRAME_CAP).unwrap());
    assert_eq!(
        write_frame(&mut Zero, b"a", 1),
        Err(StreamError::Io(io::ErrorKind::WriteZero))
    );
    assert_eq!(
        FrameReader::new(Broken).next_frame(),
        Err(StreamError::Io(io::ErrorKind::BrokenPipe))
    );
}

#[test]
fn every_protocol_failure_names_a_next_step_and_redacts_input() {
    use dctx_plugin_protocol::refusal::{Predicate, Stage};
    for stage in [
        Stage::Frame,
        Stage::Decode,
        Stage::Schema,
        Stage::Semantic,
        Stage::Session,
    ] {
        for &code in ErrorCode::ALL {
            let refusal = Refusal::new(code, stage, Predicate::SBounds, "fixture");
            assert!(refusal.to_string().contains("; next: "));
            assert!(
                StreamError::Protocol(refusal)
                    .to_string()
                    .contains("; next: ")
            );
        }
    }
    for kind in [
        std::io::ErrorKind::BrokenPipe,
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::UnexpectedEof,
    ] {
        assert!(StreamError::Io(kind).to_string().contains("; next: "));
    }
    let bytes = b"{\"PRIVATE-CONTENT\":\"\xff\"}";
    assert!(
        !decode_object(bytes, DecodeLimits::WIRE)
            .unwrap_err()
            .to_string()
            .contains("PRIVATE-CONTENT")
    );
}

#[test]
fn request_dependent_chunk_checks_cover_empty_max_overflow_and_invalid_eof() {
    use dctx_plugin_protocol::records::broker::ChunkRead;
    use dctx_plugin_protocol::records::results::ChunkResult;
    use dctx_plugin_protocol::session::{check_chunk, check_result};
    for size in [0, 1, MAX_CHUNK_BYTES] {
        let read = ChunkRead {
            handle: Token::new("f").unwrap(),
            offset: 0,
            length: size as u64,
            precondition: None,
        };
        let mut chunk = ChunkResult {
            bytes: Chunk::new(&vec![0xff; size]).unwrap(),
            offset: 0,
            size: size as u64,
            eof: true,
            precondition: None,
            receipt: Token::new("r").unwrap(),
        };
        check_chunk(&read, &chunk).unwrap();
        check_result(
            &Request::FileRead(read.clone()),
            &ResultBody::Chunk(chunk.clone()),
        )
        .unwrap();
        chunk.eof = false;
        assert!(check_chunk(&read, &chunk).is_err());
        chunk.eof = true;
        chunk.offset = u64::MAX;
        let mut read = read;
        read.offset = u64::MAX;
        if size > 0 {
            assert!(check_chunk(&read, &chunk).is_err());
        }
    }
}
