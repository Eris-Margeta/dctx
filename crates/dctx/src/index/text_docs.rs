// SPDX-License-Identifier: Apache-2.0
//! Headed sections of plain-text documents: `.txt`, `.rst`, `.adoc`/`.asciidoc`, `.org` and `.tex`.
//! Every heading becomes a `d:` doc node exactly like a Markdown heading: kind `heading`, level from the marker,
//! section = the heading line up to the line before the next heading of the same or a higher level.
//! Deterministic line rules only; never reads source itself, never panics on malformed input.

use dctx_map_model::NodeKind;

use super::extract::Symbol;

/// Longest title kept, in characters.
const TITLE_MAX: usize = 200;

/// Longest summary kept, in characters.
const SUMMARY_MAX: usize = 160;

/// One physical line without its line break, and the byte offset where it starts.
struct Line<'a> {
    /// Line text without `\n` and `\r`.
    text: &'a str,
    /// Byte offset of the first byte of the line.
    start: usize,
}

/// One heading found by a format rule.
struct Head {
    /// Nesting level, 1 is the highest.
    level: u8,
    /// Whitespace-normalized title.
    title: String,
    /// Zero-based index of the heading's first line.
    line: usize,
    /// Number of lines the heading itself occupies (title plus underline).
    rows: usize,
}

/// True when `language` (a tag of `extract::language_of`) is a plain-text document format.
pub(super) fn handles(language: &str) -> bool {
    matches!(language, "text" | "rst" | "asciidoc" | "org" | "tex")
}

/// Heading symbols of `text` for a language that [`handles`]. Anything else yields none.
pub(super) fn symbols(language: &str, text: &str) -> Vec<Symbol> {
    let mut at = 0usize;
    let lines: Vec<Line<'_>> = text
        .split_inclusive('\n')
        .map(|raw| {
            let line = Line {
                text: raw.trim_end_matches(['\n', '\r']),
                start: at,
            };
            at += raw.len();
            line
        })
        .collect();
    let heads = match language {
        "text" => underlined(&lines, false),
        "rst" => underlined(&lines, true),
        "asciidoc" => asciidoc(&lines),
        "org" => org(&lines),
        "tex" => tex(&lines),
        _ => Vec::new(),
    };
    sections(text, &lines, &heads)
}

/// Turns headings into doc symbols whose span and lines cover the whole section.
fn sections(text: &str, lines: &[Line<'_>], heads: &[Head]) -> Vec<Symbol> {
    let mut out = Vec::with_capacity(heads.len());
    for (index, head) in heads.iter().enumerate() {
        let rest = heads.get(index + 1..).unwrap_or_default();
        let next = rest.first().map_or(lines.len(), |other| other.line);
        let end_line = rest
            .iter()
            .find(|other| other.level <= head.level)
            .map_or(lines.len(), |other| other.line);
        let start = lines.get(head.line).map_or(0, |line| line.start);
        let end = lines.get(end_line).map_or(text.len(), |line| line.start);
        let summary = lines
            .get(head.line + head.rows..next)
            .unwrap_or_default()
            .iter()
            .map(|line| line.text.trim())
            .find(|line| !line.is_empty() && !is_rule(line))
            .map_or_else(
                || head.title.clone(),
                |line| line.chars().take(SUMMARY_MAX).collect(),
            );
        let first = u32::try_from(head.line + 1).unwrap_or(u32::MAX);
        let last = u32::try_from(end_line.max(head.line + 1)).unwrap_or(u32::MAX);
        out.push(Symbol {
            name: head.title.clone(),
            kind: "heading".to_string(),
            graph: NodeKind::Doc,
            lines: [first, last],
            span: [
                u64::try_from(start).unwrap_or(u64::MAX),
                u64::try_from(end).unwrap_or(u64::MAX),
            ],
            summary,
            summary_source: "heading".to_string(),
            signature: String::new(),
        });
    }
    out
}

/// Title text with runs of whitespace folded to one space and a length cap.
fn norm(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(TITLE_MAX)
        .collect()
}

/// True for a line made only of one repeated adornment character, at least two long.
fn rule_char(line: &str) -> Option<char> {
    let line = line.trim_end();
    let mut chars = line.chars();
    let first = chars.next()?;
    (line.chars().count() >= 2
        && !first.is_alphanumeric()
        && !first.is_whitespace()
        && chars.all(|c| c == first))
    .then_some(first)
}

/// True when the whole line is an adornment rule.
fn is_rule(line: &str) -> bool {
    rule_char(line).is_some()
}

/// Plain-text and reStructuredText titles: a line followed by an adornment line at least as long.
/// Text also takes ALL-CAPS lines of 3..80 characters between blank lines (level 1).
fn underlined(lines: &[Line<'_>], rst: bool) -> Vec<Head> {
    let allowed: &[char] = if rst {
        &['=', '-', '~', '^']
    } else {
        &['=', '-']
    };
    let mut seen: Vec<char> = Vec::new();
    let mut heads = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        let title = lines[index].text;
        let under = lines.get(index + 1).and_then(|line| {
            rule_char(line.text).map(|mark| (mark, line.text.trim_end().chars().count()))
        });
        if let Some((mark, width)) = under.filter(|(mark, _)| allowed.contains(mark))
            && title_ok(title)
            && title.trim_end().chars().count() <= width
        {
            let level = if rst {
                if !seen.contains(&mark) {
                    seen.push(mark);
                }
                let place = seen.iter().position(|known| *known == mark).unwrap_or(0);
                u8::try_from(place + 1).unwrap_or(u8::MAX)
            } else {
                u8::from(mark != '=') + 1
            };
            heads.push(Head {
                level,
                title: norm(title),
                line: index,
                rows: 2,
            });
            index += 2;
            continue;
        }
        if !rst && caps_ok(lines, index) {
            heads.push(Head {
                level: 1,
                title: norm(title),
                line: index,
                rows: 1,
            });
        }
        index += 1;
    }
    heads
}

/// A title line: not indented, short, has a letter or digit and is not itself an adornment rule.
fn title_ok(line: &str) -> bool {
    let trimmed = line.trim_end();
    !trimmed.starts_with(char::is_whitespace)
        && trimmed.chars().any(char::is_alphanumeric)
        && trimmed.chars().count() <= TITLE_MAX
        && !is_rule(trimmed)
}

/// An ALL-CAPS line of 3..=80 characters with a blank line (or the file edge) on both sides.
fn caps_ok(lines: &[Line<'_>], index: usize) -> bool {
    let text = lines[index].text.trim_end();
    let count = text.chars().count();
    let blank = |line: Option<&Line<'_>>| line.is_none_or(|line| line.text.trim().is_empty());
    (3..=80).contains(&count)
        && !text.starts_with(char::is_whitespace)
        && text.chars().filter(|c| c.is_alphabetic()).count() >= 3
        && !text.chars().any(char::is_lowercase)
        && blank(index.checked_sub(1).and_then(|before| lines.get(before)))
        && blank(lines.get(index + 1))
}

/// AsciiDoc titles: `=` to `======` and a space. Verbatim blocks (`----`, `....`, `////`) are skipped.
fn asciidoc(lines: &[Line<'_>]) -> Vec<Head> {
    let mut heads = Vec::new();
    let mut fence: Option<&str> = None;
    for (index, line) in lines.iter().enumerate() {
        let text = line.text.trim_end();
        if let Some(open) = fence {
            if text == open {
                fence = None;
            }
            continue;
        }
        if text.len() >= 4 && rule_char(text).is_some_and(|mark| matches!(mark, '-' | '.' | '/')) {
            fence = Some(text);
            continue;
        }
        let marks = text.bytes().take_while(|byte| *byte == b'=').count();
        if (1..=6).contains(&marks) && text[marks..].starts_with(' ') {
            let title = norm(text[marks..].trim_end_matches('='));
            if !title.is_empty() {
                heads.push(Head {
                    level: u8::try_from(marks).unwrap_or(u8::MAX),
                    title,
                    line: index,
                    rows: 1,
                });
            }
        }
    }
    heads
}

/// Org titles: one or more `*` at the line start and a space. `#+begin` blocks are skipped.
fn org(lines: &[Line<'_>]) -> Vec<Head> {
    let mut heads = Vec::new();
    let mut inside = false;
    for (index, line) in lines.iter().enumerate() {
        let text = line.text.trim_end();
        if text.starts_with('#') || text.trim_start().starts_with("#+") {
            let lower = text.trim_start().to_ascii_lowercase();
            if lower.starts_with("#+begin") {
                inside = true;
            } else if lower.starts_with("#+end") {
                inside = false;
            }
            continue;
        }
        let stars = text.bytes().take_while(|byte| *byte == b'*').count();
        if !inside && stars > 0 && text[stars..].starts_with(' ') {
            let title = norm(&text[stars..]);
            if !title.is_empty() {
                heads.push(Head {
                    level: u8::try_from(stars).unwrap_or(u8::MAX),
                    title,
                    line: index,
                    rows: 1,
                });
            }
        }
    }
    heads
}

/// LaTeX sectioning commands and their levels.
const TEX_COMMANDS: &[(&str, u8)] = &[
    ("chapter", 1),
    ("section", 2),
    ("subsection", 3),
    ("subsubsection", 4),
];

/// LaTeX titles: `\chapter{..}`, `\section{..}`, `\subsection{..}`, `\subsubsection{..}`, also starred or with `[short]`.
fn tex(lines: &[Line<'_>]) -> Vec<Head> {
    let mut heads = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(rest) = line.text.trim_start().strip_prefix('\\') else {
            continue;
        };
        for (name, level) in TEX_COMMANDS {
            let Some(after) = rest.strip_prefix(name) else {
                continue;
            };
            if let Some(title) = tex_title(after)
                .map(|title| norm(&title.replace('~', " ")))
                .filter(|title| !title.is_empty())
            {
                heads.push(Head {
                    level: *level,
                    title,
                    line: index,
                    rows: 1,
                });
            }
            break;
        }
    }
    heads
}

/// The braced title after a command name: optional `*`, optional `[..]`, then a balanced `{..}` on the same line.
fn tex_title(after: &str) -> Option<&str> {
    let mut rest = after.strip_prefix('*').unwrap_or(after);
    if let Some(optional) = rest.strip_prefix('[') {
        rest = &optional[optional.find(']')? + 1..];
    }
    let body = rest.strip_prefix('{')?;
    let mut depth = 1usize;
    for (offset, byte) in body.bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return body.get(..offset);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{handles, symbols};

    /// Name, level-bearing line range and summary of every heading.
    fn shown(language: &str, text: &str) -> Vec<(String, [u32; 2], String)> {
        symbols(language, text)
            .into_iter()
            .map(|s| (s.name, s.lines, s.summary))
            .collect()
    }

    /// Text with underlines and ALL-CAPS lines gives nested sections whose span is the section text.
    #[test]
    fn text_underlines_and_caps_make_sections() {
        let text = "PROJECT NOTES\n=============\n\nIntro paragraph.\n\nDeploy Steps\n------------\n\nRun the thing.\n\nCLEANUP\n\nlast\n";
        let found = symbols("text", text);
        let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["PROJECT NOTES", "Deploy Steps", "CLEANUP"]);
        assert_eq!(found[0].lines, [1, 10]);
        assert_eq!(found[1].lines, [6, 10]);
        assert_eq!(found[2].lines, [11, 13]);
        assert_eq!(found[0].summary, "Intro paragraph.");
        let span = found[1].span;
        assert_eq!(
            &text[usize::try_from(span[0]).unwrap_or(0)..usize::try_from(span[1]).unwrap_or(0)],
            "Deploy Steps\n------------\n\nRun the thing.\n\n"
        );
        assert!(
            found
                .iter()
                .all(|s| s.kind == "heading" && s.summary_source == "heading")
        );
    }

    /// A short underline, an indented title and a lone rule are not headings.
    #[test]
    fn short_underlines_and_rules_are_not_headings() {
        assert!(shown("text", "A long title\n===\n").is_empty());
        assert!(shown("text", "  indented\n=========\n").is_empty());
        assert!(shown("text", "----\n\nwords here\n").is_empty());
        assert!(shown("text", "a\nCAPS LINE\nb\n").is_empty());
    }

    /// RST levels follow the order in which adornment characters first appear.
    #[test]
    fn rst_levels_follow_first_appearance() {
        let found = symbols("rst", "Top\n===\n\nSub\n---\n\nDeep\n~~~~\n\nTop2\n====\n");
        let levels: Vec<[u32; 2]> = found.iter().map(|s| s.lines).collect();
        assert_eq!(levels, [[1, 9], [4, 9], [7, 9], [10, 11]]);
    }

    /// AsciiDoc titles use `=` runs and ignore listing blocks; Org uses stars and ignores source blocks.
    #[test]
    fn asciidoc_and_org_markers() {
        let adoc = "= Manual Title\n\n== Installation\n\n----\n== not a title\n----\n\n=== Windows ===\nmore\n";
        assert_eq!(
            shown("asciidoc", adoc)
                .iter()
                .map(|t| t.0.as_str())
                .collect::<Vec<_>>(),
            ["Manual Title", "Installation", "Windows"]
        );
        let org = "* Inbox\n** Buy milk\n#+BEGIN_SRC sh\n* nope\n#+END_SRC\n* Projects\n";
        let found = symbols("org", org);
        assert_eq!(
            found.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["Inbox", "Buy milk", "Projects"]
        );
        assert_eq!(found[0].lines, [1, 5]);
        assert_eq!(found[1].lines, [2, 5]);
    }

    /// LaTeX commands take starred, optional-argument and nested-brace forms; comments and unbalanced titles are skipped.
    #[test]
    fn tex_commands() {
        let tex = "\\section{Introduction}\n%\\section{Hidden}\n\\subsection*{Related~Work}\n\\section[short]{A {big} idea}\n\\section{Open\n";
        let found = shown("tex", tex);
        assert_eq!(
            found.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
            ["Introduction", "Related Work", "A {big} idea"]
        );
        assert_eq!(found[0].1, [1, 3]);
        assert!(handles("tex") && handles("text") && !handles("markdown"));
    }
}
