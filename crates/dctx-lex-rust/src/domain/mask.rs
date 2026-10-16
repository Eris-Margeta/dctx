//! Masks comments, strings and char literals so the line scan sees only code.
//! Implements the PARSERS-v1 Rust row rule that literals and comments never declare.
//! Pure and iterative; used by the line scan, which also reads `///` doc lines from it.

/// Source with non-code characters blanked; same byte length and line breaks.
pub(crate) struct Masked {
    pub code: String,
    /// Byte offsets where an outer line doc comment (`///`, not `////`) starts.
    pub doc_starts: Vec<usize>,
}

enum State {
    Code,
    LineComment,
    BlockComment(usize),
    Str,
    RawStr(usize),
}

pub(crate) fn mask(source: &str) -> Masked {
    let bytes = source.as_bytes();
    let mut code = String::with_capacity(source.len());
    let mut doc_starts = Vec::new();
    let mut state = State::Code;
    let mut chars = source.char_indices().peekable();
    let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
    while let Some((i, c)) = chars.next() {
        let blank = |code: &mut String, c: char| {
            if c == '\n' {
                code.push('\n');
            } else {
                code.extend(std::iter::repeat_n(' ', c.len_utf8()));
            }
        };
        match state {
            State::Code => match c {
                '/' if at(i + 1) == b'/' => {
                    if at(i + 2) == b'/' && at(i + 3) != b'/' {
                        doc_starts.push(i);
                    }
                    state = State::LineComment;
                    blank(&mut code, c);
                }
                '/' if at(i + 1) == b'*' => {
                    state = State::BlockComment(1);
                    blank(&mut code, c);
                    chars.next();
                    code.push(' ');
                }
                '"' => {
                    state = raw_hashes(bytes, i).map_or(State::Str, State::RawStr);
                    code.push('"');
                }
                '\'' => {
                    code.push('\'');
                    if let Some(len) = char_literal(&source[i + 1..]) {
                        for _ in 0..len {
                            if let Some((_, inner)) = chars.next() {
                                blank(&mut code, inner);
                            }
                        }
                    }
                }
                _ => code.push(c),
            },
            State::LineComment => {
                if c == '\n' {
                    state = State::Code;
                }
                blank(&mut code, c);
            }
            State::BlockComment(depth) => {
                blank(&mut code, c);
                if c == '*' && at(i + 1) == b'/' {
                    chars.next();
                    code.push(' ');
                    state = if depth == 1 {
                        State::Code
                    } else {
                        State::BlockComment(depth - 1)
                    };
                } else if c == '/' && at(i + 1) == b'*' {
                    chars.next();
                    code.push(' ');
                    state = State::BlockComment(depth + 1);
                }
            }
            State::Str => match c {
                '\\' => {
                    blank(&mut code, c);
                    if let Some((_, escaped)) = chars.next() {
                        blank(&mut code, escaped);
                    }
                }
                '"' => {
                    code.push('"');
                    state = State::Code;
                }
                _ => blank(&mut code, c),
            },
            State::RawStr(hashes) => {
                let closes = c == '"' && (1..=hashes).all(|k| at(i + k) == b'#');
                if closes {
                    code.push('"');
                    state = State::Code;
                } else {
                    blank(&mut code, c);
                }
            }
        }
    }
    Masked { code, doc_starts }
}

/// `r"`, `r#"`, `br##"` ...: the number of hashes when the quote at `quote` opens a raw string.
fn raw_hashes(bytes: &[u8], quote: usize) -> Option<usize> {
    let hashes = bytes[..quote].iter().rev().take_while(|&&b| b == b'#').count();
    let r = quote.checked_sub(hashes + 1)?;
    if bytes[r] != b'r' {
        return None;
    }
    let prefix_start = match r.checked_sub(1).map(|p| bytes[p]) {
        Some(b'b' | b'c') => r - 1,
        _ => r,
    };
    let starts_token = prefix_start == 0 || !is_ident_byte(bytes[prefix_start - 1]);
    starts_token.then_some(hashes)
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Characters after an opening `'` that belong to a char literal; `None` for a lifetime.
fn char_literal(rest: &str) -> Option<usize> {
    let mut chars = rest.chars();
    match chars.next()? {
        '\\' => {
            rest.char_indices()
                .skip(2)
                .take(12)
                .find(|&(_, c)| c == '\'')
                .map(|(end, _)| rest[..=end].chars().count())
        }
        _ => (chars.next()? == '\'').then_some(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_comments_are_blanked_with_the_same_length() {
        let source = "fn a() { \"fn x\" } // fn y\n/* fn z /* n */ */ fn b() {}";
        let masked = mask(source);
        assert_eq!(masked.code.len(), source.len());
        assert!(!masked.code.contains("fn x"));
        assert!(!masked.code.contains("fn y"));
        assert!(!masked.code.contains("fn z"));
        assert!(masked.code.contains("fn b() {}"));
    }

    #[test]
    fn raw_strings_chars_and_lifetimes_are_told_apart() {
        let source = "let s = r#\"fn x\"#; let c = '{'; let e = '\\''; fn f<'a>(x: &'a str) {}";
        let masked = mask(source);
        assert!(!masked.code.contains("fn x"));
        assert_eq!(masked.code.matches('{').count(), 1);
        assert!(masked.code.contains("fn f<'a>(x: &'a str)"));
        assert_eq!(masked.code.len(), source.len());
    }

    #[test]
    fn doc_comment_starts_are_recorded() {
        let masked = mask("/// Doc.\n//// Not doc.\n// Plain.\nfn a() {}");
        assert_eq!(masked.doc_starts, [0]);
    }

    #[test]
    fn unicode_inside_literals_keeps_byte_offsets() {
        let source = "const S: &str = \"é—\"; fn ok() {}";
        let masked = mask(source);
        assert_eq!(masked.code.len(), source.len());
        assert_eq!(masked.code.find("fn ok"), source.find("fn ok"));
    }
}
