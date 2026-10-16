//! Round trips for facts, postings, adjacency, and ranking inputs.
//! Implements SEGMENTS-v1 and the DX-C1 exit checks. Depends on the builder and decoder.
//! Insertion order is not part of the bytes. Equal drafts encode to equal files.
//! Borrowed strings point into the decoded segment. Ranking revision 1 has no recency term.

use std::cmp::Ordering;

use crate::adapters::MemorySegment;
use crate::ports::load_bounded;
use crate::{
    average_document_tokens, cmp_ranked, decode, rank_score, Completion, Confidence, Coverage,
    DeclarationDraft, DeclarationKind, DecoderLimits, DocId, EdgeDraft, EdgeEnd, EdgeKind, Fact,
    FactDraft, Field, FieldHit, Language, NodeDraft, PostingDraft, RankInput, RawRefDraft,
    SegmentBuilder, SegmentError, SegmentRole, SummarySource, Target, TermDraft, ValidatedSegment,
    ViewKeyDraft, RANKING_REVISION,
};

#[test]
fn fact_segment_round_trips_postings_prefixes_and_ranking() {
    let mut builder = SegmentBuilder::fact();
    builder.add_fact(rich([2; 32], "beta", false)).unwrap();
    builder.add_fact(rich([1; 32], "alpha", true)).unwrap();
    builder.set_lengths(vec![4, 4]).unwrap();
    builder.add_term(identifier("alpha", vec![(1, vec![2]), (0, vec![1, 0])])).unwrap();
    builder
        .add_term(TermDraft {
            field: Field::Doc,
            term: "note".to_owned(),
            hits: vec![PostingDraft { doc: DocId::Index(0), positions: vec![0] }],
        })
        .unwrap();
    let bytes = builder.build().unwrap();
    assert_eq!(bytes, builder.build().unwrap());
    let loaded = load_bounded(&MemorySegment::new(bytes.clone()), &DecoderLimits::standard()).unwrap();
    assert_eq!(loaded, bytes);
    let segment = decode(&bytes, SegmentRole::Fact).unwrap();
    assert_eq!(segment.role(), SegmentRole::Fact);
    assert_eq!(segment.minor(), 0);
    assert_eq!(segment.bytes(), bytes.as_slice());
    assert!(segment.nodes().is_empty());
    assert!(segment.view_key().is_none());
    assert!(segment.rows_for(Field::Path).is_empty());
    assert!(!segment.prefixes().is_empty());
    assert_prefixes(&segment);
    assert_eq!(segment.facts().len(), 2);
    assert_eq!(segment.facts()[0].key.digest, [1; 32]);
    assert_eq!(segment.facts()[1].key.digest, [2; 32]);
    assert_rich(&segment, &segment.facts()[0], &rich([1; 32], "alpha", true));
    assert_rich(&segment, &segment.facts()[1], &rich([2; 32], "beta", false));
    let alpha = segment.term(Field::Identifier, "alpha").unwrap();
    assert_eq!(alpha.hits[0].doc, 0);
    assert_eq!(alpha.hits[0].positions, vec![0, 1]);
    assert_eq!(alpha.hits[1].doc, 1);
    assert_eq!(alpha.hits[1].positions, vec![2]);
    let note = segment.term(Field::Doc, "note").unwrap();
    assert_eq!(note.hits, vec![crate::TermHit { doc: 0, positions: vec![0] }]);
    assert_eq!(segment.rows_for(Field::Identifier)[0].field, Field::Identifier);
    assert_eq!(segment.rows_for(Field::Doc)[0].field, Field::Doc);
    assert_eq!(average_document_tokens(&segment.postings().lengths), 4);
    let hits = [FieldHit {
        field: Field::Identifier,
        term_frequency: 2,
        document_frequency: 2,
    }];
    let base = RankInput {
        document_count: 2,
        average_tokens: 4,
        document_tokens: 4,
        hits: &hits,
        exact_name: false,
        public: false,
        tier0: false,
        degree: 0,
        focus: false,
    };
    assert_eq!(RANKING_REVISION, 1);
    assert_eq!(rank_score(&base).unwrap(), 0);
    assert_eq!(rank_score(&RankInput { exact_name: true, ..base }).unwrap(), 2_000_000);
    assert_eq!(rank_score(&base).unwrap(), rank_score(&base).unwrap());
    assert_eq!(cmp_ranked(0, "b", 0, "a"), Ordering::Greater);
    let digest = crate::segment_object_digest(&bytes).unwrap();
    assert_ne!(digest, crate::envelope_checksum(&bytes).unwrap());
    assert_eq!(crate::segment_object_digest(&bytes).unwrap(), digest);
}

#[test]
fn insertion_order_does_not_change_segment_bytes() {
    let forward = build_pair(false);
    let reverse = build_pair(true);
    assert_eq!(forward, reverse);
    let decoded = decode(&forward, SegmentRole::Fact).unwrap();
    assert_eq!(decoded.facts()[0].key.digest, [1; 32]);
    assert_eq!(decoded.string(decoded.facts()[0].declarations[0].name).unwrap(), "alpha");
}

#[test]
fn view_segment_round_trips_adjacency_and_path_postings() {
    let bytes = view_bytes(false);
    assert_eq!(bytes, view_bytes(true));
    let segment = decode(&bytes, SegmentRole::View).unwrap();
    assert_eq!(segment.role(), SegmentRole::View);
    assert!(segment.facts().is_empty());
    assert_eq!(segment.nodes().len(), 2);
    assert_eq!(segment.string(segment.nodes()[0].public_id).unwrap(), "a");
    assert_eq!(segment.string(segment.nodes()[1].public_id).unwrap(), "b");
    assert_eq!(segment.string(segment.nodes()[0].path).unwrap(), "a.rs");
    assert_eq!(segment.nodes()[0].dense_id, 0);
    assert_eq!(segment.nodes()[1].local_fact_id, 2);
    assert_eq!(segment.nodes()[0].fact.digest, [7; 32]);
    assert_eq!(segment.nodes()[1].fact.language, Language::TypeScript);
    assert_eq!(segment.nodes()[1].fact.byte_length, 2);
    assert_eq!(segment.string(segment.nodes()[1].fact.dialect).unwrap(), "tsx");
    assert!(segment.nodes()[0].public);
    assert!(segment.nodes()[1].tier0);
    let key = segment.view_key().unwrap();
    assert_eq!(segment.string(key.trust).unwrap(), "ns");
    assert_eq!(segment.string(key.root).unwrap(), "root");
    assert_eq!(segment.string(key.resolver_revision).unwrap(), "rev");
    assert_eq!(key.snapshot, [3; 32]);
    assert_eq!(key.policy, [4; 32]);
    assert_eq!(key.environment, [5; 32]);
    assert_eq!(key.overlay_generation, 4);
    assert_eq!(segment.forward().len(), 2);
    assert_eq!(segment.forward()[0].source, 0);
    assert_eq!(segment.forward()[0].kind, EdgeKind::Calls);
    assert_eq!(segment.forward()[0].confidence, Confidence::Guess);
    assert_eq!(segment.forward()[0].target, Target::Unresolved(segment.find_string("missing").unwrap()));
    assert_eq!(segment.forward()[1].source, 1);
    assert_eq!(segment.forward()[1].kind, EdgeKind::Imports);
    assert_eq!(segment.forward()[1].target, Target::Resolved(0));
    assert_eq!(segment.degree(0), Some(1));
    assert_eq!(segment.degree(1), Some(1));
    assert_eq!(segment.degree(2), None);
    assert_eq!(segment.forward_from(0).len(), 1);
    assert_eq!(segment.reverse_resolved().len(), 1);
    assert_eq!(segment.reverse_resolved()[0].target, 0);
    assert_eq!(segment.reverse_resolved()[0].source, 1);
    assert_eq!(segment.reverse_unresolved().len(), 1);
    assert_eq!(segment.string(segment.reverse_unresolved()[0].name).unwrap(), "missing");
    assert_eq!(segment.reverse_unresolved()[0].source, 0);
    let path = segment.term(Field::Path, "b.rs").unwrap();
    assert_eq!(path.hits[0].doc, 1);
    assert_eq!(path.hits[0].positions, vec![0]);
    let ident = segment.term(Field::Identifier, "b").unwrap();
    assert_eq!(ident.hits[0].doc, 0);
    assert_eq!(ident.hits[0].positions, vec![0, 1]);
    assert_eq!(segment.postings().lengths, vec![2, 5]);
    assert_prefixes(&segment);
    assert_eq!(decode(&bytes, SegmentRole::Fact).unwrap_err(), SegmentError::Role("forbidden"));
}

#[test]
fn builder_refusals_keep_a_positive_control() {
    let mut facts = SegmentBuilder::fact();
    let mut wide = bare([1; 32]);
    wide.byte_length = 1;
    wide.declarations.push(declaration("a", 0, 2));
    assert_eq!(facts.add_fact(wide).unwrap_err(), SegmentError::Facts("span"));
    facts.add_fact(bare([1; 32])).unwrap();
    facts.add_fact(bare([1; 32])).unwrap();
    assert_eq!(facts.build().unwrap_err(), SegmentError::Facts("duplicate"));
    let mut distinct = SegmentBuilder::fact();
    distinct.add_fact(bare([1; 32])).unwrap();
    distinct.add_fact(bare([2; 32])).unwrap();
    assert!(distinct.build().is_ok());

    let mut terms = SegmentBuilder::fact();
    terms.set_lengths(vec![4]).unwrap();
    assert_eq!(
        terms.add_term(identifier("a", vec![(0, vec![1, 1])])).unwrap_err(),
        SegmentError::Postings("zero delta")
    );
    assert_eq!(
        terms
            .add_term(TermDraft {
                field: Field::Path,
                term: "a.rs".to_owned(),
                hits: vec![PostingDraft { doc: DocId::Index(0), positions: vec![0] }],
            })
            .unwrap_err(),
        SegmentError::Postings("path")
    );
    terms.add_term(identifier("a", vec![(5, vec![1])])).unwrap();
    assert_eq!(terms.build().unwrap_err(), SegmentError::Postings("doc"));
    let mut posted = SegmentBuilder::fact();
    posted.set_lengths(vec![4]).unwrap();
    posted.add_term(identifier("a", vec![(0, vec![1, 2])])).unwrap();
    let posted_bytes = posted.build().unwrap();
    let posted = decode(&posted_bytes, SegmentRole::Fact).unwrap();
    assert_eq!(posted.term(Field::Identifier, "a").unwrap().hits[0].positions, vec![1, 2]);

    let mut short = view_builder();
    short.add_node(node("a.rs", "a", [7; 32], Language::Rust, false)).unwrap();
    short.add_node(node("b.rs", "b", [8; 32], Language::Rust, false)).unwrap();
    short.set_lengths(vec![1]).unwrap();
    assert_eq!(short.build().unwrap_err(), SegmentError::Role("length"));
    short.set_lengths(vec![1, 1]).unwrap();
    assert!(short.build().is_ok());

    let mut edges = view_builder();
    edges.add_node(node("a.rs", "a", [7; 32], Language::Rust, false)).unwrap();
    assert_eq!(
        edges
            .add_edge(EdgeDraft {
                source: "a".to_owned(),
                kind: EdgeKind::Contains,
                confidence: Confidence::Guess,
                target: EdgeEnd::Resolved("a".to_owned()),
            })
            .unwrap_err(),
        SegmentError::Adjacency("confidence")
    );
    edges
        .add_edge(EdgeDraft {
            source: "a".to_owned(),
            kind: EdgeKind::Contains,
            confidence: Confidence::Exact,
            target: EdgeEnd::Resolved("a".to_owned()),
        })
        .unwrap();
    let held_bytes = edges.build().unwrap();
    let held = decode(&held_bytes, SegmentRole::View).unwrap();
    assert_eq!(held.forward()[0].kind, EdgeKind::Contains);
    assert_eq!(held.forward()[0].target, Target::Resolved(0));
}

fn assert_prefixes(segment: &ValidatedSegment<'_>) {
    let dictionary = segment.dictionary();
    assert!(!segment.prefixes().is_empty());
    for entry in segment.prefixes() {
        let start = usize::try_from(entry.first).unwrap();
        let last = usize::try_from(entry.last).unwrap();
        assert!(start <= last && last < dictionary.len());
        assert_eq!(&dictionary[start..=last], segment.string_range(entry.prefix));
    }
}

fn assert_rich(segment: &ValidatedSegment<'_>, fact: &Fact, draft: &FactDraft) {
    assert_eq!(fact.key.digest, draft.digest);
    assert_eq!(fact.key.byte_length, draft.byte_length);
    assert_eq!(fact.key.language, draft.language);
    assert_eq!(segment.string(fact.key.dialect).unwrap(), draft.dialect);
    assert_eq!(segment.string(fact.key.parser_id).unwrap(), draft.parser_id);
    assert_eq!(segment.string(fact.key.parser_revision).unwrap(), draft.parser_revision);
    assert_eq!(fact.key.extraction_plan, draft.extraction_plan);
    assert_eq!(fact.declarations.len(), draft.declarations.len());
    for (got, want) in fact.declarations.iter().zip(&draft.declarations) {
        assert_eq!(segment.string(got.name).unwrap(), want.name);
        assert_eq!(got.kind, want.kind);
        assert_eq!((got.span_start, got.span_end), (want.span_start, want.span_end));
        assert_eq!(segment.string(got.summary).unwrap(), want.summary);
        assert_eq!(got.summary_source, want.summary_source);
        assert_eq!(got.public, want.public);
    }
    assert_eq!(fact.raw_refs.len(), draft.raw_refs.len());
    for (got, want) in fact.raw_refs.iter().zip(&draft.raw_refs) {
        assert_eq!(segment.string(got.src).unwrap(), want.src);
        assert_eq!(got.kind, want.kind);
        assert_eq!(segment.string(got.spelling).unwrap(), want.spelling);
        assert_eq!(segment.string(got.scope).unwrap(), want.scope);
        match (&got.alias, &want.alias) {
            (Some(id), Some(text)) => assert_eq!(segment.string(*id).unwrap(), text),
            (None, None) => {}
            _ => panic!("alias mismatch"),
        }
        assert_eq!((got.line_start, got.line_end), (want.line_start, want.line_end));
    }
    assert_eq!(fact.provenance.coverage, draft.coverage);
    assert_eq!(fact.provenance.completion, draft.completion);
    assert_eq!(segment.string(fact.provenance.extractor).unwrap(), draft.extractor);
    assert_eq!(fact.provenance.generation, draft.generation);
    match draft.completion {
        Completion::Complete => assert_eq!(fact.provenance.completion_reason, 0),
        Completion::Partial | Completion::Refused => {
            assert_eq!(segment.string(fact.provenance.completion_reason).unwrap(), draft.completion_reason);
        }
    }
    match (&fact.provenance.model, &draft.model) {
        (Some(id), Some(text)) => assert_eq!(segment.string(*id).unwrap(), text),
        (None, None) => {}
        _ => panic!("model mismatch"),
    }
    assert_eq!(fact.provenance.body_fingerprint, draft.body_fingerprint);
}

fn rich(digest: [u8; 32], name: &str, partial: bool) -> FactDraft {
    let mut draft = bare(digest);
    draft.byte_length = if partial { 8 } else { 9 };
    draft.extraction_plan = if partial { [9; 32] } else { [8; 32] };
    draft.declarations.push(declaration(name, 0, draft.byte_length));
    draft.coverage = if partial { Coverage::Structural } else { Coverage::Syntax };
    draft.generation = if partial { 7 } else { 8 };
    if partial {
        draft.completion = Completion::Partial;
        draft.completion_reason = "macro".to_owned();
        draft.model = Some("none".to_owned());
        draft.raw_refs.push(RawRefDraft {
            src: name.to_owned(),
            kind: EdgeKind::Calls,
            spelling: "beta".to_owned(),
            scope: String::new(),
            alias: Some("b".to_owned()),
            line_start: 2,
            line_end: 3,
        });
    } else {
        draft.body_fingerprint = Some([4; 32]);
    }
    draft
}

fn bare(digest: [u8; 32]) -> FactDraft {
    FactDraft {
        digest,
        byte_length: 0,
        language: Language::Rust,
        dialect: "2024".to_owned(),
        parser_id: "parse".to_owned(),
        parser_revision: "rev".to_owned(),
        extraction_plan: [9; 32],
        declarations: Vec::new(),
        raw_refs: Vec::new(),
        coverage: Coverage::Syntax,
        completion: Completion::Complete,
        completion_reason: String::new(),
        extractor: "extract".to_owned(),
        generation: 1,
        model: None,
        body_fingerprint: None,
    }
}

fn declaration(name: &str, start: u64, end: u64) -> DeclarationDraft {
    DeclarationDraft {
        name: name.to_owned(),
        kind: if name == "alpha" { DeclarationKind::Function } else { DeclarationKind::Struct },
        span_start: start,
        span_end: end,
        summary: if name == "alpha" { "adds".to_owned() } else { "sub".to_owned() },
        summary_source: if name == "alpha" { SummarySource::Doc } else { SummarySource::Signature },
        public: name == "alpha",
    }
}

fn identifier(term: &str, hits: Vec<(u32, Vec<u32>)>) -> TermDraft {
    TermDraft {
        field: Field::Identifier,
        term: term.to_owned(),
        hits: hits
            .into_iter()
            .map(|(doc, positions)| PostingDraft { doc: DocId::Index(doc), positions })
            .collect(),
    }
}

fn build_pair(swap: bool) -> Vec<u8> {
    let mut builder = SegmentBuilder::fact();
    let alpha = rich([1; 32], "alpha", true);
    let beta = rich([2; 32], "beta", false);
    if swap {
        builder.add_fact(beta).unwrap();
        builder.add_fact(alpha).unwrap();
        builder.add_term(identifier("note", vec![(0, vec![0])])).unwrap();
        builder.add_term(identifier("alpha", vec![(0, vec![0])])).unwrap();
    } else {
        builder.add_fact(alpha).unwrap();
        builder.add_fact(beta).unwrap();
        builder.add_term(identifier("alpha", vec![(0, vec![0])])).unwrap();
        builder.add_term(identifier("note", vec![(0, vec![0])])).unwrap();
    }
    builder.set_lengths(vec![4, 4]).unwrap();
    builder.build().unwrap()
}

fn view_bytes(swap: bool) -> Vec<u8> {
    let mut builder = view_builder();
    let alpha = node("a.rs", "a", [7; 32], Language::Rust, false);
    let beta = node("b.rs", "b", [8; 32], Language::TypeScript, true);
    if swap {
        builder.add_node(alpha).unwrap();
        builder.add_node(beta).unwrap();
    } else {
        builder.add_node(beta).unwrap();
        builder.add_node(alpha).unwrap();
    }
    let imports = EdgeDraft {
        source: "b".to_owned(),
        kind: EdgeKind::Imports,
        confidence: Confidence::Name,
        target: EdgeEnd::Resolved("a".to_owned()),
    };
    let calls = EdgeDraft {
        source: "a".to_owned(),
        kind: EdgeKind::Calls,
        confidence: Confidence::Guess,
        target: EdgeEnd::Unresolved("missing".to_owned()),
    };
    if swap {
        builder.add_edge(imports).unwrap();
        builder.add_edge(calls).unwrap();
    } else {
        builder.add_edge(calls).unwrap();
        builder.add_edge(imports).unwrap();
    }
    builder.set_lengths(vec![2, 5]).unwrap();
    builder
        .add_term(TermDraft {
            field: Field::Path,
            term: "b.rs".to_owned(),
            hits: vec![PostingDraft { doc: DocId::Public("b".to_owned()), positions: vec![0] }],
        })
        .unwrap();
    builder
        .add_term(TermDraft {
            field: Field::Identifier,
            term: "b".to_owned(),
            hits: vec![PostingDraft { doc: DocId::Public("a".to_owned()), positions: vec![1, 0] }],
        })
        .unwrap();
    builder.build().unwrap()
}

fn view_builder() -> SegmentBuilder {
    let mut builder = SegmentBuilder::view();
    builder
        .set_view_key(ViewKeyDraft {
            trust: "ns".to_owned(),
            root: "root".to_owned(),
            snapshot: [3; 32],
            overlay_generation: 4,
            policy: [4; 32],
            resolver_revision: "rev".to_owned(),
            environment: [5; 32],
        })
        .unwrap();
    builder
}

fn node(path: &str, public_id: &str, digest: [u8; 32], language: Language, tier0: bool) -> NodeDraft {
    NodeDraft {
        path: path.to_owned(),
        public_id: public_id.to_owned(),
        digest,
        byte_length: if tier0 { 2 } else { 1 },
        language,
        dialect: if tier0 { "tsx".to_owned() } else { "d".to_owned() },
        parser_id: if tier0 { "q".to_owned() } else { "p".to_owned() },
        parser_revision: if tier0 { "r2".to_owned() } else { "r1".to_owned() },
        extraction_plan: if tier0 { [3; 32] } else { [2; 32] },
        local_fact_id: if tier0 { 2 } else { 1 },
        public: !tier0,
        tier0,
    }
}
