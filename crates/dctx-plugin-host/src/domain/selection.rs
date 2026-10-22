//! Source selection decisions `S(t)` and provenance checks.
//! Implements PLUGINS.md §4.3 and Authority.tla `Decisions`/`Read`: only `Include` authorizes content,
//! `DebugVisible` is metadata only, unknown paths fail closed to `Exclude`. Depends on protocol scalars.
//! Used by grant (selection layer), authority (read/hash/delivery checks) and the broker.

use std::collections::BTreeMap;

use dctx_plugin_protocol::records::scalars::ProjectPath;

/// Effective selection decision for one project path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Decision {
    /// Content may be read, hashed and delivered.
    Include,
    /// Neither content nor metadata is exposed.
    Exclude,
    /// Listed without content.
    OmitContent,
    /// Visible for debugging metadata only.
    DebugVisible,
}

impl Decision {
    /// Whether this decision authorizes content access (read, hash or delivery).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn authorizes_content(self) -> bool {
        matches!(self, Self::Include)
    }
}

/// Current selection snapshot; paths without a decision are excluded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    decisions: BTreeMap<ProjectPath, Decision>,
}

impl Selection {
    /// Builds a selection from explicit decisions.
    pub fn new(decisions: impl IntoIterator<Item = (ProjectPath, Decision)>) -> Self {
        Self { decisions: decisions.into_iter().collect() }
    }

    /// Decision for `path`, failing closed to `Exclude`.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn decision(&self, path: &ProjectPath) -> Decision {
        self.decisions.get(path).copied().unwrap_or(Decision::Exclude)
    }

    /// Whether `path` content may be accessed.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn authorizes(&self, path: &ProjectPath) -> bool {
        self.decision(path).authorizes_content()
    }

    /// Whether every provenance constituent is currently `Include` (an empty set is authorized).
    pub fn authorizes_all<'a>(&self, provenance: impl IntoIterator<Item = &'a ProjectPath>) -> bool {
        provenance.into_iter().all(|path| self.authorizes(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> ProjectPath {
        ProjectPath::new(text).expect("path")
    }

    #[test]
    fn only_include_authorizes_content() {
        let selection = Selection::new([
            (path("a"), Decision::Include),
            (path("b"), Decision::Exclude),
            (path("c"), Decision::OmitContent),
            (path("d"), Decision::DebugVisible),
        ]);
        assert!(selection.authorizes(&path("a")));
        for denied in ["b", "c", "d", "unknown"] {
            assert!(!selection.authorizes(&path(denied)), "{denied}");
        }
        assert!(selection.authorizes_all([&path("a")]));
        assert!(!selection.authorizes_all([&path("a"), &path("d")]));
    }
}
