// SPDX-License-Identifier: Apache-2.0
//! Build script: embeds the short git commit of the source tree as `DCTX_GIT_SHA` for `dctx --version`.
//! Never spawns git (decision 078): it reads `HEAD`, the branch ref and `packed-refs` as plain files of the nearest `.git`
//! (a directory, or a worktree's `gitdir:` file). A `DCTX_GIT_SHA` in the build environment wins; outside a checkout or
//! on any error the value is `unknown`. Uses only the standard library. Reruns when `HEAD` or the branch it names moves.

use std::fs;
use std::path::{Path, PathBuf};

/// Hex digits kept, as `git rev-parse --short` prints by default.
const SHORT: usize = 7;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DCTX_GIT_SHA");
    let mut watched = Vec::new();
    let preset = std::env::var("DCTX_GIT_SHA").ok().filter(|sha| is_hex(sha));
    let sha = preset.or_else(|| {
        let start = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
        head_commit(&start, &mut watched)
    });
    let short = sha.map(|sha| sha.chars().take(SHORT).collect::<String>());
    println!(
        "cargo:rustc-env=DCTX_GIT_SHA={}",
        short.as_deref().unwrap_or("unknown")
    );
    // Watch only files that exist: Cargo reruns a script on every build when a watched path is missing.
    for path in watched.iter().filter(|path| path.is_file()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

/// True for a non-empty run of ASCII hex digits.
fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The commit `HEAD` names in the checkout that holds `start`, pushing every file it read onto `watched`.
fn head_commit(start: &Path, watched: &mut Vec<PathBuf>) -> Option<String> {
    let (git_dir, common) = git_dirs(start)?;
    let head_path = git_dir.join("HEAD");
    watched.push(head_path.clone());
    let head = fs::read_to_string(&head_path).ok()?;
    let head = head.trim();
    let Some(name) = head.strip_prefix("ref:").map(str::trim) else {
        return is_hex(head).then(|| head.to_owned());
    };
    for dir in [&git_dir, &common] {
        let loose = dir.join(name);
        watched.push(loose.clone());
        if let Ok(text) = fs::read_to_string(&loose) {
            let sha = text.trim();
            return is_hex(sha).then(|| sha.to_owned());
        }
    }
    let packed = common.join("packed-refs");
    watched.push(packed.clone());
    let text = fs::read_to_string(&packed).ok()?;
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .find(|(sha, reference)| reference.trim() == name && is_hex(sha))
        .map(|(sha, _)| sha.to_owned())
}

/// The git dir of the nearest `.git` at or above `start` and its common dir (the same, unless a `commondir` file says
/// otherwise, as in a linked worktree).
fn git_dirs(start: &Path) -> Option<(PathBuf, PathBuf)> {
    let dot = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|path| path.exists())?;
    let git_dir = if dot.is_dir() {
        dot
    } else {
        let text = fs::read_to_string(&dot).ok()?;
        let target = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
        if target.is_absolute() {
            target
        } else {
            dot.parent()?.join(target)
        }
    };
    let common = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => {
            let target = PathBuf::from(text.trim());
            if target.is_absolute() {
                target
            } else {
                git_dir.join(target)
            }
        }
        Err(_) => git_dir.clone(),
    };
    Some((git_dir, common))
}
