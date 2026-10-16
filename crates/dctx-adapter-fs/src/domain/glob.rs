//! Iterative ignore wildcards that do not cross `/`.
//! Implements the plain pattern language shared with ADR-015b rule lines.
//! Depends on no allocator beyond the match stack. [`super::rule`] calls it for glob rules.

pub(crate) fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut stack: Vec<(&[u8], &[u8])> = Vec::with_capacity(8);
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
            for index in 0..=txt.len() {
                if index < txt.len() && txt[index] == b'/' {
                    break;
                }
                stack.push((rest, &txt[index..]));
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
    fn star_stops_at_a_slash() {
        assert!(glob_match(b"src/*.rs", b"src/main.rs"));
        assert!(!glob_match(b"src/*.rs", b"src/nested/main.rs"));
        assert!(glob_match(b"**/*.rs", b"src/nested/main.rs"));
    }
}
