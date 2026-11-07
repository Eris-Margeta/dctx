// SPDX-License-Identifier: Apache-2.0
//! Historical differences with named expected outcomes: the old core `git_safety.rs` (85c0f7a, ADR-005)
//! modelled on strings next to the plugin, per PLUGINS.md §6.1 "A table records each deliberate amendment,
//! so parity does not freeze historical bugs" (G-PARITY). Plus the trace-git-* scenario pairs.
//! Depends on tests/common's fake broker.

use std::fmt::Write as _;
mod common;

use common::{FakeBroker, GitScript, binding, hook};
use dctx_plugin_git::app::invoke::invoke;
use dctx_plugin_protocol::ErrorCode;

const FULL: &str = "# [dctx] Automated Rules\n.dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n# [dctx v0.26.0]\n";
const RULES: [&str; 4] = [
    ".dctx",
    ".dctx/",
    "*.llmcontext.txt",
    "*.llmcontext-*-diff.txt",
];

/// The historical algorithm: (new content, staged) or None when it returned early.
fn historical(content: &str, version: &str) -> Option<(String, bool)> {
    let marker = format!("# [dctx v{version}]");
    if content.lines().any(|line| line.trim() == marker) {
        return None;
    }
    let present: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let missing: Vec<&str> = RULES
        .iter()
        .copied()
        .filter(|rule| !present.contains(rule))
        .collect();
    let mut out = format!("{content}\n");
    if !missing.is_empty() {
        out.push_str("# [dctx] Automated Rules\n");
        for rule in &missing {
            let _ = writeln!(out, "{rule}");
        }
    }
    let _ = writeln!(out, "{marker}");
    Some((out, true))
}

struct Outcome {
    status: String,
    reason: String,
    code: Option<ErrorCode>,
    broker: FakeBroker,
}

fn plugin(content: &str, repository: bool, setup: impl FnOnce(&mut FakeBroker)) -> Outcome {
    let mut broker = FakeBroker::new(content.as_bytes());
    setup(&mut broker);
    let result = invoke(
        &mut broker,
        &binding(true, &[], true),
        &hook("0.26.0", true, repository),
    );
    match result {
        Ok(answer) => Outcome {
            status: answer.report.status.as_str().into(),
            reason: answer.report.reason.into(),
            code: None,
            broker,
        },
        Err(failure) => Outcome {
            status: String::new(),
            reason: String::new(),
            code: Some(failure.code),
            broker,
        },
    }
}

#[test]
fn h1_marker_alone_no_longer_means_safe() {
    let removed = ".dctx\n.dctx/\n*.llmcontext.txt\n# [dctx v0.26.0]\n";
    assert_eq!(
        historical(removed, "0.26.0"),
        None,
        "historical fast path trusted the marker"
    );
    let outcome = plugin(removed, true, |_| {});
    assert_eq!(
        (outcome.reason.as_str(), outcome.broker.writes),
        ("completed", 1)
    );
    assert!(String::from_utf8_lossy(&outcome.broker.file).ends_with("*.llmcontext-*-diff.txt\n"));
}

#[test]
fn h2_no_blank_separator_and_h9_crlf_preserved() {
    let (old, _) = historical("notes\n", "0.26.0").expect("slow path");
    assert!(
        old.starts_with("notes\n\n"),
        "historical always wrote an empty line first"
    );
    assert_eq!(
        String::from_utf8_lossy(&plugin("notes\n", true, |_| {}).broker.file),
        format!("notes\n{FULL}")
    );
    let (old, _) = historical("notes\r\n", "0.26.0").expect("slow path");
    assert!(
        old.contains("\n.dctx\n"),
        "historical mixed LF into a CRLF file"
    );
    let crlf = plugin("notes\r\n", true, |_| {}).broker.file;
    assert!(
        !String::from_utf8_lossy(&crlf)
            .replace("\r\n", "")
            .contains('\n'),
        "plugin keeps CRLF"
    );
}

#[test]
fn h3_non_repository_is_skipped_with_zero_effects() {
    assert!(
        historical("notes\n", "0.26.0").is_some(),
        "historical wrote outside Git repositories"
    );
    let outcome = plugin("notes\n", false, |_| {});
    assert_eq!(
        (outcome.status.as_str(), outcome.reason.as_str()),
        ("skipped", "not-a-repository")
    );
    assert_eq!(
        (
            outcome.broker.reads,
            outcome.broker.writes,
            outcome.broker.git_spawns
        ),
        (0, 0, 0)
    );
}

#[test]
fn h4_staging_failures_are_reported_not_discarded() {
    for (script, reason) in [
        (GitScript::Missing, "rules-written-not-staged"),
        (GitScript::Exit(128), "rules-written-not-staged"),
    ] {
        let outcome = plugin("", true, |broker| broker.git = script);
        assert_eq!(
            (
                outcome.status.as_str(),
                outcome.reason.as_str(),
                outcome.broker.writes
            ),
            ("warning", reason, 1)
        );
    }
    let uncertain = plugin("", true, |broker| {
        broker.git = GitScript::Refuse(
            ErrorCode::ExternalProgramFailed,
            dctx_plugin_protocol::EffectState::Unknown,
        );
    });
    assert_eq!(
        (uncertain.code, uncertain.broker.writes),
        (Some(ErrorCode::EffectUnknown), 1)
    );
}

#[test]
fn h5_negations_are_conflicts_and_h6_leading_spaces_are_not_rules() {
    let negated = format!("{FULL}!*.llmcontext.txt\n");
    assert_eq!(
        historical(&negated, "0.26.0"),
        None,
        "historical claimed safety"
    );
    assert_eq!(plugin(&negated, true, |_| {}).reason, "rules-conflict");
    let spaced = "  .dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n";
    let (old, _) = historical(spaced, "0.26.0").expect("slow path");
    assert!(
        !old.contains("Automated Rules"),
        "historical trimmed '  .dctx' into a rule"
    );
    let file = String::from_utf8_lossy(&plugin(spaced, true, |_| {}).broker.file).into_owned();
    assert!(
        file.ends_with("# [dctx] Automated Rules\n.dctx\n# [dctx v0.26.0]\n"),
        "{file}"
    );
}

#[test]
fn h7_concurrent_edits_conflict_instead_of_being_overwritten() {
    let outcome = plugin("notes\n", true, |broker| {
        broker.concurrent_edit = Some(b"user edit\n".to_vec())
    });
    assert_eq!(outcome.code, Some(ErrorCode::Conflict));
    assert_eq!(
        (
            outcome.broker.file.as_slice(),
            outcome.broker.writes,
            outcome.broker.git_spawns
        ),
        (&b"user edit\n"[..], 0, 0)
    );
}

#[test]
fn h10_marker_carries_the_host_version_not_the_plugin_version() {
    let file = String::from_utf8_lossy(&plugin("", true, |_| {}).broker.file).into_owned();
    assert!(file.contains("# [dctx v0.26.0]") && !file.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn scenario_marker_idempotency_pair() {
    let allow = plugin(FULL, true, |_| {});
    assert_eq!(
        (
            allow.reason.as_str(),
            allow.broker.writes,
            allow.broker.git_spawns
        ),
        ("already-protected", 0, 0)
    );
    let removed = plugin(
        ".dctx\n.dctx/\n*.llmcontext.txt\n# [dctx v0.26.0]\n",
        true,
        |_| {},
    );
    assert_eq!(
        (
            removed.reason.as_str(),
            removed.broker.writes,
            removed.broker.git_spawns
        ),
        ("completed", 1, 1)
    );
}

#[test]
fn scenario_staging_grant_pair() {
    let allow = plugin("", true, |_| {});
    assert_eq!(allow.broker.git_spawns, 1);
    assert_eq!(
        allow.broker.staged.as_deref(),
        Some(allow.broker.file.as_slice()),
        "whole file staged"
    );
    let mut broker = FakeBroker::new(b"");
    let mut invocation = hook("0.26.0", true, true);
    invocation
        .grants
        .retain(|grant| grant.as_str() != "grant:program");
    let answer = invoke(&mut broker, &binding(true, &[], true), &invocation).expect("answer");
    assert_eq!(
        (answer.report.status.as_str(), answer.report.reason),
        ("warning", "rules-written-not-staged")
    );
    assert_eq!((broker.writes, broker.git_spawns), (1, 0));
    let offered_without_api = invoke(
        &mut FakeBroker::new(b""),
        &binding(true, &[], false),
        &hook("0.26.0", true, true),
    );
    assert_eq!(
        offered_without_api.expect("answer").report.reason,
        "rules-written-not-staged"
    );
}
