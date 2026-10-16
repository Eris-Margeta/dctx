//! Property tests: frame fuzzing, random split reads of back-to-back fixture frames, body mutation fuzzing
//! and canonical encode/decode round trips of arbitrary bounded JSON trees.
//! Implements W-FRAME/W-JSON robustness: arbitrary input never panics, splits never change messages.
//! Depends on proptest (dev-only) and the corpus fixtures.

mod common;

use std::collections::BTreeMap;

use common::*;
use dctx_plugin_protocol::{
    DecodeLimits, FrameDecoder, FrameReader, Json, MAX_FRAME_CAP, WireMessage, decode_object, encode_frame,
    to_canonical,
};
use proptest::prelude::*;

const STEMS: [&str; 8] = [
    "request-file-read",
    "result-file-read",
    "event-progress",
    "initialize",
    "invoke-command",
    "error-effect-partial",
    "request-state-write",
    "command-result",
];

fn stream() -> (Vec<u8>, Vec<Vec<u8>>) {
    let bodies: Vec<Vec<u8>> = STEMS.iter().map(|stem| fixture_bytes(stem)).collect();
    let mut stream = Vec::new();
    for body in &bodies {
        stream.extend(encode_frame(body, MAX_FRAME_CAP).expect("frame"));
    }
    (stream, bodies)
}

fn json_tree() -> impl Strategy<Value = Json> {
    let leaf = prop_oneof![
        Just(Json::Null),
        any::<bool>().prop_map(Json::Bool),
        (-1_000_000i64..9_007_199_254_740_991).prop_map(Json::Int),
        "\\PC{0,12}".prop_map(|text: String| Json::Str(text.replace('\0', "")))
    ];
    leaf.prop_recursive(4, 48, 6, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Json::Array),
            prop::collection::btree_map("[a-z\u{e9}\"\\\\\n]{0,6}", inner, 0..6)
                .prop_map(|map: BTreeMap<String, Json>| Json::Object(map.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_bytes_never_panic(input in prop::collection::vec(any::<u8>(), 0..512)) {
        let mut decoder = FrameDecoder::new();
        if let Ok(bodies) = decoder.push_all(&input) {
            for body in bodies {
                let _ = WireMessage::decode(&body, DecodeLimits::BOOTSTRAP);
            }
            let _ = decoder.finish();
        }
        let _ = WireMessage::decode(&input, DecodeLimits::WIRE);
    }

    #[test]
    fn random_splits_reconstruct_every_frame(cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..24)) {
        let (stream, bodies) = stream();
        let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(stream.len() + 1)).collect();
        points.push(0);
        points.push(stream.len());
        points.sort_unstable();
        let mut decoder = FrameDecoder::new();
        decoder.set_cap(MAX_FRAME_CAP).expect("cap");
        let mut seen = Vec::new();
        for pair in points.windows(2) {
            seen.extend(decoder.push_all(&stream[pair[0]..pair[1]]).expect("valid stream"));
        }
        prop_assert!(decoder.finish().is_ok());
        prop_assert_eq!(&seen, &bodies);
        for (body, stem) in seen.iter().zip(STEMS) {
            prop_assert_eq!(WireMessage::decode(body, DecodeLimits::WIRE), Ok(message(stem)));
        }
    }

    #[test]
    fn trickled_reader_sees_the_same_frames(step in 1usize..40) {
        let (stream, bodies) = stream();
        struct Steps { data: Vec<u8>, pos: usize, step: usize }
        impl std::io::Read for Steps {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let count = self.step.min(buf.len()).min(self.data.len() - self.pos);
                buf[..count].copy_from_slice(&self.data[self.pos..self.pos + count]);
                self.pos += count;
                Ok(count)
            }
        }
        let mut reader = FrameReader::new(Steps { data: stream, pos: 0, step });
        reader.decoder_mut().set_cap(MAX_FRAME_CAP).expect("cap");
        let mut seen = Vec::new();
        while let Some(body) = reader.next_frame().expect("valid stream") {
            seen.push(body);
        }
        prop_assert_eq!(seen, bodies);
    }

    #[test]
    fn mutated_bodies_never_panic_and_accepted_ones_are_stable(
        stem in prop::sample::select(STEMS.to_vec()),
        flips in prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 1..4),
    ) {
        let mut body = fixture_bytes(stem);
        for (index, byte) in flips {
            let at = index.index(body.len());
            body[at] = byte;
        }
        if let Ok(message) = WireMessage::decode(&body, DecodeLimits::WIRE) {
            let again = WireMessage::decode(&message.encode(), DecodeLimits::WIRE);
            prop_assert_eq!(again, Ok(message));
        }
    }

    #[test]
    fn canonical_round_trip_of_arbitrary_trees(members in prop::collection::btree_map("[a-z]{1,4}", json_tree(), 0..6)) {
        let value = Json::Object(members.into_iter().collect());
        let canonical = to_canonical(&value);
        let limits = DecodeLimits { max_bytes: 1 << 20, max_depth: 64, max_members: 1 << 16 };
        let decoded = decode_object(&canonical, limits).expect("canonical output decodes");
        prop_assert!(decoded.same_value(&value));
        prop_assert_eq!(to_canonical(&decoded), canonical);
    }
}
