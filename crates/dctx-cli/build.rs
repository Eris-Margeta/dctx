//! Build script: embeds the short git commit of the source tree as `DCTX_GIT_SHA` for `dctx --version`.
//! Runs `git rev-parse --short HEAD` in the crate directory; without git, outside a checkout or on any error the
//! value is `unknown`. Uses only the standard library. Reruns when `HEAD` or the branch it names moves.

use std::path::Path;
use std::process::Command;

fn main() {
    let sha = git(&["rev-parse", "--short", "HEAD"]).filter(|sha| !sha.is_empty() && sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
    println!("cargo:rustc-env=DCTX_GIT_SHA={}", sha.as_deref().unwrap_or("unknown"));
    println!("cargo:rerun-if-changed=build.rs");
    // Watch only files that exist: Cargo reruns a script on every build when a watched path is missing.
    let mut watched = vec![git(&["rev-parse", "--git-path", "HEAD"])];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(git(&["rev-parse", "--git-path", &branch]));
        watched.push(git(&["rev-parse", "--git-path", "packed-refs"]));
    }
    for path in watched.into_iter().flatten() {
        if Path::new(&path).is_file() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// Runs `git <args>` and returns its trimmed stdout, or `None` when git is missing or exits non-zero.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(|text| text.trim().to_owned())
}
