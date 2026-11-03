// SPDX-License-Identifier: Apache-2.0
//! Public-API properties for indivisible, bounded map answers.
//! Verifies SPEC-MAP-MODE section 3 with arbitrary Unicode and JSON escapes.
//! Uses proptest and `serde_json`; never accesses a filesystem or clock.
use dctx_map_model::{adapters::*, *};
use proptest::prelude::*;

fn answer(rows: Vec<(String, i64)>, more: usize) -> Answer {
    let records = rows
        .into_iter()
        .enumerate()
        .map(|(i, (sum, score))| Record {
            id: format!("s:src/lib.rs#f{i:03}").parse().unwrap(),
            kind: "fn".into(),
            path: "src/lib.rs".into(),
            l: [1, 2],
            sum,
            src: SummarySource::Doc,
            relation: None,
            stale: false,
            signature: None,
            incoming: vec![],
            outgoing: vec![],
            tests: vec![],
            documents: vec![],
            body: None,
            score,
        })
        .collect();
    Answer {
        place: "property".into(),
        rev: "abcdef0123456789".into(),
        dirty: 0,
        age_seconds: 0,
        verb: Verb::Find,
        records,
        more,
        next: "dctx map find \"next\" --page 2".into(),
        identity: None,
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn answers_never_exceed_budget_or_split_records(
            rows in prop::collection::vec((".{0,200}", -100i64..100), 0..65),
            tokens in 1u16..=2000,
            more in 0usize..1000,
        ) {
            let mut a = answer(rows, more);
            let mut sorted = a.records.clone();
            sorted.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
            let budget = AnswerBudget::new(tokens).unwrap();
            for format in [Format::Text, Format::Json] {
                let output = match render(&a, budget, format) {
                    Ok(output) => output,
                    Err(RenderError::BudgetTooSmall) => continue,
                    Err(error) => return Err(TestCaseError::fail(error.to_string())),
                };
                prop_assert!(budget.fits(&output));
                prop_assert!((output.len() * 2).div_ceil(7) <= usize::from(tokens));
                let count = match format {
                    Format::Json => {
                        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
                        let records = value["records"].as_array().unwrap();
                        for (actual, expected) in records.iter().zip(&sorted) {
                            prop_assert_eq!(actual, &serde_json::to_value(expected).unwrap());
                        }
                        prop_assert_eq!(usize::try_from(value["more"].as_u64().unwrap()).unwrap(), more + a.records.len() - records.len());
                        records.len()
                    }
                    Format::Text => {
                        let lines: Vec<_> = output.lines().filter(|line| line.starts_with("s:")).collect();
                        for (actual, r) in lines.iter().zip(&sorted) {
                            let sum = r.sum.split_whitespace().collect::<Vec<_>>().join(" ");
                            let expected = format!("{}  fn  L1-L2  {}  [doc]", r.id, sum);
                            prop_assert_eq!(*actual, expected);
                        }
                        let remaining = more + a.records.len() - lines.len();
                        if remaining > 0 {
                            let footer = format!("+{remaining} more · next: {}\n", a.next);
                            prop_assert!(output.ends_with(&footer));
                        }
                        lines.len()
                    }
                };
                prop_assert!(count <= 40);
                a.records.reverse();
                prop_assert_eq!(render(&a, budget, format).unwrap(), output);
            }
        }
    }
}
