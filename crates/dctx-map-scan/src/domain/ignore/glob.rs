// SPDX-License-Identifier: Apache-2.0
//! Iterative byte wildcard matching.
//! Implements SPEC-MAP-MODE section 6.1 and ADR-014/015b.
//! Uses standard-library types; consumed by scan clients.

pub(super) fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
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
