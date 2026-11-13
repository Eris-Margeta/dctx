// SPDX-License-Identifier: Apache-2.0
//! Iterative ignore wildcards: `*` and `?` do not cross `/`; `**/` consumes directories.
//! Implements the plain pattern language shared with ADR-015b rule lines.
//! Depends on two match rows, each proportional to the text length. [`super::rule`] calls it.

#[derive(Clone, Copy)]
enum Token {
    Literal(u8),
    Any,
    Star,
    Directories,
    Remainder,
}

pub(crate) fn glob_match(mut pattern: &[u8], text: &[u8]) -> bool {
    let mut tokens = Vec::new();
    while let Some((&head, rest)) = pattern.split_first() {
        if pattern == b"**" {
            tokens.push(Token::Remainder);
            break;
        }
        if let Some(rest) = pattern.strip_prefix(b"**/") {
            tokens.push(Token::Directories);
            pattern = rest;
        } else if pattern.starts_with(b"**") {
            return false;
        } else {
            tokens.push(match head {
                b'*' => Token::Star,
                b'?' => Token::Any,
                byte => Token::Literal(byte),
            });
            pattern = rest;
        }
    }
    // Each (pattern token, text offset) is evaluated once. Wildcard-heavy nonmatches
    // cannot branch exponentially or grow a stack of alternative matches.
    let mut next = vec![false; text.len() + 1];
    let mut row = next.clone();
    next[text.len()] = true;
    for token in tokens.into_iter().rev() {
        row[text.len()] = match token {
            Token::Literal(_) | Token::Any => false,
            Token::Star | Token::Directories => next[text.len()],
            Token::Remainder => true,
        };
        let mut directory_suffix = false;
        for at in (0..text.len()).rev() {
            row[at] = match token {
                Token::Literal(byte) => text[at] == byte && next[at + 1],
                Token::Any => text[at] != b'/' && next[at + 1],
                Token::Star => next[at] || (text[at] != b'/' && row[at + 1]),
                Token::Directories => {
                    if text[at] == b'/' {
                        directory_suffix |= next[at + 1];
                    }
                    next[at] || directory_suffix
                }
                Token::Remainder => true,
            };
        }
        std::mem::swap(&mut next, &mut row);
    }
    next[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_stops_at_a_slash() {
        assert!(glob_match(b"src/*.rs", b"src/main.rs"));
        assert!(!glob_match(b"src/*.rs", b"src/nested/main.rs"));
        assert!(glob_match(b"**/*.rs", b"src/nested/main.rs"));
    }
}
