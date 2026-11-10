// SPDX-License-Identifier: Apache-2.0
//! Validated traversal options and continuation commands.
//! Implements SPEC-MAP-MODE sections 3 and 4 with explicit hard bounds.
//! Uses model budgets; the CLI and library share these request rules.
use crate::{Error, Result};
use dctx_map_model::{AnswerBudget, EdgeKind, Verb};
use std::fmt::Write as _;

/// Which directions to traverse from a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Incoming relations.
    In,
    /// Outgoing relations.
    Out,
    /// Both incoming and outgoing relations.
    Both,
}

/// Options shared by library callers and `dctx map`.
#[derive(Debug, Clone)]
pub struct Request {
    /// One of the five read-only verbs.
    pub verb: Verb,
    /// Concept, path, stable id or alias, depending on the verb.
    pub target: String,
    /// Complete output budget.
    pub budget: AnswerBudget,
    /// Maximum graph depth.
    pub depth: u8,
    /// Maximum records on this page.
    pub limit: usize,
    /// Number of ranked records already consumed.
    pub offset: usize,
    /// Display kinds to include, such as `fn`, `type` or `doc`.
    pub kinds: Vec<String>,
    /// Optional place-relative directory/file filter.
    pub within: Option<String>,
    /// Relations to traverse; empty means all relations.
    pub edges: Vec<EdgeKind>,
    /// Graph traversal direction.
    pub direction: Direction,
    /// Absolute inclusive source lines within the node.
    pub lines: Option<[u32; 2]>,
    /// Permit up to 400 source lines instead of 120.
    pub full: bool,
    /// Required index revision; unavailable revisions are refused.
    pub revision: Option<String>,
    /// Packet-owned paths receiving a lexical ranking boost.
    pub focus: Vec<String>,
}
impl Request {
    /// Creates the spec defaults for one verb.
    pub fn new(verb: Verb, target: impl Into<String>) -> Self {
        Self {
            verb,
            target: target.into(),
            budget: AnswerBudget::for_verb(verb),
            depth: 1,
            limit: match verb {
                Verb::Find => 10,
                Verb::Neighbors => 20,
                _ => 60,
            },
            offset: 0,
            kinds: vec![],
            within: None,
            edges: vec![],
            direction: Direction::Both,
            lines: None,
            full: false,
            revision: None,
            focus: vec![],
        }
    }

    /// Refuses invalid combinations before any source or index IO.
    ///
    /// # Errors
    /// Refuses invalid verb limits, depth, targets, source ranges, full-mode combinations and escaping or malformed paths.
    pub fn validate(&self) -> Result<()> {
        let name = match self.verb {
            Verb::Map => "map",
            Verb::Find => "find",
            Verb::Neighbors => "neighbors",
            Verb::Explain => "explain",
            Verb::Read => "read",
        };
        self.validate_as(name)
    }

    /// Validates using a user-facing verb name in bounds errors, such as `grep` for `find`.
    /// The name affects diagnostics only; [`Self::verb`] determines the bounds.
    ///
    /// # Errors
    /// Returns the same refusals as [`Self::validate`].
    pub fn validate_as(&self, name: &str) -> Result<()> {
        let limit_max = if self.verb == Verb::Find { 40 } else { 60 };
        let depth_max = if self.verb == Verb::Neighbors { 4 } else { 3 };
        for (option, value, maximum) in [
            ("--limit", self.limit, limit_max),
            ("--depth", usize::from(self.depth), depth_max),
        ] {
            let (relation, boundary, bound) = if value == 0 {
                ("below", "minimum", 1)
            } else if value > maximum {
                ("above", "maximum", maximum)
            } else {
                continue;
            };
            let possessive = if name.ends_with('s') { "'" } else { "'s" };
            return Err(Error::Request(format!(
                "limit or depth exceeds the verb's bounds: {option} {value} is {relation} {name}{possessive} {boundary} {bound} (allowed range 1..={maximum})"
            )));
        }
        if self.verb != Verb::Map && self.target.trim().is_empty() {
            return Err(Error::Request("this verb requires a target".into()));
        }
        if self.target.len() > 4096 || self.target.chars().any(char::is_control) {
            return Err(Error::Request(
                "target is too long or contains control characters".into(),
            ));
        }
        if let Some([start, end]) = self.lines
            && (self.verb != Verb::Read
                || start == 0
                || end < start
                || end - start >= if self.full { 400 } else { 120 })
        {
            return Err(Error::Request(
                "invalid or oversized source line range".into(),
            ));
        }
        if self.full && self.verb != Verb::Read {
            return Err(Error::Request("--full is only valid with read".into()));
        }
        if let Some(path) = &self.within {
            validate_path(path)?;
        }
        for path in &self.focus {
            validate_path(path)?;
        }
        Ok(())
    }

    #[allow(clippy::expect_used, reason = "writing to a String cannot fail")]
    pub(crate) fn command(&self, offset: usize) -> String {
        let verb = match self.verb {
            Verb::Map => "map",
            Verb::Find => "find",
            Verb::Neighbors => "neighbors",
            Verb::Explain => "explain",
            Verb::Read => "read",
        };
        let mut out = format!(
            "dctx map {verb} {} --budget {}",
            quote(&self.target),
            self.budget.tokens()
        );
        if matches!(self.verb, Verb::Map | Verb::Find | Verb::Neighbors) {
            write!(out, " --limit {} --offset {offset}", self.limit)
                .expect("writing to a String cannot fail");
        }
        if matches!(self.verb, Verb::Map | Verb::Neighbors) {
            write!(out, " --depth {}", self.depth).expect("writing to a String cannot fail");
        }
        if !self.kinds.is_empty() {
            write!(out, " --kind {}", quote(&self.kinds.join(",")))
                .expect("writing to a String cannot fail");
        }
        if let Some(path) = &self.within {
            write!(out, " --in {}", quote(path)).expect("writing to a String cannot fail");
        }
        if let Some(rev) = &self.revision {
            write!(out, " --rev {}", quote(rev)).expect("writing to a String cannot fail");
        }
        if self.verb == Verb::Neighbors {
            out.push_str(match self.direction {
                Direction::In => " --dir in",
                Direction::Out => " --dir out",
                Direction::Both => " --dir both",
            });
            if !self.edges.is_empty() {
                write!(
                    out,
                    " --edge {}",
                    self.edges
                        .iter()
                        .map(|e| e.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .expect("writing to a String cannot fail");
            }
        }
        if let Some([a, b]) = self.lines {
            write!(out, " --lines {a}-{b}").expect("writing to a String cannot fail");
        }
        if self.full {
            out.push_str(" --full");
        }
        out
    }
}

pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if path != "."
        && (path.is_empty()
            || path.starts_with('/')
            || path.contains(['\\', ':'])
            || path
                .trim_end_matches('/')
                .split('/')
                .any(|p| matches!(p, "" | "." | ".."))
            || path.chars().any(char::is_control))
    {
        return Err(Error::Request("path must be relative to the place".into()));
    }
    Ok(())
}
pub(crate) fn inside(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    prefix == "."
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_errors_identify_the_option_value_verb_and_range() {
        for (verb, limit_max, depth_max, name) in [
            (Verb::Map, 60, 3, "map's"),
            (Verb::Find, 40, 3, "find's"),
            (Verb::Neighbors, 60, 4, "neighbors'"),
            (Verb::Explain, 60, 3, "explain's"),
            (Verb::Read, 60, 3, "read's"),
        ] {
            let mut request = Request::new(verb, "target");
            request.limit = limit_max;
            request.depth = depth_max;
            assert!(request.validate().is_ok());
            for (limit, depth, detail) in [
                (
                    0,
                    1,
                    format!("--limit 0 is below {name} minimum 1 (allowed range 1..={limit_max})"),
                ),
                (
                    limit_max + 1,
                    1,
                    format!(
                        "--limit {} is above {name} maximum {limit_max} (allowed range 1..={limit_max})",
                        limit_max + 1
                    ),
                ),
                (
                    1,
                    0,
                    format!("--depth 0 is below {name} minimum 1 (allowed range 1..={depth_max})"),
                ),
                (
                    1,
                    depth_max + 1,
                    format!(
                        "--depth {} is above {name} maximum {depth_max} (allowed range 1..={depth_max})",
                        depth_max + 1
                    ),
                ),
            ] {
                request.limit = limit;
                request.depth = depth;
                assert_eq!(
                    request.validate().expect_err("bounds").to_string(),
                    format!("limit or depth exceeds the verb's bounds: {detail}")
                );
            }
        }
    }

    #[test]
    fn refuses_bounds_and_path_escape() {
        let mut request = Request::new(Verb::Find, "event");
        assert!(request.validate().is_ok());
        request.limit = 41;
        assert!(request.validate().is_err());
        for bad in ["../secret", "/tmp", "a/../../b", "a\\b"] {
            assert!(validate_path(bad).is_err());
        }
        assert!(inside("crates/a/src/lib.rs", "crates/a"));
        assert!(!inside("crates/abc", "crates/a"));
    }
}
