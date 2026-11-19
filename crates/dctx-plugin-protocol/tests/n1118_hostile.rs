// SPDX-License-Identifier: Apache-2.0

mod common;

use std::io::{self, Read};
use std::panic::catch_unwind;

use dctx_plugin_protocol::records::bootstrap::BootstrapMessage;
use dctx_plugin_protocol::records::common::{ApiVersion, VersionRange};
use dctx_plugin_protocol::records::events::Event;
use dctx_plugin_protocol::records::scalars::{Name, Sender, Token};
use dctx_plugin_protocol::{
    BOOTSTRAP_FRAME_CAP, DecodeLimits, ErrorCode, FrameDecoder, FrameReader, MAX_FRAME_CAP, Phase,
    Predicate, Refusal, SUPPORTED_VERSIONS, Session, SessionBody, Stage, StreamError, WireError,
    WireMessage, WorkerProfile, accept_answer, answer_hello, encode_frame, select_version,
};

fn frame_refusal(reason: &'static str) -> Refusal {
    Refusal::new(
        ErrorCode::InvalidMessage,
        Stage::Frame,
        Predicate::WFrame,
        reason,
    )
}

#[test]
fn empty_and_truncated_inputs_have_typed_outcomes() {
    // EOF between frames is legal. An empty message body is invalid.
    assert_eq!(FrameReader::new(&b""[..]).next_frame(), Ok(None));
    assert_eq!(
        WireMessage::decode(b"", DecodeLimits::BOOTSTRAP),
        Err(frame_refusal("non-object"))
    );
    for (input, reason) in [
        (&[1][..], "truncated-prefix"),
        (&[1, 0][..], "truncated-prefix"),
        (&[1, 0, 0][..], "truncated-prefix"),
        (&[1, 0, 0, 0][..], "truncated-body"),
        (&[2, 0, 0, 0, b'{'][..], "truncated-body"),
    ] {
        assert_eq!(
            FrameReader::new(input).next_frame(),
            Err(StreamError::Protocol(frame_refusal(reason))),
            "{input:?}"
        );
    }
}

#[test]
fn illegal_lengths_are_refused_from_the_prefix_alone() {
    struct PrefixOnly<'a>(&'a [u8]);

    impl Read for PrefixOnly<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            assert!(!self.0.is_empty(), "reader tried to read a rejected body");
            self.0.read(buf)
        }
    }

    for cap in [BOOTSTRAP_FRAME_CAP, MAX_FRAME_CAP] {
        for (length, code, reason) in [
            (0, ErrorCode::InvalidMessage, "zero-length"),
            (cap + 1, ErrorCode::ResourceLimit, "frame-too-large"),
            (u32::MAX, ErrorCode::ResourceLimit, "frame-too-large"),
        ] {
            let prefix = length.to_le_bytes();
            let mut reader = FrameReader::new(PrefixOnly(&prefix));
            reader.decoder_mut().set_cap(cap).unwrap();
            let Err(StreamError::Protocol(refusal)) = reader.next_frame() else {
                panic!("expected a framing refusal for length {length} at cap {cap}");
            };
            assert_eq!(
                (refusal.code, refusal.stage, refusal.reason),
                (code, Stage::Frame, reason)
            );
            assert_eq!(
                reader.next_frame(),
                Err(StreamError::Protocol(frame_refusal(
                    "stream-desynchronized"
                )))
            );
        }
    }
}

#[test]
fn unknown_kind_and_trailing_garbage_are_refused() {
    let unknown = br#"{"v":[1,0],"session":"s:1","kind":"unknown"}"#;
    let mut trailing_body = common::message("hello").encode();
    trailing_body.push(b'!');
    for (body, stage, reason) in [
        (&unknown[..], Stage::Schema, "unknown-kind"),
        (&trailing_body, Stage::Decode, "trailing-json"),
    ] {
        let frame = encode_frame(body, BOOTSTRAP_FRAME_CAP).unwrap();
        let decoded = FrameReader::new(frame.as_slice())
            .next_frame()
            .unwrap()
            .unwrap();
        let refusal = WireMessage::decode(&decoded, DecodeLimits::BOOTSTRAP).unwrap_err();
        assert_eq!(
            (refusal.code, refusal.stage, refusal.reason),
            (ErrorCode::InvalidMessage, stage, reason)
        );
    }

    let mut frame = encode_frame(b"{}", BOOTSTRAP_FRAME_CAP).unwrap();
    frame.push(b'!');
    let mut reader = FrameReader::new(frame.as_slice());
    assert_eq!(reader.next_frame(), Ok(Some(b"{}".to_vec())));
    assert_eq!(
        reader.next_frame(),
        Err(StreamError::Protocol(frame_refusal("truncated-prefix")))
    );
}

#[test]
fn every_message_kind_round_trips_at_unpadded_and_maximum_frame_size() {
    for (stems, limits, cap) in [
        (
            &["hello", "hello-selected", "hello-error"][..],
            DecodeLimits::BOOTSTRAP,
            BOOTSTRAP_FRAME_CAP,
        ),
        (
            &[
                "request-file-read",
                "result-file-read",
                "error-code-denied",
                "event-progress",
                "event-cancelled",
            ][..],
            DecodeLimits::WIRE,
            MAX_FRAME_CAP,
        ),
    ] {
        for stem in stems {
            let message = common::message(stem);
            let canonical = message.encode();
            for len in [canonical.len(), cap as usize] {
                let mut body = canonical.clone();
                // JSON whitespace fills the frame without changing the record.
                body.resize(len, b' ');
                let frame = encode_frame(&body, cap).unwrap();
                assert_eq!(
                    frame[..4],
                    u32::try_from(len).unwrap().to_le_bytes(),
                    "{stem}"
                );
                let mut reader = FrameReader::new(frame.as_slice());
                reader.decoder_mut().set_cap(cap).unwrap();
                let decoded = reader.next_frame().unwrap().unwrap();
                assert_eq!(decoded, body, "{stem}");
                assert_eq!(
                    WireMessage::decode(&decoded, limits),
                    Ok(message.clone()),
                    "{stem}"
                );
                assert_eq!(reader.next_frame(), Ok(None), "{stem}");
            }
        }
    }

    // One byte is the framing minimum, irrespective of the JSON envelope.
    let frame = encode_frame(b"{", BOOTSTRAP_FRAME_CAP).unwrap();
    assert_eq!(
        FrameDecoder::new().push(&frame),
        Ok((5, Some(b"{".to_vec())))
    );
}

#[test]
fn negotiation_rejects_disjoint_duplicate_and_zero_major_offers() {
    let range = |major| VersionRange {
        major,
        min_minor: 0,
        max_minor: 0,
    };
    for (offer, code, reason) in [
        (
            vec![range(2)],
            ErrorCode::UnsupportedVersion,
            "no-common-version",
        ),
        (
            vec![range(1), range(1)],
            ErrorCode::InvalidMessage,
            "duplicate-major",
        ),
        (vec![range(0)], ErrorCode::InvalidMessage, "zero-major"),
    ] {
        let refusal = select_version(&offer, &SUPPORTED_VERSIONS).unwrap_err();
        assert_eq!((refusal.code, refusal.reason), (code, reason));
    }
    assert_eq!(
        select_version(&[range(0)], &[range(0)]),
        Err(Refusal::new(
            ErrorCode::InvalidMessage,
            Stage::Semantic,
            Predicate::WNegotiate,
            "zero-major"
        ))
    );
    assert_eq!(
        select_version(&SUPPORTED_VERSIONS, &[range(0)])
            .unwrap_err()
            .reason,
        "zero-major"
    );
}

#[test]
fn negotiation_selects_highest_common_version_in_any_order() {
    let range = |major, min_minor, max_minor| VersionRange {
        major,
        min_minor,
        max_minor,
    };
    let mut offer = [range(2, 5, 8), range(1, 0, 99), range(3, 0, 1)];
    let mut supported = [range(3, 2, 8), range(1, 1, 99), range(2, 3, 7)];
    for _ in 0..offer.len() {
        for _ in 0..supported.len() {
            assert_eq!(
                select_version(&offer, &supported),
                Ok(ApiVersion { major: 2, minor: 7 })
            );
            supported.rotate_left(1);
        }
        offer.rotate_left(1);
    }
}

#[test]
fn negotiation_bounds_features_and_ignores_offer_order() {
    let WireMessage::Bootstrap(BootstrapMessage::Hello { id, mut hello }) =
        common::message("hello")
    else {
        panic!("expected hello fixture");
    };
    let WireMessage::Bootstrap(BootstrapMessage::Selected { selected, .. }) =
        common::message("hello-selected")
    else {
        panic!("expected selected fixture");
    };
    let worker = WorkerProfile {
        supported: SUPPORTED_VERSIONS.to_vec(),
        identity: selected.identity.clone(),
        required_apis: selected.required_apis,
        optional_apis: selected.optional_apis,
    };

    let answer = answer_hello(&id, &hello, &worker).unwrap();
    let negotiated = accept_answer(&id, &hello, &answer, &worker.identity, &[]).unwrap();
    hello.apis.reverse();
    let reordered = answer_hello(&id, &hello, &worker).unwrap();
    assert_eq!(reordered, answer);
    assert_eq!(
        accept_answer(&id, &hello, &reordered, &worker.identity, &[]),
        Ok(negotiated)
    );

    for (apis, extensions) in [(64, 16), (65, 16), (64, 17), (1024, 16)] {
        hello.apis = (0..apis)
            .map(|n| Name::new(&format!("api-{n}")).unwrap())
            .collect();
        hello.extensions = (0..extensions)
            .map(|n| Name::new(&format!("ext-{n}")).unwrap())
            .collect();
        let message = WireMessage::Bootstrap(BootstrapMessage::Hello {
            id: id.clone(),
            hello: hello.clone(),
        });
        let body = message.encode();
        assert!(body.len() < DecodeLimits::BOOTSTRAP.max_bytes);
        let expected = if apis <= 64 && extensions <= 16 {
            Ok(message)
        } else {
            Err(Refusal::new(
                ErrorCode::InvalidMessage,
                Stage::Schema,
                Predicate::SBounds,
                "array-length",
            ))
        };
        assert_eq!(
            WireMessage::decode(&body, DecodeLimits::BOOTSTRAP),
            expected
        );
    }
}

fn active_session() -> Session {
    let WireMessage::Bootstrap(BootstrapMessage::Hello { id, hello }) = common::message("hello")
    else {
        panic!("expected hello fixture");
    };
    let WireMessage::Bootstrap(answer) = common::message("hello-selected") else {
        panic!("expected selected fixture");
    };
    let BootstrapMessage::Selected { selected, .. } = &answer else {
        panic!("expected selection");
    };
    let negotiated = accept_answer(&id, &hello, &answer, &selected.identity, &[]).unwrap();
    let mut session = Session::new(Token::new("s:1").unwrap(), negotiated);
    for (sender, stem) in [
        (Sender::Host, "initialize"),
        (Sender::Plugin, "result-initialize"),
        (Sender::Host, "invoke-before-pack-output-written"),
    ] {
        session
            .observe(sender, &common::session_message(stem))
            .unwrap();
    }
    session
}

#[test]
fn stream_refuses_regressed_sequences_foreign_ids_and_data_after_terminal_response() {
    let progress = |sequence, invocation| {
        let mut message = common::session_message("event-progress");
        let SessionBody::Event {
            seq,
            event: Event::Progress(event),
        } = &mut message.body
        else {
            panic!("expected progress fixture");
        };
        *seq = sequence;
        event.seq = sequence;
        event.invocation = Token::new(invocation).unwrap();
        message
    };

    let mut session = active_session();
    session
        .observe(Sender::Plugin, &progress(1, "op:1"))
        .unwrap();
    session
        .observe(Sender::Plugin, &progress(3, "op:1"))
        .unwrap();
    for sequence in [2, 3] {
        let refusal = session
            .observe(Sender::Plugin, &progress(sequence, "op:1"))
            .unwrap_err();
        assert_eq!(
            (refusal.code, refusal.reason),
            (ErrorCode::InvalidMessage, "event-sequence")
        );
    }
    let refusal = session
        .observe(Sender::Plugin, &progress(4, "op:foreign"))
        .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::InvalidMessage, "unknown-invocation")
    );
    session
        .observe(Sender::Plugin, &progress(4, "op:1"))
        .unwrap();

    let final_response = common::with_id(common::session_message("result-invoke"), "h:invoke");
    session.observe(Sender::Plugin, &final_response).unwrap();
    let refusal = session
        .observe(Sender::Plugin, &progress(5, "op:1"))
        .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::InvalidMessage, "unsolicited-event")
    );
    let refusal = session
        .observe(Sender::Plugin, &final_response)
        .unwrap_err();
    assert_eq!(
        (refusal.code, refusal.reason),
        (ErrorCode::InvalidMessage, "duplicate-response")
    );
}

#[test]
fn refusal_codes_round_trip_with_frozen_spellings() {
    use ErrorCode as C;
    let codes = [
        (C::Denied, "Denied"),
        (C::ConsentRequired, "ConsentRequired"),
        (C::UnsupportedVersion, "UnsupportedVersion"),
        (C::UnsupportedCapability, "UnsupportedCapability"),
        (C::UnsupportedPlatform, "UnsupportedPlatform"),
        (C::InvalidConfig, "InvalidConfig"),
        (C::InvalidMessage, "InvalidMessage"),
        (C::ResourceLimit, "ResourceLimit"),
        (C::Conflict, "Conflict"),
        (C::Busy, "Busy"),
        (C::Cancelled, "Cancelled"),
        (C::Stalled, "Stalled"),
        (C::PluginExited, "PluginExited"),
        (C::ExternalProgramMissing, "ExternalProgramMissing"),
        (C::ExternalProgramFailed, "ExternalProgramFailed"),
        (C::IntegrityFailure, "IntegrityFailure"),
        (C::AuditUnavailable, "AuditUnavailable"),
        (C::EffectUnknown, "EffectUnknown"),
    ];
    assert_eq!(codes.map(|(code, _)| code).as_slice(), ErrorCode::ALL);
    for (code, spelling) in codes {
        assert_eq!(code.as_str(), spelling);
        assert_eq!(ErrorCode::parse(spelling), Some(code));
        let error = WireError::refusal(code, Phase::Invoke, Name::new("fixture").unwrap());
        let encoded = error.encode();
        assert_eq!(
            encoded
                .get("code")
                .and_then(dctx_plugin_protocol::Json::as_str),
            Some(spelling)
        );
        assert_eq!(WireError::decode(&encoded), Ok(error));
    }
}

#[test]
fn seeded_random_frame_decoding_never_panics() {
    const SEED: u64 = 0x1118_9d27_64c0_ba5e;
    let mut state = SEED;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state.to_le_bytes()[0]
    };

    for case in 0..10_240 {
        let mut input: Vec<_> = (0..next()).map(|_| next()).collect();
        // Give half the random bodies plausible prefixes so they exercise more than the size check.
        if case % 2 == 0 && input.len() >= 4 {
            let length = u32::from(next());
            input[..4].copy_from_slice(&length.to_le_bytes());
        }
        let outcome = catch_unwind(|| {
            let mut reader = FrameReader::new(input.as_slice());
            while let Ok(Some(body)) = reader.next_frame() {
                assert!((1..=BOOTSTRAP_FRAME_CAP as usize).contains(&body.len()));
                let _ = WireMessage::decode(&body, DecodeLimits::BOOTSTRAP);
            }
        });
        assert!(
            outcome.is_ok(),
            "seed {SEED:#018x}, case {case}, input {input:02x?}"
        );
    }
}
