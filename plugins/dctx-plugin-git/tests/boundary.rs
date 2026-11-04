// SPDX-License-Identifier: Apache-2.0
//! Boundary audits: no direct filesystem/process/network access in the worker, `forbid(unsafe_code)`, and the
//! source manifest/handshake agreeing with the frozen manifest-git fixture.
//! Implements PLUGINS.md §7.2 PL-G "no direct fs/process access in worker" and G-PROFILE.
//! Reads the crate's own sources at compile time; depends on tests/common for fixtures.

mod common;

use common::document;
use dctx_plugin_git::adapters::worker::{OPTIONAL_APIS, PLUGIN_ID, REQUIRED_APIS};
use dctx_plugin_git::domain::outcome::{DiagnosticCode, Reason};
use dctx_plugin_protocol::{Json, decode_object};

const SOURCES: [(&str, &str); 23] = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("main.rs", include_str!("../src/main.rs")),
    ("ports/mod.rs", include_str!("../src/ports/mod.rs")),
    ("adapters/mod.rs", include_str!("../src/adapters/mod.rs")),
    ("adapters/link.rs", include_str!("../src/adapters/link.rs")),
    ("adapters/wire_broker.rs", include_str!("../src/adapters/wire_broker.rs")),
    ("adapters/worker.rs", include_str!("../src/adapters/worker.rs")),
    ("app/mod.rs", include_str!("../src/app/mod.rs")),
    ("app/binding.rs", include_str!("../src/app/binding.rs")),
    ("app/failure.rs", include_str!("../src/app/failure.rs")),
    ("app/invoke.rs", include_str!("../src/app/invoke.rs")),
    ("app/protect.rs", include_str!("../src/app/protect.rs")),
    ("app/stage.rs", include_str!("../src/app/stage.rs")),
    ("app/transfer.rs", include_str!("../src/app/transfer.rs")),
    ("domain/mod.rs", include_str!("../src/domain/mod.rs")),
    ("domain/command.rs", include_str!("../src/domain/command.rs")),
    ("domain/coverage.rs", include_str!("../src/domain/coverage.rs")),
    ("domain/glob.rs", include_str!("../src/domain/glob.rs")),
    ("domain/outcome.rs", include_str!("../src/domain/outcome.rs")),
    ("domain/pattern.rs", include_str!("../src/domain/pattern.rs")),
    ("domain/plan.rs", include_str!("../src/domain/plan.rs")),
    ("domain/rules.rs", include_str!("../src/domain/rules.rs")),
    ("domain/text.rs", include_str!("../src/domain/text.rs")),
];

const FORBIDDEN: [&str; 12] = [
    "std::fs", "fs::", "std::net", "TcpStream", "UdpSocket", "Command::new", "process::Command", "env::var",
    "File::", "OpenOptions", "unsafe {", "unsafe fn",
];

#[test]
fn worker_sources_have_no_direct_effects_and_carry_headers() {
    for (name, source) in SOURCES {
        for token in FORBIDDEN {
            assert!(!source.contains(token), "{name} contains {token}");
        }
        let body = source.strip_prefix("// SPDX-License-Identifier: Apache-2.0\n").unwrap_or(source);
        assert!(body.starts_with("//!"), "{name} lacks its //! header");
        let lines = body.lines().count();
        assert!(lines <= 300, "{name} has {lines} lines");
    }
    assert!(SOURCES[0].1.contains("#![forbid(unsafe_code)]") && SOURCES[1].1.contains("#![forbid(unsafe_code)]"));
    let manifest = include_str!("../Cargo.toml");
    assert!(manifest.contains("dctx-plugin-protocol") && !manifest.contains("git2") && !manifest.contains("gix"));
}

fn strings(value: &Json, key: &str) -> Vec<String> {
    value.get(key).and_then(Json::as_array).unwrap_or(&[]).iter().filter_map(Json::as_str).map(str::to_owned).collect()
}

#[test]
fn source_manifest_and_handshake_match_the_frozen_profile() {
    let fixture = document("fixtures/valid/manifest-git.json");
    let source = decode_object(include_bytes!("../plugin.json"), common::CORPUS).expect("plugin.json");
    for key in ["manifest_version", "id", "name", "version", "license", "protocol", "host_api", "hooks", "commands", "capabilities", "config", "limits", "reason_codes", "diagnostic_codes"] {
        let (left, right) = (source.get(key).expect(key), fixture.get(key).expect(key));
        assert!(left.same_value(right), "plugin.json {key} differs from manifest-git");
    }
    assert_eq!(source.get("id").and_then(Json::as_str), Some(PLUGIN_ID));
    assert_eq!(source.get("version").and_then(Json::as_str), Some(env!("CARGO_PKG_VERSION")));
    let reasons = strings(&fixture, "reason_codes");
    for reason in [
        Reason::Completed, Reason::AlreadyProtected, Reason::NotARepository, Reason::MissingIgnore, Reason::OutsideProject,
        Reason::RulesWrittenNotStaged, Reason::RulesConflict, Reason::CoverageUnknown, Reason::UnsupportedRepositoryLayout,
        Reason::StagingDenied, Reason::GitMissing,
    ] {
        assert!(reasons.iter().any(|code| code == reason.as_str()), "{reason:?} undeclared");
    }
    let diagnostics = strings(&fixture, "diagnostic_codes");
    for code in [
        DiagnosticCode::StagingDenied, DiagnosticCode::GitMissing, DiagnosticCode::WholeFileStaging,
        DiagnosticCode::AlreadyTracked, DiagnosticCode::RulesWrittenNotStaged, DiagnosticCode::RulesConflict,
        DiagnosticCode::CoverageUnknown,
    ] {
        assert!(diagnostics.iter().any(|declared| declared == code.as_str()), "{code:?} undeclared");
    }
    let selected = document("fixtures/valid/hello-selected.json");
    let result = selected.get("result").expect("result");
    assert_eq!(strings(result, "required_apis"), REQUIRED_APIS);
    assert_eq!(strings(result, "optional_apis"), OPTIONAL_APIS);
}
