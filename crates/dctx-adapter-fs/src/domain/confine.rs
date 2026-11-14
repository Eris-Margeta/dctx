// SPDX-License-Identifier: Apache-2.0
//! Lexical path confinement for symlink targets.
//! Implements DX-B3 escape refusal without following links.
//! Depends only on `std::path`. The walker asks [`is_inside`] before any content read.

use std::path::{Component, Path, PathBuf};

/// Drops `.` and applies `..` without consulting the filesystem.
/// Leading parents on relative paths are preserved; absolute paths stop at their root.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push(component.as_os_str());
                }
            }
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::Prefix(_) => out.push(component.as_os_str()),
        }
    }
    out
}

/// Joins a link target onto its parent, then normalizes.
/// Absolute targets replace the parent.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn join_lexical(parent: &Path, target: &Path) -> PathBuf {
    let combined = if target.is_absolute() {
        target.to_path_buf()
    } else {
        parent.join(target)
    };
    normalize(&combined)
}

/// Whether `path` is the root or a normal-component descendant of it.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn is_inside(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }
    let Ok(suffix) = path.strip_prefix(root) else {
        return false;
    };
    suffix
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_segments_and_absolute_targets_leave_the_root() {
        let parent = std::env::temp_dir();
        let root = parent.join("proj");
        let escaped = join_lexical(&root, Path::new("../secret"));
        assert_eq!(escaped, normalize(&parent.join("secret")));
        assert!(!is_inside(&root, &escaped));
        let target = parent.join("outside/secret");
        assert!(target.is_absolute());
        let outside = join_lexical(&root, &target);
        assert!(!is_inside(&root, &outside));
        let inside = join_lexical(&root, Path::new("ok.txt"));
        assert_eq!(inside, parent.join("proj/ok.txt"));
        assert!(is_inside(&root, &inside));
        assert!(!is_inside(&root, &parent.join("proj-evil/ok.txt")));
    }
}
