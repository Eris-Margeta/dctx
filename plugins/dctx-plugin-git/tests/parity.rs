// SPDX-License-Identifier: Apache-2.0
//! Every git-cases.json fixture through the fake broker, directly against the use case (no framing).
//! Implements PLUGINS.md §7.2 PL-G "All §6 fixtures through a fake broker" (G-PARITY, G-COVERAGE,
//! G-IDEMPOTENT, G-STAGE). Depends on tests/common; the wire path is in conformance.rs.

mod common;

use common::cases::{Observed, cases, check, setup};
use dctx_plugin_git::app::invoke::invoke;

#[test]
fn every_parity_case_matches_its_golden_outcome() {
    let all = cases();
    assert_eq!(all.len(), 15, "the frozen corpus has fifteen cases");
    for case in &all {
        let (mut broker, binding, invocation) = setup(case);
        let observed = match invoke(&mut broker, &binding, &invocation) {
            Ok(answer) => Observed {
                status: Some(answer.report.status.as_str().to_owned()),
                reason: Some(answer.report.reason.to_owned()),
                code: None,
                diagnostics: answer.report.diagnostics.iter().map(|(code, _)| code.as_str().to_owned()).collect(),
                receipts: answer.report.receipts.len(),
            },
            Err(failure) => Observed {
                code: Some(failure.code.as_str().to_owned()),
                receipts: failure.receipts.len(),
                ..Observed::default()
            },
        };
        check(case, &observed, &broker);
    }
}

#[test]
fn second_run_after_an_edit_is_already_protected_with_zero_effects() {
    for case in cases().iter().filter(|case| case.raw.get("expected").and_then(|e| e.get("writes")).is_some()) {
        let (mut broker, binding, invocation) = setup(case);
        let Ok(first) = invoke(&mut broker, &binding, &invocation) else { continue };
        if broker.writes == 0 {
            continue;
        }
        let (writes, spawns) = (broker.writes, broker.git_spawns);
        let second = invoke(&mut broker, &binding, &invocation).expect("second run");
        assert_eq!(second.report.reason, "already-protected", "{}", case.id);
        assert_eq!((broker.writes, broker.git_spawns), (writes, spawns), "{}: no retry of staging", case.id);
        assert!(first.report.protected && second.report.protected, "{}", case.id);
    }
}
