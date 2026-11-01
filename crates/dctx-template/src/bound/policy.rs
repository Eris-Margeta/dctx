//! Path policy functions for bound answer rendering.

use dctx_core::FileHandlingDecision;
use dctx_core::selection::glob_match;

use crate::model::{HostCeiling, Selection};

fn under_root(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn pattern_usable(pattern: &str) -> bool {
    if pattern.is_empty()
        || pattern.len() > 4096
        || pattern.contains('\0')
        || pattern.contains('\\')
    {
        return false;
    }
    !pattern.split('/').any(|segment| segment == "..")
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let pattern_bytes = pattern.as_bytes();
    let path_bytes = path.as_bytes();
    if pattern.contains('/') {
        glob_match(pattern_bytes, path_bytes)
    } else {
        let base = match path.rfind('/') {
            Some(index) => &path[index + 1..],
            None => path,
        };
        glob_match(pattern_bytes, base.as_bytes()) || glob_match(pattern_bytes, path_bytes)
    }
}

pub(super) fn host_decision(host: &HostCeiling, path: &str) -> FileHandlingDecision {
    let mut decision = FileHandlingDecision::Exclude;
    for grant in &host.grants {
        if pattern_usable(&grant.pattern) && path_matches(&grant.pattern, path) {
            decision = grant.decision;
        }
    }
    decision
}

pub(super) fn template_decision(selection: &Selection, path: &str) -> FileHandlingDecision {
    if !selection.roots.is_empty() && !selection.roots.iter().any(|root| under_root(path, root)) {
        return FileHandlingDecision::Exclude;
    }
    let mut decision = FileHandlingDecision::Include;
    for rule in &selection.rules {
        if pattern_usable(&rule.pattern) && path_matches(&rule.pattern, path) {
            decision = rule.decision;
        }
    }
    decision
}
