//! Byte-for-byte golden examples and renderer refusal controls.
//! Pins SPEC-MAP-MODE sections 3, 4.1 and 4.2.
//! Uses pure fixture records; executed as the render module's unit tests.
use super::*;

pub(super) fn record(name: &str) -> Record {
    Record {
        id: format!("s:src/lib.rs#{name}").parse().unwrap(),
        kind: "fn".into(),
        path: "src/lib.rs".into(),
        l: [41, 88],
        sum: "Appends one event.".into(),
        src: SummarySource::Doc,
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
pub(super) fn answer() -> Answer {
    Answer {
        place: "demo".into(),
        rev: "c23fd83abcdef".into(),
        dirty: 1,
        age_seconds: 2,
        verb: Verb::Find,
        records: vec![record("append")],
        more: 0,
        next: "dctx map find \"event\" --page 2".into(),
        identity: None,
        reason: None,
    }
}

#[test]
fn golden_text_and_json_are_exact_bytes() {
    let a = answer();
    let text = "# map demo rev=c23fd83+dirty 1 age=2\ns:src/lib.rs#append  fn  L41-L88  Appends one event.  [doc]\n";
    let json = "{\"v\":1,\"place\":\"demo\",\"rev\":\"c23fd83\",\"dirty\":1,\"verb\":\"find\",\"records\":[{\"id\":\"s:src/lib.rs#append\",\"kind\":\"fn\",\"path\":\"src/lib.rs\",\"l\":[41,88],\"sum\":\"Appends one event.\",\"src\":\"doc\"}],\"more\":0,\"next\":\"\"}";
    assert_eq!(render_text(&a, AnswerBudget::default()).unwrap(), text);
    assert_eq!(render_json(&a, AnswerBudget::default()).unwrap(), json);
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap().as_bytes(),
        text.as_bytes()
    );
    assert_eq!(
        render_json(&a, AnswerBudget::default()).unwrap().as_bytes(),
        json.as_bytes()
    );
}

#[test]
fn golden_cut_counts_all_omissions_and_reserves_footer() {
    let mut a = answer();
    a.records.push(record("zebra"));
    a.more = 8;
    let text = "# map demo rev=c23fd83+dirty 1 age=2\ns:src/lib.rs#append  fn  L41-L88  Appends one event.  [doc]\n+9 more · next: dctx map find \"event\" --page 2\n";
    let budget = AnswerBudget::new(text.len().div_ceil(7) as u16 * 2).unwrap();
    assert_eq!(render_text(&a, budget).unwrap(), text);
    let json = "{\"v\":1,\"place\":\"demo\",\"rev\":\"c23fd83\",\"dirty\":1,\"verb\":\"find\",\"records\":[{\"id\":\"s:src/lib.rs#append\",\"kind\":\"fn\",\"path\":\"src/lib.rs\",\"l\":[41,88],\"sum\":\"Appends one event.\",\"src\":\"doc\"}],\"more\":9,\"next\":\"dctx map find \\\"event\\\" --page 2\"}";
    let budget = AnswerBudget::new((json.len() * 2).div_ceil(7) as u16).unwrap();
    assert_eq!(render_json(&a, budget).unwrap(), json);
}

#[test]
fn golden_neighbor_directions_confidence_and_staleness() {
    let mut a = answer();
    a.verb = Verb::Neighbors;
    a.records[0].relation = Some(Relation {
        edge: EdgeKind::Calls,
        confidence: Confidence::Name,
        incoming: false,
    });
    a.records[0].stale = true;
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\ncalls→  name  s:src/lib.rs#append  fn  L41-L88  Appends one event.  [doc]  stale\n"
    );
    a.records[0].relation.as_mut().unwrap().incoming = true;
    assert!(
        render_text(&a, AnswerBudget::default())
            .unwrap()
            .contains("←calls  name")
    );
    a.records[0].relation.as_mut().unwrap().confidence = Confidence::Exact;
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::InvalidRecord)
    ));
}

#[test]
fn golden_explain_and_read() {
    let mut a = answer();
    a.verb = Verb::Explain;
    a.records[0].signature = Some("pub fn append()".into());
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\nid: s:src/lib.rs#append\nkind: fn\nsignature: pub fn append()\nlocation: src/lib.rs:L41-L88\nsummary: Appends one event.\nsource: doc\nin: \nout: \ntests: \ndocuments: \nstale: false\n"
    );
    a.verb = Verb::Read;
    a.records[0].body = Some("fn append() {\n    // exact spacing\n}\n".into());
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\ns:src/lib.rs#append  fn  L41-L88  Appends one event.  [doc]\nfn append() {\n    // exact spacing\n}\n"
    );
    a.records[0].l = [1, 401];
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::InvalidRecord)
    ));
}

#[test]
fn golden_moved_gone_and_empty_answers() {
    let mut a = answer();
    let old: StableId = "s:src/old.rs#append".parse().unwrap();
    a.identity = Some(IdentityStatus::Moved {
        old: old.clone(),
        new: a.records[0].id.clone(),
    });
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\nmoved: s:src/old.rs#append -> s:src/lib.rs#append\ns:src/lib.rs#append  fn  L41-L88  Appends one event.  [doc]\n"
    );
    let json = render_json(&a, AnswerBudget::default()).unwrap();
    assert!(json.ends_with("\"identity\":{\"status\":\"moved\",\"old\":\"s:src/old.rs#append\",\"new\":\"s:src/lib.rs#append\"}}"));
    a.records.clear();
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::InvalidRecord)
    ));
    a.identity = Some(IdentityStatus::Gone { old });
    a.next = "dctx map find \"append\"".into();
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\ngone: s:src/old.rs#append\nresolve id · try: dctx map find \"append\"\n"
    );
    a.identity = None;
    assert_eq!(
        render_text(&a, AnswerBudget::default()).unwrap(),
        "# map demo rev=c23fd83+dirty 1 age=2\nno match · try: dctx map find \"append\"\n"
    );
    a.reason = Some("index unavailable".into());
    assert!(
        render_text(&a, AnswerBudget::default())
            .unwrap()
            .contains("index unavailable · try:")
    );
}

#[test]
fn every_refusal_has_a_positive_control() {
    let mut a = answer();
    assert!(render_text(&a, AnswerBudget::default()).is_ok());
    assert!(matches!(
        render_text(&a, AnswerBudget::new(1).unwrap()),
        Err(RenderError::BudgetTooSmall)
    ));
    a.records[0].l = [2, 1];
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::InvalidRecord)
    ));
    a.records[0].l = [1, 2];
    a.rev = "not a sha".into();
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::InvalidMetadata)
    ));
    a.rev = "abcdef0".into();
    a.records.clear();
    a.next.clear();
    assert!(matches!(
        render_json(&a, AnswerBudget::default()),
        Err(RenderError::MissingNext)
    ));
    a.next = "dctx map map".into();
    assert!(render_json(&a, AnswerBudget::default()).is_ok());
    a.records = vec![record("x")];
    a.more = usize::MAX;
    let mut big = record("z");
    big.sum = "x".repeat(7001);
    a.records.push(big);
    assert!(matches!(
        render_text(&a, AnswerBudget::default()),
        Err(RenderError::CountOverflow)
    ));
}

#[test]
fn hard_record_counts_and_deterministic_ranking() {
    let mut a = answer();
    a.verb = Verb::Map;
    a.records = (0..80)
        .rev()
        .map(|i| {
            let mut r = record(&format!("f{i:02}"));
            r.sum.clear();
            r.score = (i % 3).into();
            r
        })
        .collect();
    for (verb, limit) in [(Verb::Map, 60), (Verb::Find, 40)] {
        a.verb = verb;
        let text = render_text(&a, AnswerBudget::new(2000).unwrap()).unwrap();
        assert_eq!(
            text.lines().filter(|line| line.starts_with("s:")).count(),
            limit
        );
        let output = render_json(&a, AnswerBudget::new(2000).unwrap()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed["records"].as_array().unwrap().len() <= limit);
        a.records.reverse();
        assert_eq!(
            render_json(&a, AnswerBudget::new(2000).unwrap()).unwrap(),
            output
        );
    }
}

#[test]
fn explain_has_a_thousand_token_ceiling_even_with_a_larger_budget() {
    let mut a = answer();
    a.verb = Verb::Explain;
    a.records[0].signature = Some("x".repeat(4000));
    for format in [Format::Text, Format::Json] {
        let output = render(&a, AnswerBudget::new(2000).unwrap(), format).unwrap();
        assert!(output.len() <= 3500);
        assert!(!output.contains(&"x".repeat(4000)));
    }
}
