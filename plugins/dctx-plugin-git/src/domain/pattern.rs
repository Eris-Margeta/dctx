// SPDX-License-Identifier: Apache-2.0
//! One `.gitignore` line classified as blank, comment or rule (negation, directory-only, last component).
//! Implements the gitignore line grammar used by G-COVERAGE: `#` comments, unescaped trailing spaces
//! dropped, `!` negation, trailing `/` directory-only; escapes stay for the glob tokenizer. Pure; no IO.
//! Used by `domain::coverage`.

use crate::domain::glob::{Glob, Unsupported};

/// One parsed rule line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// `!` re-includes matching paths.
    pub negated: bool,
    /// A trailing `/` limits the rule to directories.
    pub dir_only: bool,
    /// Pattern text without negation, trailing spaces or the directory slash.
    pub body: String,
}

/// A classified line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// Empty or only spaces.
    Blank,
    /// A `#` comment (markers and the header included).
    Comment,
    /// A pattern.
    Rule(Rule),
}

fn strip_trailing_spaces(line: &str) -> &str {
    let mut body = line;
    while let Some(rest) = body.strip_suffix(' ') {
        let backslashes = rest.bytes().rev().take_while(|b| *b == b'\\').count();
        if backslashes % 2 == 1 {
            break;
        }
        body = rest;
    }
    body
}

/// Classifies one line (without its line ending).
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn parse_line(line: &str) -> Line {
    if line.starts_with('#') {
        return Line::Comment;
    }
    let body = strip_trailing_spaces(line);
    if body.is_empty() {
        return Line::Blank;
    }
    let (negated, body) = match body.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let escaped_slash = body
        .strip_suffix('/')
        .is_some_and(|rest| rest.ends_with('\\'));
    let (dir_only, body) = match body.strip_suffix('/') {
        Some(rest) if !escaped_slash => (true, rest),
        _ => (false, body),
    };
    Line::Rule(Rule {
        negated,
        dir_only,
        body: body.to_owned(),
    })
}

impl Rule {
    /// The glob of the last path component, which any name the rule matches must satisfy.
    /// # Errors
    /// Returns `Unsupported` for forms outside the supported last-component glob profile.
    pub fn last_component(&self) -> Result<Glob, Unsupported> {
        let last = self.body.rsplit('/').next().unwrap_or(&self.body);
        if last.is_empty() {
            return Err(Unsupported);
        }
        Glob::parse(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(line: &str) -> Rule {
        match parse_line(line) {
            Line::Rule(rule) => rule,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn classifies_lines() {
        assert_eq!(parse_line(""), Line::Blank);
        assert_eq!(parse_line("   "), Line::Blank);
        assert_eq!(parse_line("# [dctx v0.26.0]"), Line::Comment);
        assert_eq!(
            parse_line("\\#x"),
            Line::Rule(Rule {
                negated: false,
                dir_only: false,
                body: "\\#x".into()
            })
        );
        assert_eq!(
            rule("!*.llmcontext.txt  "),
            Rule {
                negated: true,
                dir_only: false,
                body: "*.llmcontext.txt".into()
            }
        );
        assert_eq!(
            rule("!keep/"),
            Rule {
                negated: true,
                dir_only: true,
                body: "keep".into()
            }
        );
        assert_eq!(rule("a\\ ").body, "a\\ ");
        assert!(!rule("\\!x").negated);
    }

    #[test]
    fn last_component_of_paths() {
        assert!(
            rule("!out/p.llmcontext.txt")
                .last_component()
                .expect("ok")
                .matches("p.llmcontext.txt")
        );
        assert!(
            rule("!/p.txt")
                .last_component()
                .expect("ok")
                .matches("p.txt")
        );
        assert!(
            rule("!**")
                .last_component()
                .expect("ok")
                .matches("anything")
        );
        assert_eq!(rule("![abc").last_component(), Err(Unsupported));
        assert_eq!(rule("!a//").last_component(), Err(Unsupported));
    }
}
