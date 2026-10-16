//! Owned rule-file texts the facade can rebuild a selection from.
//! Implements PLUGINS.md §6.2: a `.gitignore` edit is rule-file input, and only the host reloads it.
//! Depends on [`dctx_core::selection`]. The session stores this beside the compiled policy.
//! Without these texts a changed ignore file cannot be revalidated, and the pack is refused.

use dctx_core::selection::{RuleTexts, SelectionPolicy};

/// Caller-supplied rule files. `None` means that file was absent when the session was built.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleSources {
    /// Global ignore rules.
    pub global_ignore: Option<String>,
    /// Global omit-content rules.
    pub global_omit: Option<String>,
    /// Local ignore rules, including `.gitignore` when the caller loaded it.
    pub local_ignore: Option<String>,
    /// Local omit-content rules.
    pub local_omit: Option<String>,
}

impl RuleSources {
    /// The selection policy these texts compile to, including the built-in layer.
    pub fn policy(&self) -> SelectionPolicy {
        SelectionPolicy::from_texts(&RuleTexts {
            global_ignore: self.global_ignore.as_deref(),
            global_omit: self.global_omit.as_deref(),
            local_ignore: self.local_ignore.as_deref(),
            local_omit: self.local_omit.as_deref(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_core::domain::{FileHandlingDecision, ItemContext};

    #[test]
    fn local_ignore_rebuild_excludes_only_the_new_rule() {
        let open = RuleSources::default().policy();
        let secret = ItemContext { relative_path: "src/secret.rs", name: "secret.rs", is_directory: false, debug_mode: false };
        assert_eq!(open.decide(&secret), FileHandlingDecision::Include);
        let closed = RuleSources { local_ignore: Some("secret.rs\n".to_string()), ..RuleSources::default() }.policy();
        assert_eq!(closed.decide(&secret), FileHandlingDecision::Exclude);
        let kept = ItemContext { relative_path: "src/lib.rs", name: "lib.rs", is_directory: false, debug_mode: false };
        assert_eq!(closed.decide(&kept), FileHandlingDecision::Include);
    }
}
