// SPDX-License-Identifier: Apache-2.0
//! Iterative byte wildcard matching.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

// Most repository paths need fewer than 64 pending alternatives. Spill only
// unusually long/branchy patterns to the heap; never cap matching semantics.
struct Pending {
    inline: [(usize, usize); 64],
    spill: Vec<(usize, usize)>,
    len: usize,
}

impl Pending {
    fn push(&mut self, state: (usize, usize)) {
        if self.len < self.inline.len() {
            self.inline[self.len] = state;
        } else {
            self.spill.push(state);
        }
        self.len += 1;
    }

    fn pop(&mut self) -> Option<(usize, usize)> {
        self.len = self.len.checked_sub(1)?;
        if self.len < self.inline.len() {
            Some(self.inline[self.len])
        } else {
            self.spill.pop()
        }
    }
}

pub(super) fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut pending = Pending {
        inline: [(0, 0); 64],
        spill: Vec::new(),
        len: 0,
    };
    pending.push((0, 0));
    while let Some((mut p, mut t)) = pending.pop() {
        // Consume literals and single continuations directly. Only actual
        // wildcard alternatives need stack entries.
        loop {
            let pat = &pattern[p..];
            let txt = &text[t..];
            if pat.is_empty() {
                if txt.is_empty() {
                    return true;
                }
                break;
            }
            if pat.starts_with(b"**") {
                if pat.len() == 2 {
                    return true;
                }
                if pat[2] != b'/' {
                    break;
                }
                pending.push((p + 3, t));
                if txt.is_empty() {
                    break;
                }
                t += 1;
            } else if pat[0] == b'*' {
                // Preserve the legacy rule: '*' cannot start at a slash,
                // including its zero-byte alternative.
                if txt.first() == Some(&b'/') {
                    break;
                }
                if !txt.is_empty() {
                    pending.push((p, t + 1));
                }
                p += 1;
            } else if !txt.is_empty() && ((pat[0] == b'?' && txt[0] != b'/') || pat[0] == txt[0]) {
                p += 1;
                t += 1;
            } else {
                break;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    // Frozen pre-optimization matcher: protects legacy wildcard semantics,
    // including malformed double stars and the zero-byte-at-slash behavior.
    fn legacy_match(pattern: &[u8], text: &[u8]) -> bool {
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
                if pat.len() > 2 && pat[2] == b'/' {
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

    fn words(alphabet: &[u8], depth: usize) -> Vec<Vec<u8>> {
        let mut out = vec![Vec::new()];
        let mut start = 0;
        for _ in 0..depth {
            let end = out.len();
            for index in start..end {
                for byte in alphabet {
                    let mut word = out[index].clone();
                    word.push(*byte);
                    out.push(word);
                }
            }
            start = end;
        }
        out
    }

    #[test]
    fn exhaustive_short_globs_match_legacy() {
        let patterns = words(b"a/*?", 5);
        let texts = words(b"a/b", 4);
        for pattern in &patterns {
            for text in &texts {
                assert_eq!(
                    glob_match(pattern, text),
                    legacy_match(pattern, text),
                    "pattern={pattern:?} text={text:?}"
                );
            }
        }
    }

    #[test]
    fn spilled_alternatives_match_legacy() {
        let text = format!("{}file.rs", "nested/".repeat(100));
        for pattern in ["**/*.rs", "**/*.secret", "**/file.?s", "**/**/missing"] {
            assert_eq!(
                glob_match(pattern.as_bytes(), text.as_bytes()),
                legacy_match(pattern.as_bytes(), text.as_bytes())
            );
        }
    }
}
