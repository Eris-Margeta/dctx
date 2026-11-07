// SPDX-License-Identifier: Apache-2.0
//! Iterative glob matcher for ignore and omit patterns (`**`, `*`, `?`, literals).
//! Implements the pattern syntax of the pack selection rules (ARCHITECTURE-PROPOSAL §2, DTS SELECT).
//! Depends on nothing; pure byte matching with no IO.
//! Used by [`super::rule`] for pure ignore and omit rule matching.

/// Matches `text` against a glob `pattern`.
///
/// `**` matches zero or more path segments (including `/`), `*` matches within one
/// segment, `?` matches one non-`/` byte and every other byte matches itself.
/// The search uses an explicit stack (pre-sized to 64) instead of recursion.
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut stack: Vec<(&[u8], &[u8])> = Vec::with_capacity(64);
    stack.push((pattern, text));

    while let Some((pat, txt)) = stack.pop() {
        if pat.is_empty() {
            if txt.is_empty() {
                return true;
            }
            continue;
        }

        if pat.starts_with(b"**") {
            if pat.len() == 2 {
                return true;
            }
            if pat[2] == b'/' {
                let rest = &pat[3..];
                stack.push((rest, txt));
                if !txt.is_empty() {
                    stack.push((pat, &txt[1..]));
                }
            }
        } else if pat[0] == b'*' {
            let rest = &pat[1..];
            for i in 0..=txt.len() {
                if i < txt.len() && txt[i] == b'/' {
                    break;
                }
                stack.push((rest, &txt[i..]));
            }
        } else if pat[0] == b'?' {
            if !txt.is_empty() && txt[0] != b'/' {
                stack.push((&pat[1..], &txt[1..]));
            }
        } else if !txt.is_empty() && pat[0] == txt[0] {
            stack.push((&pat[1..], &txt[1..]));
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match(b"hello", b"hello"));
        assert!(!glob_match(b"hello", b"world"));
    }

    #[test]
    fn test_glob_match_single_star() {
        assert!(glob_match(b"*.txt", b"file.txt"));
        assert!(glob_match(b"*.txt", b".txt"));
        assert!(!glob_match(b"*.txt", b"file.rs"));
        assert!(!glob_match(b"*.txt", b"dir/file.txt")); // * doesn't match /
    }

    #[test]
    fn test_glob_match_prefix_star() {
        assert!(glob_match(b"test*", b"test"));
        assert!(glob_match(b"test*", b"test.rs"));
        assert!(glob_match(b"test*", b"testing"));
        assert!(!glob_match(b"test*", b"mytest"));
    }

    #[test]
    fn test_glob_match_question_mark() {
        assert!(glob_match(b"test?", b"tests"));
        assert!(glob_match(b"test?", b"test1"));
        assert!(!glob_match(b"test?", b"test"));
        assert!(!glob_match(b"test?", b"test12"));
        assert!(!glob_match(b"test?", b"test/")); // ? doesn't match /
    }

    #[test]
    fn test_glob_match_double_star() {
        assert!(glob_match(b"**", b"anything"));
        assert!(glob_match(b"**", b"a/b/c"));
        assert!(glob_match(b"**/file.txt", b"file.txt"));
        assert!(glob_match(b"**/file.txt", b"dir/file.txt"));
        assert!(glob_match(b"**/file.txt", b"a/b/c/file.txt"));
    }

    #[test]
    fn test_glob_match_complex() {
        assert!(glob_match(b"src/*.rs", b"src/main.rs"));
        assert!(!glob_match(b"src/*.rs", b"src/sub/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/sub/main.rs"));
    }

    #[test]
    fn test_glob_match_empty() {
        assert!(glob_match(b"", b""));
        assert!(!glob_match(b"", b"something"));
        assert!(glob_match(b"*", b""));
    }

    #[test]
    fn test_glob_iterative_double_star_zero_segments() {
        assert!(glob_match(b"**/test", b"test"));
        assert!(glob_match(b"**/test.rs", b"test.rs"));
    }

    #[test]
    fn test_glob_iterative_double_star_multiple_segments() {
        assert!(glob_match(b"**/test.rs", b"a/test.rs"));
        assert!(glob_match(b"**/test.rs", b"a/b/test.rs"));
        assert!(glob_match(b"**/test.rs", b"a/b/c/d/e/test.rs"));
    }

    #[test]
    fn test_glob_iterative_star_within_segment() {
        assert!(glob_match(b"test*.rs", b"test.rs"));
        assert!(glob_match(b"test*.rs", b"test_main.rs"));
        assert!(glob_match(b"test*.rs", b"testing.rs"));
        assert!(!glob_match(b"test*.rs", b"dir/test.rs")); // * stops at /
    }

    #[test]
    fn test_glob_iterative_question_mark() {
        assert!(glob_match(b"test?.rs", b"test1.rs"));
        assert!(glob_match(b"test?.rs", b"testa.rs"));
        assert!(!glob_match(b"test?.rs", b"test.rs")); // ? requires one char
        assert!(!glob_match(b"test?.rs", b"test12.rs")); // ? is exactly one char
        assert!(!glob_match(b"test?rs", b"test/rs")); // ? doesn't match /
    }

    #[test]
    fn test_glob_iterative_complex_patterns() {
        assert!(glob_match(b"src/**/*.rs", b"src/main.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/lib/util.rs"));
        assert!(glob_match(b"src/**/*.rs", b"src/a/b/c/test.rs"));
        assert!(!glob_match(b"src/**/*.rs", b"tests/main.rs"));
        assert!(!glob_match(b"src/**/*.rs", b"src/main.txt"));
    }

    #[test]
    fn test_glob_iterative_empty_pattern() {
        assert!(glob_match(b"", b""));
        assert!(!glob_match(b"", b"text"));
        assert!(!glob_match(b"pattern", b""));
    }

    #[test]
    fn test_glob_iterative_pattern_longer_than_text() {
        assert!(!glob_match(b"very_long_pattern", b"short"));
        assert!(!glob_match(b"test.rs.backup", b"test.rs"));
    }

    #[test]
    fn test_glob_iterative_multiple_stars() {
        assert!(glob_match(b"*test*", b"test"));
        assert!(glob_match(b"*test*", b"mytest"));
        assert!(glob_match(b"*test*", b"testing"));
        assert!(glob_match(b"*test*", b"mytesting"));
        assert!(!glob_match(b"*test*", b"my/testing")); // * stops at /
    }

    #[test]
    fn test_glob_iterative_double_star_at_end() {
        assert!(glob_match(b"src/**", b"src/"));
        assert!(glob_match(b"src/**", b"src/file"));
        assert!(glob_match(b"src/**", b"src/a/b/c"));
    }

    #[test]
    fn test_glob_iterative_literal_with_special_chars() {
        assert!(glob_match(b"test-file.rs", b"test-file.rs"));
        assert!(glob_match(b"test_file.rs", b"test_file.rs"));
        assert!(glob_match(b"test.backup.rs", b"test.backup.rs"));
    }
}
