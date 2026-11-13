// SPDX-License-Identifier: Apache-2.0
//! Inline scan of paragraph text for code spans and inline link or image destinations.
//! Implements `CommonMark` 0.31 §2.4 escapes, §6.1 code spans, §6.3 inline links, §6.5 autolinks.
//! Pure, linear-time apart from bounded title scans; used by the outline step.
use crate::ReferenceKind;

/// Reference found in paragraph text; offsets index that text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InlineRef {
    pub kind: ReferenceKind,
    pub name: String,
    pub start: usize,
    pub end: usize,
}

/// Longest link title scanned, so failed link attempts stay cheap.
const MAX_TITLE: usize = 4096;
/// Deepest balanced parentheses in a bare destination.
const MAX_PARENS: usize = 32;

struct Opener {
    image: bool,
    active: bool,
}

pub(crate) fn scan(text: &str, max_brackets: usize) -> Vec<InlineRef> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut openers: Vec<Opener> = Vec::new();
    let mut unmatched_runs: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += if bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) {
                    2
                } else {
                    1
                }
            }
            b'`' => {
                let run = run_len(bytes, i);
                let closer = if unmatched_runs.contains(&run) {
                    None
                } else {
                    closing_run(bytes, i + run, run)
                };
                if let Some(close) = closer {
                    let name = text[i + run..close]
                        .trim_matches('`')
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !name.is_empty() {
                        found.push(InlineRef {
                            kind: ReferenceKind::CodeSpan,
                            name,
                            start: i,
                            end: close + run,
                        });
                    }
                    i = close + run;
                } else {
                    unmatched_runs.push(run);
                    i += run;
                }
            }
            b'<' => i = autolink_end(bytes, i).unwrap_or(i + 1),
            b'!' if bytes.get(i + 1) == Some(&b'[') => {
                if openers.len() < max_brackets {
                    openers.push(Opener {
                        image: true,
                        active: true,
                    });
                }
                i += 2;
            }
            b'[' => {
                if openers.len() < max_brackets {
                    openers.push(Opener {
                        image: false,
                        active: true,
                    });
                }
                i += 1;
            }
            b']' => {
                let opener = openers.pop();
                #[allow(
                    clippy::manual_option_zip,
                    reason = "Option::zip eagerly parses destinations even when no active link opener exists."
                )]
                let link = opener
                    .filter(|o| o.active && bytes.get(i + 1) == Some(&b'('))
                    .and_then(|o| destination(text, i + 2).map(|d| (o, d)));
                match link {
                    Some((opener, (target, close))) => {
                        let name = target
                            .map(|(start, end)| {
                                (start, end, text[start..end].trim_matches(['<', '>']))
                            })
                            .filter(|(_, _, name)| !name.is_empty());
                        if let Some((start, end, name)) = name {
                            found.push(InlineRef {
                                kind: classify(name),
                                name: name.to_owned(),
                                start,
                                end,
                            });
                        }
                        if !opener.image {
                            openers
                                .iter_mut()
                                .filter(|o| !o.image)
                                .for_each(|o| o.active = false);
                        }
                        i = close + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    found
}

fn run_len(bytes: &[u8], at: usize) -> usize {
    bytes[at..].iter().take_while(|b| **b == bytes[at]).count()
}

/// Start of the next backtick run of exactly `len`, scanning from `from`.
fn closing_run(bytes: &[u8], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let run = run_len(bytes, j);
            if run == len {
                return Some(j);
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// End of a `<scheme:...>` autolink starting at `at`.
fn autolink_end(bytes: &[u8], at: usize) -> Option<usize> {
    let rest = &bytes[at + 1..];
    let scheme = rest
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
        .count();
    if !(2..=32).contains(&scheme)
        || !rest[0].is_ascii_alphabetic()
        || rest.get(scheme) != Some(&b':')
    {
        return None;
    }
    let body = rest[scheme..]
        .iter()
        .take_while(|b| {
            !matches!(b, b'<' | b'>') && !b.is_ascii_whitespace() && !b.is_ascii_control()
        })
        .count();
    (rest.get(scheme + body) == Some(&b'>')).then_some(at + 1 + scheme + body + 1)
}

/// Parses `(destination "title")` after the `(` at `from - 1`.
/// Returns the destination byte range, if any, and the index of the closing `)`.
fn destination(text: &str, from: usize) -> Option<(Option<(usize, usize)>, usize)> {
    let bytes = text.as_bytes();
    let mut i = skip_space(bytes, from);
    let target = match bytes.get(i)? {
        b')' => return Some((None, i)),
        b'<' => {
            let start = i;
            i += 1;
            loop {
                match bytes.get(i)? {
                    b'\\' if bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
                    b'>' => break,
                    b'<' | b'\n' => return None,
                    _ => i += 1,
                }
            }
            i += 1;
            (start, i)
        }
        _ => {
            let start = i;
            let mut depth = 0_usize;
            while let Some(&byte) = bytes.get(i) {
                match byte {
                    b'\\' if bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
                    b'(' if depth < MAX_PARENS => {
                        depth += 1;
                        i += 1;
                    }
                    b'(' => return None,
                    b')' if depth == 0 => break,
                    b')' => {
                        depth -= 1;
                        i += 1;
                    }
                    _ if byte.is_ascii_whitespace() || byte.is_ascii_control() => break,
                    _ => i += 1,
                }
            }
            if depth != 0 {
                return None;
            }
            (start, i)
        }
    };
    let gap = skip_space(bytes, i);
    i = gap;
    if gap > target.1 && matches!(bytes.get(i), Some(b'"' | b'\'' | b'(')) {
        let close = if bytes[i] == b'(' { b')' } else { bytes[i] };
        let limit = bytes.len().min(i + MAX_TITLE);
        i += 1;
        loop {
            if i >= limit {
                return None;
            }
            match bytes[i] {
                b'\\' => i += 2,
                b'(' if close == b')' => return None,
                byte if byte == close => break,
                _ => i += 1,
            }
        }
        i = skip_space(bytes, i + 1);
    }
    (bytes.get(i) == Some(&b')')).then_some((Some(target), i))
}

/// Skips spaces and tabs with at most one line ending.
fn skip_space(bytes: &[u8], mut i: usize) -> usize {
    let mut newline = false;
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b' ' | b'\t' => {}
            b'\n' if !newline => newline = true,
            _ => break,
        }
        i += 1;
    }
    i
}

/// Repository path, external URL or in-document anchor.
pub(crate) fn classify(name: &str) -> ReferenceKind {
    if name.starts_with('#') {
        return ReferenceKind::AnchorLink;
    }
    let scheme = name
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'))
        .count();
    let has_scheme = scheme >= 2
        && name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name[scheme..].starts_with(':');
    if has_scheme || name.starts_with("//") {
        ReferenceKind::ExternalLink
    } else {
        ReferenceKind::PathLink
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<(ReferenceKind, String)> {
        scan(text, 64)
            .into_iter()
            .map(|r| (r.kind, r.name))
            .collect()
    }

    #[test]
    fn code_spans_follow_backtick_run_rules() {
        use ReferenceKind::CodeSpan;
        assert_eq!(names("`a`"), [(CodeSpan, "a".into())]);
        assert_eq!(names("`` a ` b ``"), [(CodeSpan, "a ` b".into())]);
        assert_eq!(names("`a\n  b`"), [(CodeSpan, "a b".into())]);
        assert_eq!(names("\\`no`"), []);
        assert_eq!(names("``only one`"), []);
        assert_eq!(names("`` x `y`"), [(CodeSpan, "y".into())]);
        assert_eq!(names("` `"), []);
        assert_eq!(scan("a `b` c", 4)[0].start..scan("a `b` c", 4)[0].end, 2..5);
    }

    #[test]
    fn inline_links_yield_destinations() {
        use ReferenceKind::*;
        assert_eq!(names("[x](src/lib.rs)"), [(PathLink, "src/lib.rs".into())]);
        assert_eq!(
            names("![i](<a b.png> \"t\")"),
            [(PathLink, "a b.png".into())]
        );
        assert_eq!(names("[x](a(b)c 'title')"), [(PathLink, "a(b)c".into())]);
        assert_eq!(
            names("[`c`](https://x.y)"),
            [(CodeSpan, "c".into()), (ExternalLink, "https://x.y".into())]
        );
        assert_eq!(names("[x](#part)"), [(AnchorLink, "#part".into())]);
        assert_eq!(names("[x]()"), []);
        assert_eq!(names("[x] (a)"), []);
        assert_eq!(names("[x](a b)"), []);
        assert_eq!(names("[x][ref]"), []);
        assert!(names("`[x](a)`").iter().all(|(kind, _)| *kind == CodeSpan));
    }

    #[test]
    fn links_do_not_nest_and_autolinks_are_opaque() {
        use ReferenceKind::*;
        assert_eq!(names("[a [b](c) d](e)"), [(PathLink, "c".into())]);
        assert_eq!(
            names("![a [b](c)](d)"),
            [(PathLink, "c".into()), (PathLink, "d".into())]
        );
        assert_eq!(names("<https://x.y/`a`>"), []);
        assert_eq!(names("<b>`c`</b>"), [(CodeSpan, "c".into())]);
    }

    #[test]
    fn classification_uses_schemes_and_anchors() {
        assert_eq!(classify("docs/a.md#b"), ReferenceKind::PathLink);
        assert_eq!(classify("../x"), ReferenceKind::PathLink);
        assert_eq!(classify("mailto:a@b"), ReferenceKind::ExternalLink);
        assert_eq!(classify("//cdn.example"), ReferenceKind::ExternalLink);
        assert_eq!(classify("c:/x"), ReferenceKind::PathLink);
    }

    #[test]
    fn unbalanced_input_is_bounded() {
        assert_eq!(names(&"[".repeat(500)), []);
        assert_eq!(names(&"](\"".repeat(500)), []);
        assert_eq!(names(&"`".repeat(300)), []);
        assert_eq!(names("[x](a\\"), []);
    }
}
