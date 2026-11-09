// SPDX-License-Identifier: Apache-2.0
//! `dctx --version` names the release and the build commit (v1 card 7).
//! Depends on the compiled `dctx-cli` binary; `build.rs` embeds the short git sha or `unknown`.
//! Used by `cargo test -p dctx-cli`.

use std::process::Command;

/// Runs the binary with one argument and returns its stdout, asserting a zero exit.
fn stdout_of(arg: &str) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_dctx-cli"))
        .arg(arg)
        .output()
        .expect("run dctx-cli");
    assert!(
        output.status.success(),
        "{arg} exit {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}

#[test]
fn version_starts_with_release_and_ends_with_build_sha() {
    for arg in ["--version", "-v", "version"] {
        let line = stdout_of(arg);
        assert!(line.starts_with("dctx 1.3.0 ("), "{arg}: {line:?}");
        let sha = line
            .trim_end()
            .strip_prefix("dctx 1.3.0 (")
            .and_then(|rest| rest.strip_suffix(')'))
            .expect("dctx 1.3.0 (<sha>)");
        assert!(
            sha == "unknown"
                || (sha.len() >= 7 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())),
            "{arg}: sha {sha:?}"
        );
        assert_eq!(line.lines().count(), 1, "{arg}: {line:?}");
    }
}

#[test]
fn version_matches_the_package_version() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "1.3.0");
    assert!(stdout_of("--version").starts_with(&format!("dctx {} (", env!("CARGO_PKG_VERSION"))));
}
