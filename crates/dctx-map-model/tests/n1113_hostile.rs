// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile cases through the public model API. String APIs accept
//! UTF-8 by construction; malformed bytes are exercised at the JSON boundary.
//! Numeric maxima are checked without allocating coordinate-sized buffers.
#![forbid(unsafe_code)]

use dctx_map_model::{adapters::*, *};

fn span(start: u64, end: u64) -> Result<ByteSpan, InvalidByteSpan> {
    ByteSpan::new(ByteOffset::new(start), ByteOffset::new(end))
}

fn record(name: &str) -> Record {
    Record {
        id: StableId::new(NodeKind::Symbol, "sample.rs", Some(name), None).unwrap(),
        kind: "fn".into(),
        path: "sample.rs".into(),
        l: [1, 1],
        sum: name.into(),
        src: SummarySource::Signature,
        relation: None,
        stale: false,
        signature: None,
        incoming: vec![],
        outgoing: vec![],
        tests: vec![],
        documents: vec![],
        body: None,
        score: 0,
    }
}

fn answer() -> Answer {
    Answer {
        place: "hostile".into(),
        rev: "abcdef0".into(),
        dirty: 0,
        age_seconds: 0,
        verb: Verb::Find,
        records: vec![record("alpha"), record("beta")],
        more: 0,
        next: "dctx map find next --page 2".into(),
        identity: None,
        reason: None,
    }
}

fn formats() -> Vec<Format> {
    let mut formats = vec![Format::Text];
    if cfg!(feature = "wire-json") {
        formats.push(Format::Json);
    }
    formats
}

fn rendered(a: &Answer, budget: AnswerBudget, format: Format) -> Result<Vec<u8>, String> {
    render(a, budget, format)
        .map(String::into_bytes)
        .map_err(|error| error.to_string())
}

#[test]
fn spans_project_multibyte_final_scalars_without_panicking() {
    for text in ["é", "中", "🦀", "a\né", "é\n中\n🦀", "\r\né"] {
        let length = u64::try_from(text.len()).unwrap();
        let whole = ByteSpan::on_utf8(text, ByteOffset::new(0), ByteOffset::new(length)).unwrap();
        assert_eq!(
            whole.lines_in(text).unwrap(),
            [1, u32::try_from(text.lines().count()).unwrap()]
        );
    }
}

#[test]
fn every_span_and_truncated_utf8_boundary_is_checked() {
    let bytes = "a\né🦀\nz".as_bytes();
    for cut in 0..=bytes.len() {
        // This API receives &str: decoding refuses a cut inside a scalar first.
        let Ok(text) = std::str::from_utf8(&bytes[..cut]) else {
            continue;
        };
        for start in 0..=text.len() + 1 {
            for end in 0..=text.len() + 1 {
                let start64 = u64::try_from(start).unwrap();
                let end64 = u64::try_from(end).unwrap();
                let result =
                    ByteSpan::on_utf8(text, ByteOffset::new(start64), ByteOffset::new(end64));
                let expected = if end < start {
                    Err(InvalidByteSpan::Inverted)
                } else if end > text.len() {
                    Err(InvalidByteSpan::OutOfRange)
                } else if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                    Err(InvalidByteSpan::NotUtf8Boundary)
                } else {
                    let line = |offset| {
                        u32::try_from(
                            text.as_bytes()[..offset]
                                .iter()
                                .filter(|b| **b == b'\n')
                                .count(),
                        )
                        .unwrap()
                            + 1
                    };
                    Ok([
                        line(start),
                        line(if start == end { start } else { end - 1 }),
                    ])
                };
                assert_eq!(result.map(|s| s.lines_in(text).unwrap()), expected);
                if let Ok(s) = span(start64, end64) {
                    assert_eq!(s.start().get(), start64);
                    assert_eq!(s.end().get(), end64);
                    assert_eq!(s.len().get(), end64 - start64);
                    assert_eq!(s.is_empty(), start == end);
                    assert_eq!(s.lines_in(text), expected);
                }
            }
        }
    }
}

#[test]
fn coordinate_wrappers_preserve_zero_and_maximum_and_refuse_over_text() {
    for value in [0, 1, u64::MAX - 1, u64::MAX] {
        assert_eq!(ByteOffset::new(value).get(), value);
        assert_eq!(ByteLength::new(value).get(), value);
        assert_eq!(Generation::new(value).get(), value);
    }
    let maximum = span(0, u64::MAX).unwrap();
    assert_eq!(maximum.len().get(), u64::MAX);
    assert_eq!(maximum.lines_in(""), Err(InvalidByteSpan::OutOfRange));
    assert_eq!(span(u64::MAX, 0), Err(InvalidByteSpan::Inverted));
    assert_eq!(
        span(u64::MAX, u64::MAX).unwrap().lines_in("x"),
        Err(InvalidByteSpan::OutOfRange)
    );
}

#[test]
fn stable_id_and_reference_truncations_round_trip_or_refuse() {
    for sample in [
        "",
        "x",
        "s:src/é.rs#中~4294967295",
        "@abcdef",
        "m:sample.rs",
    ] {
        for cut in 0..=sample.len() {
            let Some(text) = sample.get(..cut) else {
                continue;
            };
            let first = text.parse::<StableId>();
            assert_eq!(first, text.parse::<StableId>());
            if let Ok(id) = first {
                assert_eq!(id.as_str(), text);
                assert_eq!(id.to_string(), text);
                assert_eq!(id.alias(), id.clone().alias());
                assert_eq!(text.parse::<IdReference>().unwrap(), IdReference::Full(id));
            }
            assert_eq!(text.parse::<IdReference>(), text.parse::<IdReference>());
        }
    }
    for invalid in [
        "s:x#n~4294967296",
        "s:x#n~0",
        "s:x#n~01",
        "s:x#n~1~2",
        "s:x#n#n",
        "s:../x#n",
        "s:a//b#n",
        "m:C:x",
        "m:x\0",
        "@abcdef0",
        "@ABCDEF",
    ] {
        assert!(invalid.parse::<IdReference>().is_err(), "{invalid:?}");
    }
}

#[test]
fn stable_id_constructor_kinds_ordinals_and_alias_thresholds() {
    for kind in [
        NodeKind::Place,
        NodeKind::Package,
        NodeKind::Module,
        NodeKind::Symbol,
        NodeKind::Test,
        NodeKind::Doc,
    ] {
        let needs_name = matches!(kind, NodeKind::Symbol | NodeKind::Test | NodeKind::Doc);
        let name = needs_name.then_some("item");
        let id = StableId::new(kind, "sample.rs", name, None).unwrap();
        assert!(id.as_str().starts_with(kind.letter()));
        assert_eq!(id.as_str().parse::<StableId>().unwrap(), id);
        assert!(StableId::new(kind, "sample.rs", name, Some(0)).is_err());
        assert!(StableId::new(kind, "sample.rs", None, Some(1)).is_err());
        if needs_name {
            assert!(StableId::new(kind, "sample.rs", name, Some(u32::MAX)).is_ok());
        }
    }
    for bytes in [119, 120, 121, 4096, 4097] {
        // Stable ids have no length ceiling; 120 is the alias threshold.
        let id: StableId = format!("m:{}", "a".repeat(bytes - 2)).parse().unwrap();
        assert_eq!(id.as_str().len(), bytes);
        assert_eq!(id.alias().is_some(), bytes > 120);
        if let Some(alias) = id.alias() {
            assert_eq!(alias.len(), 7);
            assert_eq!(
                alias.parse::<IdReference>().unwrap(),
                IdReference::Alias(alias)
            );
        }
    }
}

#[test]
fn content_bytes_and_text_have_deterministic_distinct_identities() {
    for bytes in [b"".as_slice(), b"x", b"\xff", b"\xc3\x28", b"xyxy", b"xyyx"] {
        let content = ContentId::of_bytes(bytes).unwrap();
        assert_eq!(content, ContentId::of_bytes(bytes).unwrap());
        assert_eq!(content.byte_length(), u64::try_from(bytes.len()).unwrap());
        assert_eq!(content.version(), 1);
        assert_eq!(content.algorithm().code(), 1);
        assert_eq!(content.algorithm().name(), "sha256");
        assert_eq!(content.digest_bytes().len(), 32);
        let text = content.to_string();
        assert_eq!(text.parse::<ContentId>().unwrap(), content);
        for cut in 0..text.len() {
            assert!(ContentId::parse(&text[..cut]).is_err());
        }
    }
    assert_ne!(ContentId::of_bytes(b"xyxy"), ContentId::of_bytes(b"xyyx"));
    let prefix = format!("dctx:object:v1:sha256:{}:", "a".repeat(64));
    let max = format!("{prefix}{}", u64::MAX);
    assert_eq!(ContentId::parse(&max).unwrap().byte_length(), u64::MAX);
    for tail in ["18446744073709551616", "00", "-1", "+1", "1:1", "sha256:1"] {
        assert!(ContentId::parse(&format!("{prefix}{tail}")).is_err());
    }
    for input in [
        "".into(),
        "x".into(),
        max.replace("v1:sha256", "sha256:v1"),
        format!("{prefix}1\0"),
        "a".repeat(4096),
    ] {
        assert!(ContentId::parse(&input).is_err());
    }
}

#[test]
fn evidence_labels_cover_empty_controls_and_exact_byte_limit() {
    for invalid in ["", " ", "\0", "a\nb", "a\tb", "a\u{85}b", "\u{2003}"] {
        assert_eq!(EvidenceLabel::new(invalid), Err(InvalidLabel));
    }
    for sample in ["x".repeat(MAX_LABEL_BYTES), "é".repeat(MAX_LABEL_BYTES / 2)] {
        for cut in 0..=sample.len() {
            let Some(text) = sample.get(..cut) else {
                continue;
            };
            assert_eq!(EvidenceLabel::new(text).is_ok(), !text.is_empty());
            assert_eq!(
                EvidenceLabel::new(text),
                EvidenceLabel::new(text.to_owned())
            );
        }
        assert_eq!(EvidenceLabel::new(format!("{sample}x")), Err(InvalidLabel));
        assert_eq!(EvidenceLabel::new(&sample).unwrap().as_str(), sample);
    }
}

#[test]
fn budgets_cover_the_entire_numeric_space_and_exact_utf8_bytes() {
    for tokens in 0..=u16::MAX {
        let budget = AnswerBudget::new(tokens);
        assert_eq!(budget.is_ok(), (1..=MAX_BUDGET).contains(&tokens));
        if let Ok(budget) = budget {
            assert_eq!(budget.tokens(), tokens);
            assert_eq!(budget.bytes(), usize::from(tokens) * 7 / 2);
            assert!(budget.fits(&"x".repeat(budget.bytes())));
            assert!(!budget.fits(&"x".repeat(budget.bytes() + 1)));
        }
    }
    assert_eq!(AnswerBudget::default().tokens(), DEFAULT_BUDGET);
    for verb in [
        Verb::Map,
        Verb::Find,
        Verb::Neighbors,
        Verb::Explain,
        Verb::Read,
    ] {
        assert_eq!(
            AnswerBudget::for_verb(verb).tokens(),
            if verb == Verb::Explain {
                400
            } else {
                DEFAULT_BUDGET
            }
        );
    }
    assert!(!AnswerBudget::new(1).unwrap().fits("éé"));
}

#[test]
fn summaries_reject_controls_and_blank_text_and_respect_character_limit() {
    let fallback = Summary {
        text: "fallback".into(),
        source: SummarySource::Signature,
    };
    for text in [
        "".into(),
        " ".into(),
        "\t".into(),
        "bad\0text".into(),
        "bad\u{1b}[2J".into(),
        "bad\ntext".into(),
        "x".repeat(101),
        "é".repeat(101),
    ] {
        let summary = GeneratedSummary {
            fingerprint: "body".into(),
            text,
        };
        assert_eq!(summary.select("body", &fallback), fallback);
    }
    for text in ["x".into(), "x".repeat(100), "é".repeat(100)] {
        let summary = GeneratedSummary {
            fingerprint: "body".into(),
            text: text.clone(),
        };
        assert_eq!(
            summary.select("body", &fallback),
            Summary {
                text,
                source: SummarySource::Llm
            }
        );
        assert_eq!(summary.select("changed", &fallback), fallback);
        assert_eq!(
            summary.select("body", &fallback),
            summary.select("body", &fallback)
        );
    }
}

#[test]
fn precision_and_provenance_builders_are_order_independent() {
    let content = ContentId::of_bytes(b"\xff").unwrap();
    let body = ContentId::of_bytes(b"").unwrap();
    let parser = EvidenceLabel::new("parser:1").unwrap();
    for coverage in [Coverage::Syntax, Coverage::Structural, Coverage::Opaque] {
        assert!(!coverage.as_str().is_empty());
        for completion in [
            Completion::Complete,
            Completion::Partial(parser.clone()),
            Completion::Refused(parser.clone()),
        ] {
            assert!(!completion.as_str().is_empty());
            assert_eq!(
                completion.reason().is_some(),
                completion != Completion::Complete
            );
            let precision = Precision::new(coverage, completion.clone(), parser.clone());
            assert_eq!(precision.coverage(), coverage);
            assert_eq!(precision.completion(), &completion);
            assert_eq!(precision.parser(), &parser);
            let base = Provenance::new(
                content,
                precision.clone(),
                parser.clone(),
                body,
                Generation::new(u64::MAX),
            );
            assert_eq!(base.content(), content);
            assert_eq!(base.precision(), &precision);
            assert_eq!(base.extractor(), &parser);
            assert_eq!(base.extraction_plan(), body);
            assert_eq!(base.generation().get(), u64::MAX);
            assert!(base.model().is_none());
            assert!(base.body_fingerprint().is_none());
            assert_eq!(base.summary_tier(), SummaryTier::TierZero);
            assert_eq!(
                base.clone().with_model(parser.clone()).summary_tier(),
                SummaryTier::TierZero
            );
            assert_eq!(
                base.clone().with_body_fingerprint(body).summary_tier(),
                SummaryTier::TierZero
            );
            let complete = base
                .clone()
                .with_model(parser.clone())
                .with_body_fingerprint(body);
            assert_eq!(
                complete,
                base.with_body_fingerprint(body).with_model(parser.clone())
            );
            assert_eq!(complete.model(), Some(&parser));
            assert_eq!(complete.body_fingerprint(), Some(body));
            assert_eq!(complete.summary_tier(), SummaryTier::TierOne);
        }
    }
}

#[test]
fn renderers_cover_empty_single_record_duplicates_and_reordered_ties() {
    for format in formats() {
        for count in [0, 1, 2, 40, 41, 60, 61] {
            let mut a = answer();
            a.records = (0..count)
                .map(|index| record(if index % 2 == 0 { "alpha" } else { "beta" }))
                .collect();
            // Equal score and id, different contents: tie-break on contents.
            if let Some(first) = a.records.first_mut() {
                first.sum = "different".into();
            }
            let budget = AnswerBudget::new(MAX_BUDGET).unwrap();
            let output = rendered(&a, budget, format).unwrap();
            assert!(output.len() <= budget.bytes());
            assert_eq!(rendered(&a, budget, format).unwrap(), output);
            a.records.reverse();
            assert_eq!(rendered(&a, budget, format).unwrap(), output);
        }
        let a = answer();
        let expected = render(&a, AnswerBudget::default(), format).unwrap();
        let convenience = match format {
            Format::Text => render_text(&a, AnswerBudget::default()),
            Format::Json => render_json(&a, AnswerBudget::default()),
        }
        .unwrap();
        assert_eq!(expected.as_bytes(), convenience.as_bytes());
    }
}

#[test]
fn renderer_limits_and_every_metadata_prefix_are_safe() {
    for format in formats() {
        for sample in ["", "x", "abcdef0", "abcdef012345", "é🦀\0"] {
            for cut in 0..=sample.len() {
                let Some(prefix) = sample.get(..cut) else {
                    continue;
                };
                let mut a = answer();
                a.rev = prefix.into();
                assert_eq!(
                    rendered(&a, AnswerBudget::default(), format).is_ok(),
                    prefix.len() >= 7 && prefix.bytes().all(|b| b.is_ascii_hexdigit())
                );
                a = answer();
                a.place = prefix.into();
                assert_eq!(
                    rendered(&a, AnswerBudget::default(), format).is_ok(),
                    !prefix.trim().is_empty()
                        && !prefix.chars().any(|c| c.is_whitespace() || c.is_control())
                );
            }
        }
        let a = answer();
        for tokens in [1, 2, 100, MAX_BUDGET] {
            let budget = AnswerBudget::new(tokens).unwrap();
            match render(&a, budget, format) {
                Ok(output) => assert!(budget.fits(&output)),
                Err(error) => assert!(matches!(error, RenderError::BudgetTooSmall)),
            }
        }
        let mut overflow = answer();
        overflow.more = usize::MAX;
        overflow.records = vec![record("a"); 41];
        assert!(matches!(
            render(&overflow, AnswerBudget::default(), format),
            Err(RenderError::CountOverflow)
        ));
        let mut a = answer();
        a.records.clear();
        a.next.clear();
        assert!(matches!(
            render(&a, AnswerBudget::default(), format),
            Err(RenderError::MissingNext)
        ));
        for bad in ["\0", "next\ncommand", "next\tcommand"] {
            a.next = bad.into();
            assert!(matches!(
                render(&a, AnswerBudget::default(), format),
                Err(RenderError::InvalidMetadata)
            ));
        }
    }
}

#[test]
fn renderer_read_and_explain_limits_are_inclusive() {
    for format in formats() {
        for lines in [1, 399, 400, 401] {
            let mut a = answer();
            a.verb = Verb::Read;
            a.records.truncate(1);
            a.records[0].l = [1, lines];
            a.records[0].body = Some("x\n".repeat(usize::try_from(lines).unwrap()));
            assert_eq!(
                render(&a, AnswerBudget::new(MAX_BUDGET).unwrap(), format).is_ok(),
                lines <= 400
            );
            a.records[0].l = [1, 1];
            assert_eq!(
                render(&a, AnswerBudget::new(MAX_BUDGET).unwrap(), format).is_ok(),
                lines <= 400
            );
        }
        for lines in [[0, 1], [2, 1], [u32::MAX, u32::MAX - 1]] {
            let mut a = answer();
            a.records[0].l = lines;
            assert!(matches!(
                render(&a, AnswerBudget::default(), format),
                Err(RenderError::InvalidRecord)
            ));
        }
        let mut a = answer();
        a.verb = Verb::Explain;
        a.records[0].sum = "é".repeat(2000);
        let output = render(&a, AnswerBudget::new(MAX_BUDGET).unwrap(), format).unwrap();
        assert!(output.len() <= 3500);
    }
}

#[test]
fn renderer_checks_relation_confidence_and_edge_count_limits() {
    for format in formats() {
        for kind in [
            EdgeKind::Contains,
            EdgeKind::Imports,
            EdgeKind::Calls,
            EdgeKind::Implements,
            EdgeKind::UsesType,
            EdgeKind::Tests,
            EdgeKind::Documents,
            EdgeKind::Reexports,
        ] {
            assert!(!kind.as_str().is_empty());
            for confidence in [Confidence::Exact, Confidence::Name, Confidence::Guess] {
                assert!(!confidence.as_str().is_empty());
                let mut a = answer();
                a.verb = Verb::Neighbors;
                a.records.truncate(1);
                a.records[0].relation = Some(Relation {
                    edge: kind,
                    confidence,
                    incoming: false,
                });
                assert_eq!(
                    render(&a, AnswerBudget::default(), format).is_ok(),
                    kind.permits(confidence)
                );
                for incoming in [false, true] {
                    a.verb = Verb::Explain;
                    a.records[0].relation = None;
                    let edge = Edge {
                        src: a.records[0].id.clone(),
                        dst: a.records[0].id.clone(),
                        kind,
                        conf: confidence,
                        owner_path: "sample.rs".into(),
                    };
                    for count in [0, 1, 5, 6] {
                        let r = &mut a.records[0];
                        r.incoming.clear();
                        r.outgoing.clear();
                        if incoming {
                            r.incoming = vec![edge.clone(); count];
                        } else {
                            r.outgoing = vec![edge.clone(); count];
                        }
                        let result = render(&a, AnswerBudget::new(MAX_BUDGET).unwrap(), format);
                        assert_eq!(
                            result.is_ok(),
                            count == 0 || (count <= 5 && kind.permits(confidence))
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn identity_notices_require_consistent_records_and_continuation() {
    for format in formats() {
        let mut a = answer();
        let old = record("old").id;
        let new = a.records[0].id.clone();
        a.identity = Some(IdentityStatus::Moved {
            old: old.clone(),
            new,
        });
        assert!(render(&a, AnswerBudget::default(), format).is_ok());
        a.records.clear();
        assert!(matches!(
            render(&a, AnswerBudget::default(), format),
            Err(RenderError::InvalidRecord)
        ));
        a.identity = Some(IdentityStatus::Gone { old });
        assert!(render(&a, AnswerBudget::default(), format).is_ok());
        a.next.clear();
        assert!(matches!(
            render(&a, AnswerBudget::default(), format),
            Err(RenderError::MissingNext)
        ));
    }
}

#[test]
fn shared_model_values_render_and_hash_concurrently_identically() {
    let a = answer();
    let content = ContentId::of_bytes(b"\xff\0").unwrap();
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    assert_eq!(ContentId::of_bytes(b"\xff\0").unwrap(), content);
                    for format in formats() {
                        assert_eq!(
                            rendered(&a, AnswerBudget::default(), format),
                            rendered(&a, AnswerBudget::default(), format)
                        );
                    }
                    render_text(&a, AnswerBudget::default())
                        .unwrap()
                        .into_bytes()
                })
            })
            .collect();
        let expected = render_text(&a, AnswerBudget::default())
            .unwrap()
            .into_bytes();
        for worker in workers {
            assert_eq!(worker.join().unwrap(), expected);
        }
    });
}

#[cfg(feature = "wire-json")]
#[test]
fn json_deserialization_rejects_invalid_utf8_duplicate_fields_and_all_truncations() {
    fn round_trip<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
    {
        let bytes = serde_json::to_vec(value).unwrap();
        assert_eq!(serde_json::from_slice::<T>(&bytes).unwrap(), *value);
        assert_eq!(serde_json::to_vec(value).unwrap(), bytes);
        for cut in 0..bytes.len() {
            let _ = serde_json::from_slice::<T>(&bytes[..cut]);
        }
        for invalid in [b"".as_slice(), b"x", b"\xff", b"\"\xc3\x28\""] {
            assert!(serde_json::from_slice::<T>(invalid).is_err());
        }
    }
    round_trip(&AnswerBudget::new(MAX_BUDGET).unwrap());
    round_trip(&record("item").id);
    round_trip(&record("item"));
    round_trip(&Relation {
        edge: EdgeKind::Calls,
        confidence: Confidence::Name,
        incoming: true,
    });
    round_trip(&IdentityStatus::Gone {
        old: record("old").id,
    });
    round_trip(&IdentityStatus::Moved {
        old: record("old").id,
        new: record("new").id,
    });
    round_trip(&Summary {
        text: "é\0".into(),
        source: SummarySource::Doc,
    });
    round_trip(&GeneratedSummary {
        fingerprint: "body".into(),
        text: "text".into(),
    });
    round_trip(&Edge {
        src: record("old").id,
        dst: record("new").id,
        kind: EdgeKind::Calls,
        conf: Confidence::Name,
        owner_path: "sample.rs".into(),
    });
    let node = Node {
        id: record("node").id,
        kind: NodeKind::Symbol,
        symbol_kind: Some(SymbolKind::Function),
        path: "sample.rs".into(),
        lines: [1, 1],
        name: "node".into(),
        signature: "fn node()".into(),
        fingerprint: "body".into(),
        public: false,
        summary: Summary {
            text: "node".into(),
            source: SummarySource::Signature,
        },
    };
    round_trip(&node);
    assert_eq!(Record::from(&node).id, node.id);
    assert_eq!(Record::from(&node).kind, "fn");
    let mut module = node.clone();
    module.symbol_kind = None;
    module.kind = NodeKind::Module;
    assert_eq!(Record::from(&module).kind, "module");
    round_trip(&Verb::Read);
    round_trip(&NodeKind::Module);
    round_trip(&SymbolKind::Function);
    round_trip(&Confidence::Exact);
    round_trip(&EdgeKind::Contains);
    round_trip(&SummarySource::Heading);
    for bytes in [b"0".as_slice(), b"2001", b"65536"] {
        assert!(serde_json::from_slice::<AnswerBudget>(bytes).is_err());
    }
    let bytes = serde_json::to_string(&record("item")).unwrap();
    let duplicate = bytes.replacen("{", "{\"kind\":\"fn\",", 1);
    assert!(serde_json::from_str::<Record>(&duplicate).is_err());
    let mut value = serde_json::to_value(record("item")).unwrap();
    let object = value.as_object_mut().unwrap();
    let ordered = serde_json::to_string(object).unwrap();
    let reversed = object
        .iter()
        .rev()
        .map(|(key, value)| format!("{}:{}", serde_json::to_string(key).unwrap(), value))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        serde_json::from_str::<Record>(&ordered).unwrap(),
        serde_json::from_str::<Record>(&format!("{{{reversed}}}")).unwrap()
    );
}

#[cfg(not(feature = "wire-json"))]
#[test]
fn disabled_json_has_an_actionable_refusal() {
    assert!(matches!(
        render_json(&answer(), AnswerBudget::default()),
        Err(RenderError::WireJsonDisabled)
    ));
}

#[test]
fn refusal_messages_name_the_corrective_action() {
    let errors: Vec<(Box<dyn std::error::Error>, &str)> = vec![
        (Box::new(InvalidBudget), "choose"),
        (Box::new(InvalidId), "use"),
        (Box::new(InvalidLabel), "supply"),
        (Box::new(InvalidContentId::UnsupportedIdentity), "use"),
        (Box::new(InvalidContentId::MessageTooLong), "split"),
        (Box::new(InvalidByteSpan::Inverted), "set"),
        (Box::new(InvalidByteSpan::OutOfRange), "bound"),
        (Box::new(InvalidByteSpan::NotUtf8Boundary), "move"),
        (Box::new(InvalidByteSpan::LineOverflow), "split"),
        (Box::new(RenderError::BudgetTooSmall), "increase"),
        (Box::new(RenderError::MissingNext), "set"),
        (Box::new(RenderError::InvalidRecord), "correct"),
        (Box::new(RenderError::InvalidMetadata), "supply"),
        (Box::new(RenderError::CountOverflow), "reduce"),
    ];
    for (error, action) in errors {
        let text = error.to_string();
        assert!(text.contains(action), "{text}");
        assert_eq!(text.as_bytes(), error.to_string().as_bytes());
        assert!(error.source().is_none());
    }
    #[cfg(feature = "wire-json")]
    {
        let error: RenderError = serde_json::from_str::<Record>("{").unwrap_err().into();
        assert!(error.to_string().contains("correct"));
        assert!(std::error::Error::source(&error).is_some());
    }
    #[cfg(not(feature = "wire-json"))]
    assert!(RenderError::WireJsonDisabled.to_string().contains("enable"));
}

#[test]
fn text_metadata_does_not_emit_terminal_control_characters() {
    let mut a = answer();
    a.records.truncate(1);
    a.records[0].sum = "safe\0text\u{1b}[2J\nnext".into();
    a.records[0].signature = Some("fn\u{1b} example()".into());
    a.reason = Some("retry\u{7}later".into());
    for verb in [Verb::Find, Verb::Explain] {
        a.verb = verb;
        let text = render_text(&a, AnswerBudget::default()).unwrap();
        assert!(
            !text.chars().any(|c| c.is_control() && c != '\n'),
            "{text:?}"
        );
        assert!(text.contains("safe text [2J next"));
    }
    // Read bodies are explicitly verbatim source; metadata normalization must
    // not alter their bytes or indentation.
    a.verb = Verb::Read;
    a.records[0].body = Some("\tverbatim\0\n".into());
    assert!(
        render_text(&a, AnswerBudget::default())
            .unwrap()
            .contains("\tverbatim\0\n")
    );
}

#[test]
fn graph_vocabulary_and_node_conversion_preserve_public_fields() {
    for kind in [
        NodeKind::Place,
        NodeKind::Package,
        NodeKind::Module,
        NodeKind::Symbol,
        NodeKind::Test,
        NodeKind::Doc,
    ] {
        assert!(!kind.as_str().is_empty());
    }
    for source in [
        SummarySource::Doc,
        SummarySource::Signature,
        SummarySource::Heading,
        SummarySource::Llm,
    ] {
        assert!(!source.as_str().is_empty());
        for symbol_kind in [
            None,
            Some(SymbolKind::Function),
            Some(SymbolKind::Method),
            Some(SymbolKind::Struct),
            Some(SymbolKind::Enum),
            Some(SymbolKind::Trait),
            Some(SymbolKind::Impl),
            Some(SymbolKind::Const),
            Some(SymbolKind::Type),
            Some(SymbolKind::Class),
            Some(SymbolKind::Interface),
            Some(SymbolKind::Macro),
        ] {
            let node = Node {
                id: record("item").id,
                kind: NodeKind::Symbol,
                symbol_kind,
                path: "sample.rs".into(),
                lines: [1, u32::MAX],
                name: "item".into(),
                signature: "signature".into(),
                fingerprint: "body".into(),
                public: true,
                summary: Summary {
                    text: "é".into(),
                    source,
                },
            };
            let converted = Record::from(&node);
            assert_eq!(converted.id, node.id);
            assert_eq!(
                converted.kind,
                symbol_kind.map_or("symbol", SymbolKind::as_str)
            );
            assert_eq!(converted.path, node.path);
            assert_eq!(converted.l, node.lines);
            assert_eq!(converted.sum, node.summary.text);
            assert_eq!(converted.src, source);
            assert_eq!(converted, Record::from(&node));
        }
    }
}

#[test]
fn every_string_field_prefix_is_repeatable_and_bounded() {
    let sample = "x é🦀\n\0\u{1b}y";
    for format in formats() {
        for cut in 0..=sample.len() {
            let Some(prefix) = sample.get(..cut) else {
                continue;
            };
            let mut a = answer();
            a.records.truncate(1);
            a.records[0].sum = prefix.into();
            a.records[0].path = prefix.into();
            a.records[0].signature = Some(prefix.into());
            a.records[0].body = Some(prefix.into());
            a.reason = Some(prefix.into());
            for verb in [Verb::Map, Verb::Find, Verb::Explain, Verb::Read] {
                a.verb = verb;
                let budget = AnswerBudget::new(MAX_BUDGET).unwrap();
                let output = rendered(&a, budget, format).unwrap();
                assert!(output.len() <= budget.bytes());
                assert_eq!(rendered(&a, budget, format).unwrap(), output);
            }
        }
    }
}

#[test]
fn constructor_names_cannot_impersonate_duplicate_ordinals() {
    let ordinal = StableId::new(NodeKind::Symbol, "sample.rs", Some("item"), Some(1)).unwrap();
    assert_eq!(ordinal.as_str(), "s:sample.rs#item~1");
    assert!(StableId::new(NodeKind::Symbol, "sample.rs", Some("item~1"), None).is_err());
    assert_eq!(ordinal.as_str().parse::<StableId>().unwrap(), ordinal);
}
