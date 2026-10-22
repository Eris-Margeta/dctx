//! Token stream for JavaScript, TypeScript and JSX source.
//! Implements PARSERS-v1 lexing rules: comments, strings, nested templates, regex versus
//! division, JSX segments and Unicode identifiers, all bounded by `Limits`.
//! Used by the structural walker; nothing here allocates per character.
mod brackets;
mod jsx;
mod literals;
mod scan;

use crate::domain::{Diagnostic, DiagnosticReason, Dialect, LexError, Limits};

/// Token category; keywords are `Ident` and compared by text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Ident,
    Private,
    Number,
    Str,
    BadStr,
    Template,
    TemplateHead,
    TemplateMiddle,
    TemplateTail,
    Regex,
    Jsx,
    JsxOpen,
    JsxClose,
    Punct,
}

/// One significant token; `doc` is 1 + the index of an adjacent preceding comment.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Token {
    pub kind: Kind,
    pub start: u32,
    pub end: u32,
    pub nl: bool,
    pub doc: u32,
}

/// Lexer output: tokens, comment spans, bracket partners and problems.
pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<(u32, u32)>,
    pub partner: Vec<u32>,
    pub diagnostics: Vec<Diagnostic>,
    pub hashbang: bool,
    pub line_starts: Vec<u32>,
}

/// Partner value of a bracket without a match.
pub(crate) const NONE: u32 = u32::MAX;

impl Lexed {
    /// One-based line of a byte offset; rows advance on `\n` only.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub fn line(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&start| start <= byte) as u32
    }
}

/// Lexes `source` and pairs its brackets.
#[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
#[allow(clippy::similar_names, reason = "The scanner and its lexed output have established role names used consistently across lexer adapters.")]
pub(crate) fn lex(source: &str, dialect: Dialect, limits: &Limits) -> Result<Lexed, LexError> {
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(i, _)| i as u32 + 1),
    );
    let mut lexer = scan::Lexer::new(source, dialect, limits.max_depth);
    lexer.run()?;
    let mut lexed = Lexed {
        tokens: lexer.tokens,
        comments: lexer.comments,
        partner: Vec::new(),
        diagnostics: Vec::new(),
        hashbang: lexer.hashbang,
        line_starts,
    };
    let mut problems = lexer.problems;
    lexed.partner = brackets::pair(source, &lexed.tokens, &mut problems, limits.max_depth)
        .map_err(|byte| LexError::TooDeep {
            line: lexed.line(byte),
            limit: limits.max_depth,
        })?;
    problems.sort_by_key(|p| p.0);
    lexed.diagnostics = problems
        .into_iter()
        .map(|(byte, reason)| Diagnostic {
            line: lexed.line(byte),
            byte,
            reason,
        })
        .collect();
    Ok(lexed)
}

/// Problems found while lexing, as (byte, reason).
pub(crate) type Problems = Vec<(u32, DiagnosticReason)>;

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str, dialect: Dialect) -> Vec<(Kind, &str)> {
        let lexed = lex(source, dialect, &Limits::default()).unwrap();
        lexed
            .tokens
            .iter()
            .map(|t| (t.kind, &source[t.start as usize..t.end as usize]))
            .collect()
    }

    #[test]
    fn regex_and_division_follow_the_previous_token() {
        let tokens = kinds("a = b / c / d; x = /[/]+/g.test(y)", Dialect::JavaScript);
        assert_eq!(tokens[3], (Kind::Punct, "/"));
        assert!(tokens.contains(&(Kind::Regex, "/[/]+/g")));
        let tokens = kinds("if (x) /a'b/.test(s); f(n)", Dialect::JavaScript);
        assert!(tokens.contains(&(Kind::Regex, "/a'b/")));
        assert!(tokens.contains(&(Kind::Ident, "f")));
    }

    #[test]
    fn templates_nest_and_unicode_identifiers_are_single_tokens() {
        let tokens = kinds("`a${ `b${ c({}) }` }d` + café·ñ", Dialect::JavaScript);
        assert_eq!(tokens[0], (Kind::TemplateHead, "`a${"));
        assert_eq!(tokens[1], (Kind::TemplateHead, "`b${"));
        assert!(tokens.contains(&(Kind::TemplateTail, "}d`")));
        assert_eq!(tokens.last(), Some(&(Kind::Ident, "café·ñ")));
    }

    #[test]
    fn jsx_text_is_opaque_and_containers_are_code() {
        let tokens = kinds("x = <a b=\"/\">it's {run()}</a> / 2", Dialect::JavaScript);
        assert_eq!(tokens[2], (Kind::Jsx, "<a b=\"/\">it's "));
        assert_eq!(tokens[3].0, Kind::JsxOpen);
        assert!(tokens.contains(&(Kind::Ident, "run")));
        assert!(tokens.contains(&(Kind::Jsx, "</a>")));
        assert_eq!(tokens[tokens.len() - 2], (Kind::Punct, "/"));
        let typed = kinds("x = <T>y", Dialect::TypeScript);
        assert_eq!(typed[2], (Kind::Punct, "<"));
    }

    #[test]
    fn partners_and_lines_are_consistent() {
        let lexed = lex("f(\n[a]\n)", Dialect::JavaScript, &Limits::default()).unwrap();
        assert_eq!(lexed.partner[1], 5);
        assert_eq!(lexed.partner[5], 1);
        assert_eq!(lexed.line(lexed.tokens[5].start), 3);
        assert_eq!(lexed.diagnostics, []);
        let open = lex("f(", Dialect::JavaScript, &Limits::default()).unwrap();
        assert_eq!(open.partner[1], NONE);
        assert_eq!(
            open.diagnostics[0].reason,
            DiagnosticReason::UnbalancedBracket
        );
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused() {
        let limits = Limits {
            max_depth: 4,
            ..Limits::default()
        };
        assert!(matches!(
            lex("((((((x))))))", Dialect::JavaScript, &limits),
            Err(LexError::TooDeep { line: 1, limit: 4 })
        ));
        assert!(lex("((x))", Dialect::JavaScript, &limits).is_ok());
    }
}
