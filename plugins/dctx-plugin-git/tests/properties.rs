// SPDX-License-Identifier: Apache-2.0
//! Property tests for the PL-G invariants: marker/version, newline preservation, negation conflicts,
//! glob-intersection soundness and concurrency (no overwrite of a concurrent edit).
//! Implements PLUGINS.md §7.2 PL-G "marker/version/newline/negation/concurrency property tests" and
//! G-IDEMPOTENT/G-REPLACE/G-COVERAGE. Depends on the pure domain and tests/common's fake broker.

mod common;

use common::{FakeBroker, binding, hook};
use dctx_plugin_git::app::invoke::invoke;
use dctx_plugin_git::domain::glob::Glob;
use dctx_plugin_git::domain::plan::{Plan, plan};
use dctx_plugin_git::domain::text::{IgnoreText, MAX_IGNORE_BYTES};
use dctx_plugin_protocol::ErrorCode;
use proptest::prelude::*;

fn version() -> impl Strategy<Value = String> {
    (0u32..50, 0u32..50, 0u32..50).prop_map(|(a, b, c)| format!("{a}.{b}.{c}"))
}

fn user_lines() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec("[a-z0-9_./*# -]{0,24}", 0..12)
}

fn body(lines: &[String], crlf: bool, terminated: bool) -> String {
    let newline = if crlf { "\r\n" } else { "\n" };
    let mut text = lines.join(newline);
    if terminated && !text.is_empty() {
        text.push_str(newline);
    }
    text
}

fn text(body: &str) -> IgnoreText {
    IgnoreText::parse(body.as_bytes(), MAX_IGNORE_BYTES).expect("valid")
}

proptest! {
    #[test]
    fn appending_preserves_bytes_and_newline_style_and_is_idempotent(
        lines in user_lines(), crlf: bool, terminated: bool, host in version(),
    ) {
        let original = body(&lines, crlf, terminated);
        let Plan::Append(append) = plan(&text(&original), &host, &[], false) else {
            return Ok(());
        };
        prop_assert!(append.replacement.starts_with(original.as_bytes()), "user bytes are a prefix");
        prop_assert!(append.growth <= 4096);
        let suffix = String::from_utf8(append.replacement[original.len()..].to_vec()).expect("utf8");
        if crlf && original.contains('\n') {
            prop_assert!(!suffix.replace("\r\n", "").contains('\n'), "CRLF file gains only CRLF lines");
        } else {
            prop_assert!(!suffix.contains('\r'), "LF file gains only LF lines");
        }
        let committed = String::from_utf8(append.replacement).expect("utf8");
        prop_assert_eq!(plan(&text(&committed), &host, &[], false), Plan::AlreadyProtected);
    }

    #[test]
    fn a_new_host_version_appends_only_its_marker_and_keeps_the_old_one(old in version(), new in version()) {
        prop_assume!(old != new);
        let protected = format!(".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n# [dctx v{old}]\n");
        prop_assert_eq!(plan(&text(&protected), &old, &[], false), Plan::AlreadyProtected);
        let Plan::Append(append) = plan(&text(&protected), &new, &[], false) else {
            return Err(TestCaseError::fail("new host version must append"));
        };
        prop_assert!(append.appended_rules.is_empty() && append.marker_added);
        prop_assert_eq!(String::from_utf8(append.replacement).expect("utf8"), format!("{protected}# [dctx v{new}]\n"));
    }

    #[test]
    fn a_later_negation_of_any_export_name_is_never_protected(name in "[a-zA-Z0-9_-]{1,12}", diff: bool, before: bool) {
        let negated = if diff { format!("!{name}.llmcontext-1.0-diff.txt") } else { format!("!{name}.llmcontext.txt") };
        let rules = ".dctx\n.dctx/\n*.llmcontext.txt\n*.llmcontext-*-diff.txt\n# [dctx v0.26.0]\n";
        let content = if before { format!("{negated}\n{rules}") } else { format!("{rules}{negated}\n") };
        let verdict = plan(&text(&content), "0.26.0", &[], false);
        if before {
            prop_assert_eq!(verdict, Plan::AlreadyProtected);
        } else {
            prop_assert_eq!(verdict, Plan::RulesConflict);
        }
    }

    #[test]
    fn glob_intersection_never_misses_a_common_name(a in "[a-c*?.]{0,6}", b in "[a-c*?.]{0,6}", name in "[a-c.]{0,6}") {
        let (left, right) = (Glob::parse(&a).expect("glob"), Glob::parse(&b).expect("glob"));
        if left.matches(&name) && right.matches(&name) {
            prop_assert!(left.intersects(&right), "{a} and {b} both match {name}");
        }
    }

    #[test]
    fn a_concurrent_edit_is_never_overwritten(original in user_lines(), edit in "[a-z\n]{0,40}") {
        let content = body(&original, false, true);
        let mut broker = FakeBroker::new(content.as_bytes());
        broker.concurrent_edit = Some(edit.clone().into_bytes());
        let result = invoke(&mut broker, &binding(true, &[], true), &hook("0.26.0", true, true));
        if let Ok(answer) = &result {
            prop_assert_ne!(answer.report.reason, "completed", "no completed edit over a concurrent change");
        } else {
            prop_assert_eq!(result.err().map(|failure| failure.code), Some(ErrorCode::Conflict));
            prop_assert_eq!(&broker.file, &edit.into_bytes());
        }
        prop_assert_eq!((broker.writes, broker.git_spawns), (0, 0));
    }
}
