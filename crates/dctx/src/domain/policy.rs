//! Effective policy: host grant, then selection, then the DTS plan. Each step can only narrow.
//! Implements ARCHITECTURE-PROPOSAL §2 (host ∩ organization ∩ session) and DTS v2.
//! Depends on dctx-core selection and dctx-template plans. The session holds a [`PolicyView`].
//! Empty host grants exclude only when the caller chose [`HostGrants::Ceiling`]. [`HostGrants::PermitAll`] does not.

use dctx_core::domain::{FileHandlingDecision, ItemContext};
use dctx_core::selection::{glob_match, SelectionPolicy};
use dctx_template::{ContextPlan, HostCeiling, SelectRule};

use super::error::DctxError;

/// How the session reads [`HostCeiling::grants`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostGrants {
    /// Every path starts at Include. A template or a selection rule can still narrow it.
    PermitAll,
    /// Unmatched paths start at Exclude. Grant rules apply last-match and never expand later.
    Ceiling,
}

/// Parser the installed plan asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParserChoice {
    /// Line-oriented extractor. Coverage stays `structural`.
    Structural,
    /// `dctx-lex-rust` when the feature is on and the file is Rust.
    Syn,
    /// Names and hashes only.
    Opaque,
    /// Not linked. Snapshot returns `syntax-tree-sitter`.
    TreeSitter,
}

/// Borrowed view of the policies a walk or a query consults.
#[derive(Debug, Clone, Copy)]
pub struct PolicyView<'a> {
    /// Caller selection, including hardcoded rules when built from texts.
    pub selection: &'a SelectionPolicy,
    /// Organization ceiling.
    pub ceiling: &'a HostCeiling,
    /// Whether empty grants mean permit-all or exclude-all.
    pub grants: HostGrants,
    /// Installed plan, if any.
    pub plan: Option<&'a ContextPlan>,
    /// View name inside the plan.
    pub view: &'a str,
}

impl<'a> PolicyView<'a> {
    /// The decision for `path`. The empty root is decided too; the walk still lists it.
    pub fn decide(&self, path: &str, is_dir: bool) -> Result<FileHandlingDecision, DctxError> {
        let host = host_decision(self.grants, self.ceiling, path);
        let name = path.rsplit('/').next().unwrap_or(path);
        let selected = self.selection.decide(&ItemContext {
            relative_path: path,
            name,
            is_directory: is_dir,
            debug_mode: false,
        });
        let planned = plan_decision(self.plan, self.view, path)?;
        Ok(narrower(narrower(host, selected), planned))
    }

    /// Parser named by the plan. No plan means structural.
    pub(crate) fn parser(&self) -> Result<ParserChoice, DctxError> {
        let Some(plan) = self.plan else {
            return Ok(ParserChoice::Structural);
        };
        let view = plan
            .views
            .iter()
            .find(|view| view.name == self.view)
            .ok_or(DctxError::PolicyUnavailable)?;
        if view.index.is_empty() {
            return Ok(ParserChoice::Structural);
        }
        let index = plan
            .indexes
            .iter()
            .find(|index| index.name == view.index)
            .ok_or(DctxError::PolicyUnavailable)?;
        match index.parser.as_str() {
            "structural" | "syntax" => Ok(ParserChoice::Structural),
            "syn" => Ok(ParserChoice::Syn),
            "opaque" => Ok(ParserChoice::Opaque),
            "tree-sitter" => Ok(ParserChoice::TreeSitter),
            _ => Err(DctxError::UnsupportedCapability {
                capability: "template",
                detail: super::error::bounded_detail(&index.parser),
            }),
        }
    }

    /// Languages admitted by the plan. Empty means every language.
    pub(crate) fn languages(&self) -> Result<Option<Vec<String>>, DctxError> {
        let Some(plan) = self.plan else {
            return Ok(None);
        };
        let view = plan
            .views
            .iter()
            .find(|view| view.name == self.view)
            .ok_or(DctxError::PolicyUnavailable)?;
        if view.selection.is_empty() {
            return Ok(None);
        }
        let selection = plan
            .selections
            .iter()
            .find(|selection| selection.name == view.selection)
            .ok_or(DctxError::PolicyUnavailable)?;
        if selection.languages.is_empty() {
            Ok(None)
        } else {
            Ok(Some(selection.languages.clone()))
        }
    }

    /// Refuses a plan the build cannot execute. Structural plans pass.
    pub(crate) fn check(&self) -> Result<(), DctxError> {
        match self.parser()? {
            ParserChoice::TreeSitter => Err(DctxError::unsupported("syntax-tree-sitter")),
            ParserChoice::Syn if !cfg!(feature = "syntax-rust") => Err(DctxError::unsupported("syntax-rust")),
            ParserChoice::Structural | ParserChoice::Opaque | ParserChoice::Syn => {
                let _ = self.languages()?;
                Ok(())
            }
        }
    }
}

fn host_decision(grants: HostGrants, ceiling: &HostCeiling, path: &str) -> FileHandlingDecision {
    match grants {
        HostGrants::PermitAll => FileHandlingDecision::Include,
        HostGrants::Ceiling => {
            let mut decision = FileHandlingDecision::Exclude;
            for grant in &ceiling.grants {
                if pattern_usable(&grant.pattern) && path_matches(&grant.pattern, path) {
                    decision = grant.decision;
                }
            }
            decision
        }
    }
}

fn plan_decision(plan: Option<&ContextPlan>, view_name: &str, path: &str) -> Result<FileHandlingDecision, DctxError> {
    let Some(plan) = plan else {
        return Ok(FileHandlingDecision::Include);
    };
    let view = plan.views.iter().find(|view| view.name == view_name).ok_or(DctxError::PolicyUnavailable)?;
    if view.selection.is_empty() {
        return Ok(FileHandlingDecision::Include);
    }
    let selection = plan
        .selections
        .iter()
        .find(|selection| selection.name == view.selection)
        .ok_or(DctxError::PolicyUnavailable)?;
    if !selection.roots.is_empty() && !path.is_empty() && !selection.roots.iter().any(|root| under_root(path, root)) {
        return Ok(FileHandlingDecision::Exclude);
    }
    Ok(apply_rules(&selection.rules, path))
}

fn apply_rules(rules: &[SelectRule], path: &str) -> FileHandlingDecision {
    let mut decision = FileHandlingDecision::Include;
    for rule in rules {
        if pattern_usable(&rule.pattern) && path_matches(&rule.pattern, path) {
            decision = rule.decision;
        }
    }
    decision
}

pub(crate) fn narrower(host: FileHandlingDecision, requested: FileHandlingDecision) -> FileHandlingDecision {
    if content_rank(requested) > content_rank(host) || visibility_rank(requested) > visibility_rank(host) {
        host
    } else {
        requested
    }
}

fn content_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include => 2,
        FileHandlingDecision::OmitContent | FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}

fn visibility_rank(decision: FileHandlingDecision) -> u8 {
    match decision {
        FileHandlingDecision::Include | FileHandlingDecision::OmitContent => 2,
        FileHandlingDecision::DebugVisible => 1,
        FileHandlingDecision::Exclude => 0,
    }
}

fn under_root(path: &str, root: &str) -> bool {
    path == root || path.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'))
}

fn pattern_usable(pattern: &str) -> bool {
    if pattern.is_empty() || pattern.len() > 4096 || pattern.contains('\0') || pattern.contains('\\') {
        return false;
    }
    !pattern.split('/').any(|segment| segment == "..")
}

fn path_matches(pattern: &str, path: &str) -> bool {
    if pattern.contains('/') {
        glob_match(pattern.as_bytes(), path.as_bytes())
    } else {
        let base = match path.rfind('/') {
            Some(index) => &path[index + 1..],
            None => path,
        };
        glob_match(pattern.as_bytes(), base.as_bytes()) || glob_match(pattern.as_bytes(), path.as_bytes())
    }
}

/// True when every non-empty ancestor of `path`, and `path` itself, is Include.
pub(crate) fn ancestors_include(
    decide: impl Fn(&str, bool) -> Result<FileHandlingDecision, DctxError>,
    path: &str,
) -> Result<bool, DctxError> {
    if path.is_empty() {
        return Ok(decide(path, true)? == FileHandlingDecision::Include);
    }
    let mut cursor = String::new();
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            cursor.push('/');
        }
        cursor.push_str(segment);
        let is_last = cursor.len() == path.len();
        if decide(&cursor, !is_last)? != FileHandlingDecision::Include {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_template::GrantRule;

    #[test]
    fn narrowing_never_expands_and_ceiling_starts_excluded() {
        use FileHandlingDecision::{DebugVisible, Exclude, Include, OmitContent};
        assert_eq!(narrower(Include, Exclude), Exclude);
        assert_eq!(narrower(Exclude, Include), Exclude);
        assert_eq!(narrower(Include, OmitContent), OmitContent);
        assert_eq!(narrower(OmitContent, Include), OmitContent);
        assert_eq!(narrower(DebugVisible, OmitContent), DebugVisible);
        let mut ceiling = HostCeiling::base();
        assert_eq!(host_decision(HostGrants::PermitAll, &ceiling, "src/lib.rs"), Include);
        assert_eq!(host_decision(HostGrants::Ceiling, &ceiling, "src/lib.rs"), Exclude);
        ceiling.grants.push(GrantRule { pattern: "src/**".to_string(), decision: Include });
        assert_eq!(host_decision(HostGrants::Ceiling, &ceiling, "src/lib.rs"), Include);
        assert_eq!(host_decision(HostGrants::Ceiling, &ceiling, "README.md"), Exclude);
    }
}
