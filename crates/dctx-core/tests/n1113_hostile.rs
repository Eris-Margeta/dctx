// SPDX-License-Identifier: Apache-2.0
//! Deterministic hostile inputs for the public core API. Byte APIs receive raw
//! invalid UTF-8; `&str` APIs receive only validated prefixes, including controls.
//! Bounded APIs test their exact limits; unbounded APIs use large finite samples
//! and numeric extrema rather than attempting allocations near address-space size.

use dctx_core::budget::{byte_ceiling, estimate_tokens, OutputBudget, PackLimits, Reservation};
use dctx_core::diff::{compare_trees, ChangeType, DiffReport};
use dctx_core::domain::{FileHandlingDecision as Decision, ItemContext};
use dctx_core::error::{DctxError, ErrorCode, Resource, MAX_IO_DETAIL_BYTES};
use dctx_core::facts::{
    apply_content, mark_subtree_ignored, snapshot_digest, ContentDigests, EntryMeta, EntryStamp,
    PackSnapshot, ReadGrant, SourceEntry,
};
use dctx_core::file_classifier::{classify_file, FileContentHandling};
use dctx_core::filter::{has_git_segment, is_secret_shaped_name, FilterConfig};
use dctx_core::ids::{
    ContentDigest, RelativePath, SnapshotDigest, Timestamp, ViewKey, MAX_PATH_BYTES,
};
use dctx_core::selection::rule::{glob_subject, parse_rule_lines};
use dctx_core::selection::{
    glob_match, last_match, parse_ignore_pattern_line, parse_rules, rule_matches, LayerCounts,
    RuleSource, RuleTexts, SelectionPolicy, HARDCODED_IGNORE_RULES, HARDCODED_OMIT_CONTENT_RULES,
};
use dctx_core::settings::{parse_config, parse_config_line, AppConfig};
use dctx_core::version::{version_after_diff, Version};
use dctx_core::{DirContextTreeNode, IgnoreRule, NodeType, PatternType};
use std::path::{Path, PathBuf};

fn node(kind: NodeType, path: &str) -> DirContextTreeNode {
    DirContextTreeNode::new(kind, path, PathBuf::from(path))
}

fn file(path: &str, content: &[u8]) -> DirContextTreeNode {
    let mut result = node(NodeType::File, path);
    apply_content(&mut result, content);
    result
}

fn tree(children: Vec<DirContextTreeNode>) -> DirContextTreeNode {
    let mut root = node(NodeType::Directory, "");
    root.children = children;
    root
}

fn context(path: &str, is_directory: bool, debug_mode: bool) -> ItemContext<'_> {
    ItemContext {
        relative_path: path,
        name: path.rsplit('/').next().unwrap(),
        is_directory,
        debug_mode,
    }
}

fn report_bytes(report: &DiffReport) -> Vec<u8> {
    format!("{report:?}").into_bytes()
}

// Exercises parsing, predicates, accessors, policy compilation and explanations
// together so repeat and thread checks cover their observable outputs as bytes.
fn text_probe(text: &str) -> Vec<u8> {
    let rules = parse_rules(text);
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(
        format!("{rules:?}"),
        format!("{:?}", parse_rule_lines(&lines))
    );
    let policy = SelectionPolicy::from_rules(rules, parse_rules(text));
    let layered = SelectionPolicy::from_texts(&RuleTexts {
        global_ignore: Some(text),
        global_omit: Some(text),
        local_ignore: Some(text),
        local_omit: Some(text),
    });
    let mut output = format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        RelativePath::parse(text),
        RelativePath::root().child(text),
        ViewKey::parse(text),
        Version::parse(text),
        parse_config(text),
        parse_ignore_pattern_line(text),
        is_secret_shaped_name(text),
        has_git_segment(text),
    );
    let filter = FilterConfig::new().with_extensions(vec![text.to_string()]);
    for path in ["", "x", "src/x.rs", text] {
        for directory in [false, true] {
            for debug in [false, true] {
                let item = context(path, directory, debug);
                let expected = policy
                    .decide_with(&item, |rules| last_match(rules, path, item.name, directory));
                assert_eq!(policy.decide(&item), expected);
                for rule in policy.ignore_rules() {
                    let _ = glob_subject(rule, path, item.name);
                    let _ = rule_matches(rule, path, item.name, directory);
                }
                output.push_str(&policy.explain(&item));
                output.push_str(&layered.explain(&item));
                output.push_str(&format!(
                    "{}{}",
                    filter.should_include_file(item.name, u64::MAX, false, Path::new(path)),
                    filter.should_include_directory(item.name)
                ));
            }
        }
    }
    assert_eq!(policy.ignore_layers().total(), policy.ignore_rules().len());
    assert_eq!(policy.omit_layers().total(), policy.omit_rules().len());
    assert_eq!(
        policy.total_rules(),
        policy.ignore_rules().len() + policy.omit_rules().len()
    );
    output.into_bytes()
}

#[test]
fn empty_and_every_ascii_one_byte_text_are_repeatable() {
    assert_eq!(text_probe(""), text_probe(""));
    for byte in 0..=127u8 {
        let bytes = [byte];
        let text = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(text_probe(text), text_probe(text), "byte {byte}");
    }
}

#[test]
fn every_truncation_of_text_samples_is_safe_and_repeatable() {
    for sample in [
        "src/**/x?.rs\n!keep.rs # comment\n",
        "default_template = '雪é'\nmax_diff_files = 42\n",
        "V4294967295.4294967295",
        "a/雪/é.rs",
    ] {
        for end in 0..=sample.len() {
            if let Ok(prefix) = std::str::from_utf8(&sample.as_bytes()[..end]) {
                assert_eq!(text_probe(prefix), text_probe(prefix));
            } else {
                assert!(!sample.is_char_boundary(end));
            }
        }
    }
}

#[test]
fn paths_and_keys_enforce_byte_limits_including_multibyte_text() {
    for exact in ["a".repeat(MAX_PATH_BYTES), "é".repeat(MAX_PATH_BYTES / 2)] {
        let path = RelativePath::parse(&exact).unwrap();
        assert_eq!(path.as_str(), exact);
        assert_eq!(path.name(), exact);
        assert_eq!(path.depth(), 1);
        assert_eq!(path.to_string(), exact);
        assert_eq!(ViewKey::parse(&exact).unwrap().as_str(), exact);
        assert_eq!(RelativePath::root().child(&exact).unwrap(), path);
        for over in [format!("{exact}x"), format!("{exact}é")] {
            for error in [
                RelativePath::parse(&over).unwrap_err(),
                RelativePath::root().child(&over).unwrap_err(),
                ViewKey::parse(&over).unwrap_err(),
            ] {
                assert_eq!(error.code(), ErrorCode::ResourceLimit);
            }
        }
        assert!(path.child("x").is_err());
    }
    let base = RelativePath::parse(&"a".repeat(MAX_PATH_BYTES - 2)).unwrap();
    assert_eq!(base.child("x").unwrap().as_str().len(), MAX_PATH_BYTES);
    assert!(base.child("xx").is_err());
    assert!(RelativePath::default().is_root());
    assert_eq!(RelativePath::root().name(), "");
    assert_eq!(RelativePath::root().depth(), 0);
    assert!(ViewKey::parse("").is_err());
}

#[test]
fn path_confinement_refuses_controls_and_malformed_segments() {
    for text in [
        "/x", "x/", "x//y", ".", "..", "x/../y", "x\\y", "x:y", "x.", "x ",
    ] {
        assert_eq!(
            RelativePath::parse(text).unwrap_err().code(),
            ErrorCode::InvalidRequest
        );
    }
    for byte in 0..=127u8 {
        let character = char::from(byte);
        if character.is_control() {
            let text = format!("x{character}y");
            assert!(RelativePath::parse(&text).is_err());
            assert!(RelativePath::root().child(&text).is_err());
            assert!(ViewKey::parse(&text).is_err());
        }
    }
    for text in ["x/y", "", "..", "x\\y"] {
        assert!(RelativePath::root().child(text).is_err());
    }
    assert_eq!(RelativePath::parse("a/雪/x").unwrap().depth(), 3);
}

#[test]
fn token_arithmetic_matches_wide_integer_oracle_at_extrema() {
    for value in (0..=1024).chain([u64::MAX / 7, u64::MAX / 2, u64::MAX - 1, u64::MAX]) {
        assert_eq!(
            u128::from(estimate_tokens(value)),
            (u128::from(value) * 2).div_ceil(7)
        );
        let expected = (u128::from(value) * 7 / 2).min(u128::from(u64::MAX));
        assert_eq!(u128::from(byte_ceiling(value)), expected);
        assert!(estimate_tokens(byte_ceiling(value)) <= value);
        if byte_ceiling(value) < u64::MAX {
            assert!(estimate_tokens(byte_ceiling(value) + 1) > value);
        }
    }
}

#[test]
fn budget_intersection_is_idempotent_commutative_and_enforces_exact_limits() {
    let budgets = [
        OutputBudget::default(),
        OutputBudget {
            encoded_bytes: Some(0),
            estimated_tokens: Some(0),
            records: Some(0),
        },
        OutputBudget {
            encoded_bytes: Some(7),
            estimated_tokens: Some(1),
            records: Some(1),
        },
        OutputBudget {
            encoded_bytes: Some(u64::MAX),
            estimated_tokens: Some(u64::MAX),
            records: Some(u64::MAX),
        },
    ];
    for first in budgets {
        assert_eq!(first.intersect(first), first);
        for second in budgets {
            assert_eq!(first.intersect(second), second.intersect(first));
            for third in budgets {
                assert_eq!(
                    first.intersect(second).intersect(third),
                    first.intersect(second.intersect(third))
                );
            }
        }
        for bytes in [0, 1, 3, 4, 7, u64::MAX] {
            for records in [0, 1, 2, u64::MAX] {
                assert_eq!(
                    first.admits(bytes, records),
                    first.encoded_bytes.is_none_or(|cap| bytes <= cap)
                        && first
                            .estimated_tokens
                            .is_none_or(|cap| estimate_tokens(bytes) <= cap)
                        && first.records.is_none_or(|cap| records <= cap)
                );
            }
        }
    }
}

#[test]
fn pack_limits_and_reservations_handle_zero_exact_over_and_maximum() {
    assert_eq!(PackLimits::default(), PackLimits::unbounded());
    for cap in [0, 1, 4096, u64::MAX - 1, u64::MAX] {
        let limits = PackLimits {
            max_entries: cap,
            max_depth: cap,
            max_file_bytes: cap,
            max_source_bytes: cap,
        };
        for value in [0, 1, cap, cap.saturating_add(1), u64::MAX] {
            for (result, resource) in [
                (limits.check_entries(value), Resource::Entries),
                (limits.check_depth(value), Resource::Depth),
                (limits.check_source_bytes(value), Resource::SourceBytes),
            ] {
                assert_eq!(
                    result,
                    if value <= cap {
                        Ok(())
                    } else {
                        Err(DctxError::ResourceLimit {
                            resource,
                            limit: cap,
                        })
                    }
                );
            }
            let reservation = Reservation::new(cap, value);
            assert_eq!(reservation.id(), cap);
            assert_eq!(reservation.bytes(), value);
            assert_eq!(Timestamp(value).millis(), value);
        }
    }
}

#[test]
fn every_single_byte_and_every_truncation_reaches_byte_apis() {
    for byte in 0..=255u8 {
        let bytes = [byte];
        assert!(glob_match(&bytes, &bytes), "literal {byte}");
        assert_eq!(glob_match(b"?", &bytes), byte != b'/');
        assert_eq!(
            classify_file(Path::new("x"), &bytes),
            classify_file(Path::new("x"), &bytes)
        );
        let mut result = file("x", &bytes);
        let before = format!("{result:?}");
        apply_content(&mut result, &bytes);
        assert_eq!(format!("{result:?}"), before);
        if byte >= 128 {
            assert!(std::str::from_utf8(&bytes).is_err());
        }
    }
    for sample in [
        b"a/\xff\xfe\0?*.rs".as_slice(),
        "雪/é.rs".as_bytes(),
        b"\x89PNG\r\n\x1a\n",
    ] {
        for end in 0..=sample.len() {
            let prefix = &sample[..end];
            let _ = classify_file(Path::new("x"), prefix);
            let _ = file("x", prefix);
            for pattern in [b"**".as_slice(), b"**/*.rs", b"a?/*", prefix] {
                assert_eq!(glob_match(pattern, prefix), glob_match(pattern, prefix));
            }
        }
    }
}

#[test]
fn glob_star_can_stop_immediately_before_a_separator() {
    for (pattern, text) in [
        ("*/x", "a/x"),
        ("*/x", "/x"),
        ("a*/x", "abc/x"),
        ("*/?/z", "a/b/z"),
    ] {
        assert!(
            glob_match(pattern.as_bytes(), text.as_bytes()),
            "{pattern}: {text}"
        );
    }
    assert!(!glob_match(b"*/x", b"a/b/x"));
}

#[test]
fn double_star_handles_literal_tails_and_repeated_stars() {
    for pattern in [b"**x".as_slice(), b"***x", b"****x"] {
        for text in [b"x".as_slice(), b"abx", b"a/b/x"] {
            assert!(glob_match(pattern, text), "{pattern:?}: {text:?}");
        }
        assert!(!glob_match(pattern, b"a/b/y"));
    }
    assert!(glob_match(b"**/x", b"x"));
    assert!(glob_match(b"**/x", b"a/b/x"));
}

#[test]
fn glob_adversarial_branching_terminates_without_a_timing_assertion() {
    let pattern = format!("{}b*", "*a".repeat(24));
    assert!(!glob_match(pattern.as_bytes(), &[b'a'; 48]));
    let pattern = format!("{}x", "**/".repeat(128));
    assert!(glob_match(pattern.as_bytes(), b"x"));
    let text = vec![b'a'; 65_536];
    assert!(glob_match(&text, &text));
    assert!(glob_match(b"*", &text));
    assert!(!glob_match(b"?", &text));
}

#[test]
fn question_mark_before_prefix_star_is_not_treated_as_literal() {
    let rule = parse_ignore_pattern_line("a?/*").unwrap();
    assert_eq!(rule.pattern_type, PatternType::Glob);
    assert!(rule_matches(&rule, "ab/x", "x", false));
    assert!(!rule_matches(&rule, "abc/x", "x", false));
    assert!(!rule_matches(&rule, "ab/deep/x", "x", false));
}

#[test]
fn duplicate_and_reordered_rules_preserve_last_match_semantics() {
    for (rules, expected) in [("*.rs\n!x.rs\n*.rs", true), ("*.rs\n*.rs\n!x.rs", false)] {
        let parsed = parse_rules(rules);
        let policy = SelectionPolicy::from_rules(parsed.clone(), vec![]);
        assert_eq!(last_match(&parsed, "src/x.rs", "x.rs", false), expected);
        assert_eq!(
            policy.decide(&context("src/x.rs", false, false)) == Decision::Exclude,
            expected
        );
    }
    assert!(!rule_matches(&IgnoreRule::new(), "", "", false));
    assert_eq!(IgnoreRule::default().pattern_type, PatternType::Invalid);
    for pattern in ["", "*", "!", "/", "!/"] {
        let _ = parse_ignore_pattern_line(pattern);
    }
}

#[test]
fn compiled_policies_match_linear_rules_for_every_rotation() {
    let lines = [
        "x",
        "!x/",
        "*.rs",
        "!x.rs",
        "/src/x.rs",
        "!src/x.rs/",
        "a?/*",
        "*/x",
        "*",
        "!",
        "*/",
        "!*.rs/",
    ];
    for rotation in 0..lines.len() {
        let text = lines[rotation..]
            .iter()
            .chain(&lines[..rotation])
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        let rules = parse_rules(&text);
        let policy = SelectionPolicy::from_rules(rules.clone(), rules.clone());
        for path in ["", "x", "x.rs", "src/x.rs", "ab/x", "é.rs"] {
            for directory in [false, true] {
                for debug in [false, true] {
                    let item = context(path, directory, debug);
                    let expected = policy
                        .decide_with(&item, |rules| last_match(rules, path, item.name, directory));
                    assert_eq!(policy.decide(&item), expected, "{text}: {path}");
                    assert_eq!(
                        policy.explain(&item).as_bytes(),
                        policy.explain(&item).as_bytes()
                    );
                }
            }
        }
    }
}

#[test]
fn large_rule_sets_and_long_unbounded_text_remain_repeatable() {
    for size in [4096, 4097] {
        let text = "x\n".repeat(size);
        let policy = SelectionPolicy::from_rules(parse_rules(&text), vec![]);
        assert_eq!(policy.total_rules(), size);
        assert_eq!(
            policy.decide(&context("x", false, false)),
            Decision::Exclude
        );
        assert_eq!(
            policy.explain(&context("x", false, false)),
            policy.explain(&context("x", false, false))
        );
    }
    for size in [65_536, 65_537] {
        let text = "a".repeat(size);
        assert_eq!(parse_rules(&text)[0].pattern, text);
        let config = parse_config(&format!("default_template = '{text}'"));
        assert_eq!(config.default_template, text);
    }
}

#[test]
fn policy_layers_and_rule_sources_have_stable_labels() {
    assert_eq!(RuleSource::Hardcoded.label(), "hardcoded");
    assert_eq!(RuleSource::Global.label(), "global");
    assert_eq!(RuleSource::Local.label(), "local");
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        global_ignore: Some("x\nx\n"),
        local_ignore: Some("!x\n"),
        global_omit: Some("x\n"),
        local_omit: Some("!x\n"),
    });
    assert_eq!(
        policy.ignore_layers().hardcoded,
        HARDCODED_IGNORE_RULES.len()
    );
    assert_eq!(
        policy.omit_layers().hardcoded,
        HARDCODED_OMIT_CONTENT_RULES.len()
    );
    for (index, source) in [
        (0, RuleSource::Hardcoded),
        (HARDCODED_IGNORE_RULES.len(), RuleSource::Global),
        (HARDCODED_IGNORE_RULES.len() + 2, RuleSource::Local),
    ] {
        assert_eq!(policy.ignore_layers().source_of(index), source);
    }
    assert_eq!(SelectionPolicy::default().total_rules(), 0);
    assert_eq!(
        SelectionPolicy::default().decide(&context("", false, false)),
        Decision::Include
    );
}

#[test]
fn caller_supplied_layer_counts_do_not_overflow() {
    let counts = LayerCounts {
        hardcoded: usize::MAX - 1,
        global: 2,
        local: 3,
    };
    assert_eq!(counts.total(), usize::MAX);
    assert_eq!(counts.source_of(usize::MAX), RuleSource::Global);
    let counts = LayerCounts {
        hardcoded: 1,
        global: 1,
        local: usize::MAX,
    };
    assert_eq!(counts.total(), usize::MAX);
    assert_eq!(counts.source_of(0), RuleSource::Hardcoded);
    assert_eq!(counts.source_of(1), RuleSource::Global);
    assert_eq!(counts.source_of(2), RuleSource::Local);
}

#[test]
fn classifier_threshold_sample_boundary_and_invalid_utf8_are_stable() {
    let path = Path::new("x");
    assert_eq!(
        classify_file(path, b""),
        FileContentHandling::IncludeContent
    );
    assert_eq!(
        classify_file(path, b"\xff\xfe"),
        FileContentHandling::IncludeContent
    );
    assert_eq!(
        classify_file(path, b"\x01\x02\x03abcdefg"),
        FileContentHandling::IncludeContent
    );
    assert!(matches!(
        classify_file(path, b"\x01\x02\x03\x04abcdef"),
        FileContentHandling::OmitBinary { .. }
    ));
    for size in [8191, 8192, 8193] {
        let mut bytes = vec![b'a'; size];
        assert_eq!(
            classify_file(path, &bytes),
            FileContentHandling::IncludeContent
        );
        bytes[size - 1] = 0;
        assert!(matches!(
            classify_file(path, &bytes),
            FileContentHandling::OmitBinary { .. }
        ));
    }
    let mut bytes = vec![b'a'; 8192];
    bytes.extend(vec![1; 8193]);
    assert_eq!(
        classify_file(path, &bytes),
        FileContentHandling::IncludeContent
    );
}

#[test]
fn extension_classification_wins_over_bytes_and_is_case_insensitive() {
    for (extension, label) in [
        ("png", "PNG image"),
        ("pdf", "PDF document"),
        ("zip", "ZIP archive"),
        ("a", "Static library"),
        ("exe", "Windows executable"),
        ("woff2", "Web font"),
    ] {
        for ext in [extension.to_string(), extension.to_uppercase()] {
            for content in [b"".as_slice(), b"a", b"\xff\0"] {
                assert_eq!(
                    classify_file(Path::new(&format!("x.{ext}")), content),
                    FileContentHandling::OmitBinary {
                        file_type: label.to_string()
                    }
                );
            }
        }
    }
    assert_eq!(
        classify_file(Path::new("x.SVG"), b"\0"),
        FileContentHandling::OmitSvgContent
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_are_safe_for_classifier_and_content_facts() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    for path in [b"\xff".as_slice(), b"x.\xff", b"\xff.PNG"] {
        let path = Path::new(OsStr::from_bytes(path));
        let mut result = node(NodeType::File, "x");
        result.disk_path = path.to_path_buf();
        for bytes in [b"".as_slice(), b"x", b"\xff\0"] {
            let expected = classify_file(path, bytes);
            apply_content(&mut result, bytes);
            assert_eq!(
                result.is_binary_file,
                matches!(expected, FileContentHandling::OmitBinary { .. })
            );
        }
    }
}

#[test]
fn repeated_content_application_replaces_old_classification() {
    let mut result = file("x", b"text");
    for (path, bytes) in [
        ("x.png", b"x".as_slice()),
        ("x.svg", b"<svg/>"),
        ("x", b"text"),
        ("x", b""),
        ("x", b"\0"),
    ] {
        result.disk_path = PathBuf::from(path);
        apply_content(&mut result, bytes);
        assert_eq!(result.content_size, bytes.len() as u64);
        let expected = format!("{result:?}");
        apply_content(&mut result, bytes);
        assert_eq!(format!("{result:?}"), expected);
    }
    let mut directory = tree(vec![]);
    let before = format!("{directory:?}");
    apply_content(&mut directory, b"\xff\0");
    assert_eq!(format!("{directory:?}"), before);
}

#[test]
fn all_decision_and_ancestor_combinations_gate_read_grants() {
    for path in ["", "x", "a/雪", &"a".repeat(MAX_PATH_BYTES)] {
        let path = RelativePath::parse(path).unwrap();
        for decision in [
            Decision::Include,
            Decision::Exclude,
            Decision::DebugVisible,
            Decision::OmitContent,
        ] {
            assert_eq!(
                decision.should_read_content(),
                decision == Decision::Include
            );
            assert_eq!(decision.should_recurse(), decision != Decision::Exclude);
            assert_eq!(decision.show_in_tree(), decision != Decision::Exclude);
            assert_eq!(decision.needs_metadata(), decision != Decision::Exclude);
            for ancestors in [false, true] {
                let grant = ReadGrant::authorize(&path, decision, ancestors);
                assert_eq!(grant.is_some(), ancestors && decision == Decision::Include);
                if let Some(grant) = grant {
                    assert_eq!(grant.path(), &path);
                }
            }
        }
    }
    for name in ["", "x", "../x", "\0", "雪"] {
        let entry = SourceEntry::file(name);
        assert_eq!(entry.name, name);
        assert!(!entry.is_dir());
        assert!(SourceEntry::directory(name).is_dir());
    }
    assert_eq!(EntryMeta::default().size, 0);
    let stamp = EntryStamp {
        size: u64::MAX,
        modified_ns: u128::MAX,
        inode: u64::MAX,
    };
    assert_eq!(stamp, stamp);
}

#[test]
fn filters_accept_exact_bounds_and_reject_over_bounds() {
    for cap in [0, 1, u64::MAX - 1, u64::MAX] {
        let config = FilterConfig {
            min_size_bytes: Some(cap),
            max_size_bytes: Some(cap),
            ..FilterConfig::default()
        };
        for size in [0, 1, cap, cap.saturating_add(1), u64::MAX] {
            assert_eq!(
                config.should_include_file("x", size, false, Path::new("x")),
                size == cap
            );
        }
    }
    for extensions in [
        vec!["rs", "RS", "txt"],
        vec!["txt", "RS", "rs"],
        vec!["rs", "txt"],
    ] {
        let config = FilterConfig::new()
            .with_extensions(extensions.into_iter().map(str::to_string).collect());
        for name in ["x.rs", "x.RS", "雪.rs", ".rs", "x.txt"] {
            assert!(config.should_include_file(name, 0, false, Path::new(name)));
        }
        for name in ["", "x", "x.", ".", ".."] {
            assert!(!config.should_include_file(name, 0, false, Path::new(name)));
        }
    }
}

#[test]
fn filter_builders_handle_nonfinite_and_conflicting_inputs() {
    for (mb, expected) in [
        (f64::NAN, 0),
        (f64::NEG_INFINITY, 0),
        (-1.0, 0),
        (0.0, 0),
        (1.0 / 1_048_576.0, 1),
        (1.0, 1_048_576),
        (f64::INFINITY, u64::MAX),
        (f64::MAX, u64::MAX),
    ] {
        assert_eq!(
            FilterConfig::new().with_min_size_mb(mb).min_size_bytes,
            Some(expected)
        );
        assert_eq!(
            FilterConfig::new().with_max_size_mb(mb).max_size_bytes,
            Some(expected)
        );
    }
    let both = FilterConfig::new()
        .with_only_empty(true)
        .with_only_non_empty(true);
    assert!(both.has_active_filters());
    for size in [0, 1, u64::MAX] {
        assert!(!both.should_include_file("x", size, false, Path::new("x")));
    }
    let config = FilterConfig::new()
        .with_only_hidden(true)
        .with_include_binary(true)
        .with_collect_ignored(true);
    assert!(config.collect_ignored_folders);
    for name in ["", "x", ".", ".."] {
        assert!(!config.should_include_directory(name));
    }
    assert!(config.should_include_directory(".x"));
    assert!(config.should_include_file(".x", 1, true, Path::new(".x")));
    assert!(!FilterConfig::new().has_active_filters());
}

#[test]
fn sensitive_name_and_git_segment_predicates_respect_boundaries() {
    for name in [
        ".env",
        ".ENV.local",
        "a.key",
        "id_ed25519-test",
        "credentials.json",
        "secrets.toml",
        "a.credentials.json",
    ] {
        assert!(is_secret_shaped_name(name), "{name}");
        assert!(is_secret_shaped_name(&name.to_ascii_uppercase()));
    }
    for name in [
        "",
        "x",
        "environment",
        "id_rsa.pub",
        "credentials.rs",
        "a.keys",
    ] {
        assert!(!is_secret_shaped_name(name), "{name}");
    }
    for path in [".git", ".git/x", "x/.git", "x/.git/y", ".git/.git"] {
        assert!(has_git_segment(path));
    }
    for path in ["", ".gitignore", "x/.github/y", "x/git/y", ".GIT"] {
        assert!(!has_git_segment(path));
    }
}

#[test]
fn config_duplicate_reordered_and_repeated_lines_are_stable() {
    let text = "max_diff_files = 1\nmax_diff_files = 2\ndefault_template = '雪'\n";
    let expected = parse_config(text);
    assert_eq!(expected.max_diff_files, 2);
    let mut config = AppConfig::default();
    for _ in 0..2 {
        for line in text.lines() {
            parse_config_line(line, &mut config);
        }
    }
    assert_eq!(format!("{config:?}"), format!("{expected:?}"));
    assert_eq!(
        format!(
            "{:?}",
            parse_config("max_diff_files=2\ndefault_template='雪'")
        ),
        format!("{expected:?}")
    );
    assert_eq!(
        parse_config("max_diff_files=4294967295").max_diff_files,
        u32::MAX
    );
    assert_eq!(
        parse_config("max_diff_files=4294967296").max_diff_files,
        999
    );
}

#[test]
fn invalid_config_booleans_do_not_silently_change_preferences() {
    for value in ["", "tru", "TRUE", "\0", "maybe", "4294967296"] {
        for initial in [false, true] {
            let mut config = AppConfig {
                text_output_enabled: initial,
                ..AppConfig::default()
            };
            parse_config_line(&format!("text_output_enabled={value}"), &mut config);
            assert_eq!(config.text_output_enabled, initial, "{value:?}");
            parse_config_line(&format!("output_mode={value}"), &mut config);
            assert_eq!(config.text_output_enabled, initial, "{value:?}");
        }
    }
}

#[test]
fn debug_verbose_preference_is_parsed() {
    let mut config = parse_config("debug_verbose=true");
    assert!(config.debug_verbose);
    parse_config_line("debug_verbose=false", &mut config);
    assert!(!config.debug_verbose);
    parse_config_line("debug_verbose=invalid", &mut config);
    assert!(!config.debug_verbose);
}

#[test]
fn malformed_versions_fall_back_as_a_whole() {
    for text in [
        "",
        "V",
        "x",
        "V2.",
        "V2.x",
        "Vx.2",
        "V2.3.4",
        "V4294967296.2",
        "V2.4294967296",
    ] {
        assert_eq!(Version::parse(text), Version::new_v1(), "{text:?}");
    }
    for version in [
        Version::new_v1(),
        Version { major: 0, minor: 0 },
        Version {
            major: u32::MAX,
            minor: u32::MAX,
        },
    ] {
        assert_eq!(Version::parse(&version.to_string()), version);
    }
}

#[test]
fn version_advancement_checks_extrema_and_unchanged_reports() {
    let maximum = Version {
        major: u32::MAX,
        minor: u32::MAX,
    };
    assert!(maximum.try_calculate_next(0, usize::MAX, u32::MAX).is_err());
    let (sentinel, reset) = maximum.calculate_next(0, usize::MAX, u32::MAX);
    assert_eq!(sentinel, Version { major: 0, minor: 0 });
    assert!(reset);
    let minor = Version {
        major: 1,
        minor: u32::MAX - 1,
    };
    assert_eq!(
        minor
            .try_calculate_next(usize::MAX, usize::MAX, u32::MAX)
            .unwrap(),
        (
            Version {
                major: 1,
                minor: u32::MAX
            },
            false
        )
    );
    let empty = DiffReport::default();
    assert_eq!(
        version_after_diff(Some(&maximum), &empty, 0),
        (maximum, false)
    );
    assert_eq!(
        version_after_diff(None, &empty, 0),
        (Version::new_v1(), true)
    );
    assert_eq!(
        Version::new_v1().try_calculate_next(0, 0, 0).unwrap(),
        (Version { major: 2, minor: 0 }, true)
    );
}

#[test]
fn diff_empty_one_file_and_extreme_metadata_are_repeatable() {
    let empty = tree(vec![]);
    assert!(!compare_trees(Some(&empty), &empty).has_changes);
    assert_eq!(compare_trees(None, &empty).entries.len(), 1);
    let mut old = file("x", b"x");
    for size in [0, 1, u64::MAX] {
        old.content_size = size;
        old.last_modified_timestamp = u64::MAX;
        let new = old.clone();
        assert!(!compare_trees(Some(&old), &new).has_changes);
        assert_eq!(
            report_bytes(&compare_trees(None, &new)),
            report_bytes(&compare_trees(None, &new))
        );
    }
}

#[test]
fn diff_removals_have_byte_identical_sorted_output() {
    let old = tree(
        (0..32)
            .rev()
            .map(|i| file(&format!("x{i:02}"), b"x"))
            .collect(),
    );
    let new = tree(vec![]);
    let report = compare_trees(Some(&old), &new);
    let paths: Vec<_> = report
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted);
    for _ in 0..16 {
        assert_eq!(
            report_bytes(&compare_trees(Some(&old), &new)),
            report_bytes(&report)
        );
    }
}

#[test]
fn diff_duplicate_children_are_not_lost_or_reported_as_new() {
    let old = tree(vec![file("x", b"x"), file("x", b"x"), file("y", b"y")]);
    let mut new = old.clone();
    new.children.reverse();
    let same = compare_trees(Some(&old), &new);
    assert!(!same.has_changes);
    assert_eq!(same.stats.files_unchanged, 3);
    new.children.retain(|child| child.relative_path != "x");
    let removed = compare_trees(Some(&old), &new);
    assert_eq!(removed.stats.files_removed, 2);
    assert_eq!(removed.entries.len(), 2);
}

#[test]
fn diff_type_replacement_reports_every_descendant() {
    let directory = tree(vec![file("d/a", b"a"), file("d/b", b"b")]);
    let mut directory = DirContextTreeNode {
        relative_path: "d".to_string(),
        ..directory
    };
    let original = file("d", b"x");
    for (old, new, change) in [
        (&original, &directory, ChangeType::Added),
        (&directory, &original, ChangeType::Removed),
    ] {
        let report = compare_trees(Some(old), new);
        for path in ["d/a", "d/b"] {
            assert!(
                report
                    .entries
                    .iter()
                    .any(|entry| entry.relative_path == path && entry.change_type == change),
                "missing {path}"
            );
        }
    }
    mark_subtree_ignored(&mut directory);
    assert!(directory.is_ignored && directory.children.iter().all(|child| child.is_ignored));
    let before = format!("{directory:?}");
    mark_subtree_ignored(&mut directory);
    assert_eq!(format!("{directory:?}"), before);
}

fn drop_tree_iteratively(root: DirContextTreeNode) {
    let mut pending = vec![root];
    while let Some(mut node) = pending.pop() {
        pending.append(&mut node.children);
    }
}

#[test]
fn deep_tree_walks_use_bounded_call_stack() {
    let mut deep = file("x", b"x");
    for _ in 0..8193 {
        deep = tree(vec![deep]);
    }
    mark_subtree_ignored(&mut deep);
    let report = compare_trees(Some(&deep), &deep);
    assert!(!report.has_changes);
    assert_eq!(report.stats.files_unchanged, 1);
    let added = compare_trees(None, &deep);
    assert_eq!(added.stats.files_added, 1);
    assert_eq!(added.entries.len(), 8194);
    let removed = compare_trees(Some(&deep), &tree(vec![]));
    assert_eq!(removed.stats.files_removed, 1);
    assert_eq!(removed.entries.len(), 8193);
    let snapshot =
        PackSnapshot::new(deep, Version::new_v1(), Timestamp(0), ContentDigests::new()).unwrap();
    let identity = snapshot.digest();
    let manifest = snapshot.into_manifest();
    assert_eq!(manifest.digest(), identity);
    drop_tree_iteratively(manifest.into_tree());
}

#[test]
fn wide_trees_and_reordered_unique_children_have_consistent_stats() {
    for size in [4096, 4097] {
        let old = tree((0..size).map(|i| file(&format!("x{i}"), b"x")).collect());
        let mut new = old.clone();
        new.children.reverse();
        let report = compare_trees(Some(&old), &new);
        assert!(!report.has_changes);
        assert_eq!(report.stats.files_unchanged, size);
        assert_eq!(report.stats.total_files_in_new_state, size);
    }
}

#[test]
fn snapshot_identity_preserves_stored_order_and_ignores_envelope_fields() {
    let root = tree(vec![file("a", b"a"), file("b", b"b")]);
    let digests = ContentDigests::new();
    let first = PackSnapshot::new(
        root.clone(),
        Version::new_v1(),
        Timestamp(0),
        digests.clone(),
    )
    .unwrap();
    let second = PackSnapshot::new(
        root.clone(),
        Version {
            major: u32::MAX,
            minor: u32::MAX,
        },
        Timestamp(u64::MAX),
        digests.clone(),
    )
    .unwrap();
    assert_eq!(first.digest(), second.digest());
    assert_eq!(
        first.digest().to_string().as_bytes(),
        second.digest().to_string().as_bytes()
    );
    assert_eq!(first.taken_at().millis(), 0);
    assert_eq!(second.taken_at().millis(), u64::MAX);
    assert_eq!(first.version(), &Version::new_v1());
    assert_eq!(first.digests(), &digests);
    assert!(first.tree().is_dir());
    let manifest = first.into_manifest().into_manifest();
    assert!(manifest
        .tree()
        .children
        .iter()
        .all(|child| child.file_content.is_none()));
    assert_eq!(manifest.digest(), second.digest());
    let mut reordered = root.clone();
    reordered.children.reverse();
    assert_ne!(
        snapshot_digest(&root, &digests).unwrap(),
        snapshot_digest(&reordered, &digests).unwrap()
    );
    let duplicated = tree(vec![file("a", b"a"), file("a", b"a")]);
    assert_ne!(
        snapshot_digest(&duplicated, &digests).unwrap(),
        snapshot_digest(&tree(vec![file("a", b"a")]), &digests).unwrap()
    );
}

#[test]
fn snapshot_digest_changes_for_each_encoded_fact() {
    let original = file("x", b"x");
    let digests = ContentDigests::new();
    let baseline = snapshot_digest(&original, &digests).unwrap();
    let variants = [
        DirContextTreeNode {
            relative_path: "y".to_string(),
            ..original.clone()
        },
        DirContextTreeNode {
            node_type: NodeType::Directory,
            ..original.clone()
        },
        DirContextTreeNode {
            handling_decision: Decision::OmitContent,
            ..original.clone()
        },
        DirContextTreeNode {
            is_ignored: true,
            ..original.clone()
        },
        DirContextTreeNode {
            content_size: u64::MAX,
            ..original.clone()
        },
        DirContextTreeNode {
            last_modified_timestamp: u64::MAX,
            ..original.clone()
        },
    ];
    for variant in variants {
        assert_ne!(snapshot_digest(&variant, &digests).unwrap(), baseline);
    }
    assert_eq!(snapshot_digest(&original, &digests).unwrap(), baseline);
}

#[test]
fn content_digest_map_order_and_duplicate_inserts_have_defined_identity() {
    let root = tree(vec![file("a", b"a"), file("b", b"b")]);
    let a = ContentDigest::sha256([0; 32], 1);
    let b = ContentDigest::sha256([255; 32], u64::MAX);
    for digest in [a, b] {
        assert_eq!(digest.version(), 1);
        assert_eq!(digest.algorithm_code(), digest.algorithm().code());
        assert_eq!(SnapshotDigest(digest).to_string(), digest.to_string());
        assert_eq!(digest.as_bytes().len(), 32);
    }
    assert_eq!(a.byte_length(), 1);
    assert_eq!(b.byte_length(), u64::MAX);
    let mut first = ContentDigests::new();
    first.insert("a".to_string(), a);
    first.insert("b".to_string(), b);
    let mut reordered = ContentDigests::new();
    reordered.insert("b".to_string(), b);
    reordered.insert("a".to_string(), a);
    let expected = snapshot_digest(&root, &first).unwrap();
    assert_eq!(snapshot_digest(&root, &reordered).unwrap(), expected);
    assert_eq!(reordered.insert("a".to_string(), a), Some(a));
    assert_eq!(snapshot_digest(&root, &reordered).unwrap(), expected);
    reordered.insert("a".to_string(), b);
    assert_ne!(snapshot_digest(&root, &reordered).unwrap(), expected);
    reordered.insert("a".to_string(), a);
    reordered.insert("absent".to_string(), b);
    assert_eq!(snapshot_digest(&root, &reordered).unwrap(), expected);
    reordered.insert("a".to_string(), ContentDigest::sha256([0; 32], 2));
    assert_ne!(snapshot_digest(&root, &reordered).unwrap(), expected);
}

#[test]
fn content_prefixes_produce_repeatable_snapshots_and_manifests() {
    let bytes = b"a\xff\xfe\0\x01\r\n";
    for end in 0..=bytes.len() {
        let content = &bytes[..end];
        let digest = dctx_hash::Sha256::digest(content).unwrap();
        let digests = ContentDigests::from([("x".to_string(), digest)]);
        let root = file("x", content);
        let expected = snapshot_digest(&root, &digests).unwrap();
        let first = PackSnapshot::new(
            root.clone(),
            Version::new_v1(),
            Timestamp(0),
            digests.clone(),
        )
        .unwrap();
        let second = PackSnapshot::new(root, Version::new_v1(), Timestamp(0), digests).unwrap();
        assert_eq!(first.digest(), expected);
        assert_eq!(
            first.digest().to_string().as_bytes(),
            second.digest().to_string().as_bytes()
        );
        let manifest = first.into_manifest();
        assert!(manifest.tree().file_content.is_none());
        assert_eq!(manifest.tree().content_size, end as u64);
        assert_eq!(manifest.digest(), expected);
        assert_eq!(
            snapshot_digest(manifest.tree(), manifest.digests()).unwrap(),
            expected
        );
    }
}

#[test]
fn partial_file_bodies_do_not_suppress_metadata_changes() {
    let sample = b"same bytes";
    for end in 0..=sample.len() {
        let mut old = file("x", &sample[..end]);
        old.content_size = sample.len() as u64;
        let mut new = old.clone();
        new.last_modified_timestamp = u64::MAX;
        let report = compare_trees(Some(&old), &new);
        assert_eq!(report.has_changes, end != sample.len());
        assert_eq!(
            report.stats.files_modified,
            usize::from(end != sample.len())
        );
    }
}

#[test]
fn long_versions_and_rule_subjects_do_not_require_path_sized_inputs() {
    let many_components = format!("V1{}", ".2".repeat(65_536));
    assert_eq!(Version::parse(&many_components), Version::new_v1());
    for size in [MAX_PATH_BYTES, MAX_PATH_BYTES + 1] {
        let text = "a".repeat(size);
        let literal = parse_ignore_pattern_line(&text).unwrap();
        assert!(rule_matches(&literal, &text, &text, false));
        assert_eq!(glob_subject(&literal, "path", &text), text);
        let path_rule = parse_ignore_pattern_line(&format!("src/{text}")).unwrap();
        assert_eq!(glob_subject(&path_rule, "src/x", "x"), "src/x");
        let mut result = file(&text, b"x");
        mark_subtree_ignored(&mut result);
        assert!(result.is_ignored);
        assert!(snapshot_digest(&result, &ContentDigests::new()).is_ok());
    }
}

#[test]
fn io_details_truncate_at_every_utf8_boundary() {
    for length in [
        0,
        1,
        MAX_IO_DETAIL_BYTES - 1,
        MAX_IO_DETAIL_BYTES,
        MAX_IO_DETAIL_BYTES + 1,
        4096,
    ] {
        for suffix in ["a", "é", "雪", "🦀"] {
            let text = format!("{}{suffix}", "x".repeat(length));
            let DctxError::Io { detail } = DctxError::io(&text) else {
                panic!("expected IO error");
            };
            assert!(detail.len() <= MAX_IO_DETAIL_BYTES);
            assert!(text.starts_with(&detail));
            assert!(text.is_char_boundary(detail.len()));
            if text.len() <= MAX_IO_DETAIL_BYTES {
                assert_eq!(text, detail);
            }
            assert_eq!(DctxError::io(&text), DctxError::io(&text));
        }
    }
}

#[test]
fn every_error_has_stable_code_retryability_and_an_actionable_next_step() {
    let errors = [
        (
            DctxError::InvalidRequest {
                reason: "bad value",
            },
            ErrorCode::InvalidRequest,
            false,
        ),
        (DctxError::PolicyDenied, ErrorCode::PolicyDenied, false),
        (
            DctxError::PolicyUnavailable,
            ErrorCode::PolicyUnavailable,
            false,
        ),
        (
            DctxError::UnsupportedCapability { capability: "read" },
            ErrorCode::UnsupportedCapability,
            false,
        ),
        (
            DctxError::UnsupportedVersion { found: u32::MAX },
            ErrorCode::UnsupportedVersion,
            false,
        ),
        (
            DctxError::BudgetTooSmall {
                minimum_bytes: u64::MAX,
            },
            ErrorCode::BudgetTooSmall,
            false,
        ),
        (
            DctxError::ResourceLimit {
                resource: Resource::Entries,
                limit: 0,
            },
            ErrorCode::ResourceLimit,
            false,
        ),
        (
            DctxError::CacheLimit {
                requested: u64::MAX,
                available: 0,
            },
            ErrorCode::CacheLimit,
            false,
        ),
        (DctxError::Busy, ErrorCode::Busy, true),
        (DctxError::Cancelled, ErrorCode::Cancelled, false),
        (
            DctxError::CorruptIndex {
                reason: "bad digest",
            },
            ErrorCode::CorruptIndex,
            false,
        ),
        (DctxError::SourceChanged, ErrorCode::SourceChanged, true),
        (
            DctxError::SourceUnavailable,
            ErrorCode::SourceUnavailable,
            false,
        ),
        (DctxError::io("read failed"), ErrorCode::Io, true),
    ];
    for (error, code, retryable) in errors {
        assert_eq!(error.code(), code);
        assert_eq!(error.is_retryable(), retryable);
        let message = error.to_string();
        assert_eq!(message.as_bytes(), error.to_string().as_bytes());
        let (_, next_step) = message
            .split_once("; ")
            .expect("error must name a next step");
        assert!(!next_step.is_empty());
    }
    let error = Version {
        major: u32::MAX,
        minor: 0,
    }
    .try_calculate_next(0, 1, 999)
    .unwrap_err();
    assert!(error.to_string().contains("; "));
}

#[test]
fn shared_public_apis_return_byte_identical_results_concurrently() {
    let policy =
        SelectionPolicy::from_rules(parse_rules("*.rs\n!keep.rs\n*/x"), parse_rules("*.svg"));
    let old = tree((0..16).map(|i| file(&format!("x{i}"), b"x")).collect());
    let new = tree(vec![]);
    let expected_report = report_bytes(&compare_trees(Some(&old), &new));
    let expected_identity = snapshot_digest(&old, &ContentDigests::new()).unwrap();
    let expected_text = text_probe("雪/**/x?.rs\n!keep.rs");
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    for _ in 0..16 {
                        assert_eq!(
                            policy.decide(&context("a.rs", false, false)),
                            Decision::Exclude
                        );
                        assert_eq!(
                            policy.explain(&context("x.svg", false, false)).as_bytes(),
                            policy.explain(&context("x.svg", false, false)).as_bytes()
                        );
                        assert_eq!(
                            report_bytes(&compare_trees(Some(&old), &new)),
                            expected_report
                        );
                        assert_eq!(
                            snapshot_digest(&old, &ContentDigests::new()).unwrap(),
                            expected_identity
                        );
                        assert_eq!(text_probe("雪/**/x?.rs\n!keep.rs"), expected_text);
                        assert!(OutputBudget::default().admits(u64::MAX, u64::MAX));
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
    });
}
