//! Single path-component gitignore globs and a sound "can these two globs match a common name" test.
//! Implements the supported-ignore-semantics subset of plugin-api git.md (G-COVERAGE): `*`, `?`, classes
//! and backslash escapes; unsupported forms are reported, never assumed harmless. Pure; no IO.
//! Used by domain::coverage to decide whether a negation can re-include a dctx export.
//! The intersection over-approximates (ASCII case-folded, negated classes overlap) so it never misses one.

/// A glob that this profile cannot evaluate (for example an unterminated class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unsupported;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Literal(char),
    Any,
    Star,
    Class { negated: bool, ranges: Vec<(char, char)> },
}

/// A tokenized glob for one path component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Glob {
    tokens: Vec<Token>,
}

fn fold(c: char) -> char {
    c.to_ascii_lowercase()
}

fn class_has(negated: bool, ranges: &[(char, char)], c: char) -> bool {
    let inside = |c: &char| ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(c));
    let variants = [c, c.to_ascii_lowercase(), c.to_ascii_uppercase()];
    if negated { !variants.iter().all(inside) } else { variants.iter().any(inside) }
}

fn compatible(a: &Token, b: &Token) -> bool {
    match (a, b) {
        (Token::Literal(x), Token::Literal(y)) => fold(*x) == fold(*y),
        (Token::Literal(x), Token::Class { negated, ranges }) | (Token::Class { negated, ranges }, Token::Literal(x)) => {
            class_has(*negated, ranges, *x)
        }
        (Token::Class { negated: false, ranges: left }, Token::Class { negated: false, ranges: right }) => {
            left.iter().any(|(a_lo, a_hi)| right.iter().any(|(b_lo, b_hi)| a_lo <= b_hi && b_lo <= a_hi))
                || left.iter().chain(right).any(|(lo, hi)| lo.is_ascii_alphabetic() || hi.is_ascii_alphabetic())
        }
        _ => true,
    }
}

fn parse_class(chars: &[char], start: usize) -> Result<(Token, usize), Unsupported> {
    let mut index = start;
    let negated = matches!(chars.get(index), Some('!' | '^'));
    if negated {
        index += 1;
    }
    let mut ranges = Vec::new();
    let mut first = true;
    loop {
        let mut c = *chars.get(index).ok_or(Unsupported)?;
        if c == ']' && !first {
            return Ok((Token::Class { negated, ranges }, index + 1));
        }
        first = false;
        if c == '\\' {
            index += 1;
            c = *chars.get(index).ok_or(Unsupported)?;
        }
        index += 1;
        if chars.get(index) == Some(&'-') && chars.get(index + 1).is_some_and(|next| *next != ']') {
            let mut hi = chars[index + 1];
            index += 2;
            if hi == '\\' {
                hi = *chars.get(index).ok_or(Unsupported)?;
                index += 1;
            }
            if hi < c {
                return Err(Unsupported);
            }
            ranges.push((c, hi));
        } else {
            ranges.push((c, c));
        }
    }
}

impl Glob {
    /// Tokenizes one component; a `/` inside is unsupported here.
    pub fn parse(component: &str) -> Result<Self, Unsupported> {
        let chars: Vec<char> = component.chars().collect();
        let mut tokens = Vec::new();
        let mut index = 0;
        while let Some(&c) = chars.get(index) {
            index += 1;
            let token = match c {
                '/' => return Err(Unsupported),
                '\\' => {
                    let escaped = *chars.get(index).ok_or(Unsupported)?;
                    index += 1;
                    Token::Literal(escaped)
                }
                '*' => {
                    if tokens.last() == Some(&Token::Star) {
                        continue;
                    }
                    Token::Star
                }
                '?' => Token::Any,
                '[' => {
                    let (class, next) = parse_class(&chars, index)?;
                    index = next;
                    class
                }
                other => Token::Literal(other),
            };
            tokens.push(token);
        }
        Ok(Self { tokens })
    }

    /// The literal glob for an exact name (every character escaped).
    pub fn literal(name: &str) -> Self {
        Self { tokens: name.chars().map(Token::Literal).collect() }
    }

    /// Whether some single name could match both globs (sound over-approximation).
    pub fn intersects(&self, other: &Self) -> bool {
        let (a, b) = (&self.tokens, &other.tokens);
        let width = b.len() + 1;
        let mut seen = vec![false; (a.len() + 1) * width];
        let mut stack = vec![(0usize, 0usize)];
        while let Some((i, j)) = stack.pop() {
            if seen[i * width + j] {
                continue;
            }
            seen[i * width + j] = true;
            if i == a.len() && j == b.len() {
                return true;
            }
            let (left, right) = (a.get(i), b.get(j));
            if left == Some(&Token::Star) {
                stack.push((i + 1, j));
            }
            if right == Some(&Token::Star) {
                stack.push((i, j + 1));
            }
            match (left, right) {
                (Some(Token::Star), Some(Token::Star)) | (None, _) | (_, None) => {}
                (Some(Token::Star), Some(_)) => stack.push((i, j + 1)),
                (Some(_), Some(Token::Star)) => stack.push((i + 1, j)),
                (Some(x), Some(y)) if compatible(x, y) => stack.push((i + 1, j + 1)),
                (Some(_), Some(_)) => {}
            }
        }
        false
    }

    /// Whether the glob matches `name` (exact, case-sensitive Git semantics).
    pub fn matches(&self, name: &str) -> bool {
        let chars: Vec<char> = name.chars().collect();
        let width = chars.len() + 1;
        let mut seen = vec![false; (self.tokens.len() + 1) * width];
        let mut stack = vec![(0usize, 0usize)];
        while let Some((i, j)) = stack.pop() {
            if std::mem::replace(&mut seen[i * width + j], true) {
                continue;
            }
            match (self.tokens.get(i), chars.get(j)) {
                (None, None) => return true,
                (Some(Token::Star), next) => {
                    stack.push((i + 1, j));
                    if next.is_some() {
                        stack.push((i, j + 1));
                    }
                }
                (Some(Token::Any), Some(_)) => stack.push((i + 1, j + 1)),
                (Some(Token::Literal(x)), Some(c)) if x == c => stack.push((i + 1, j + 1)),
                (Some(Token::Class { negated, ranges }), Some(c))
                    if ranges.iter().any(|(lo, hi)| (*lo..=*hi).contains(c)) != *negated =>
                {
                    stack.push((i + 1, j + 1));
                }
                _ => {}
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glob(text: &str) -> Glob {
        Glob::parse(text).expect("supported")
    }

    #[test]
    fn matching_follows_git_component_rules() {
        assert!(glob("*.llmcontext.txt").matches("proj.llmcontext.txt"));
        assert!(glob("*.llmcontext.txt").matches(".llmcontext.txt"));
        assert!(!glob("*.llmcontext.txt").matches("proj.llmcontext-1-diff.txt"));
        assert!(glob("*.llmcontext-*-diff.txt").matches("p.llmcontext-v1.2-diff.txt"));
        assert!(glob("[a-c]?\\*").matches("bx*"));
        assert!(!glob("[!a-c]x").matches("ax"));
        assert!(glob("[]]").matches("]"));
    }

    #[test]
    fn intersection_finds_common_names_and_refuses_disjoint_ones() {
        let text = glob("*.llmcontext.txt");
        assert!(glob("*.txt").intersects(&text));
        assert!(glob("important.llmcontext.txt").intersects(&text));
        assert!(glob("IMPORTANT.LLMCONTEXT.TXT").intersects(&text));
        assert!(glob("*").intersects(&text));
        assert!(glob("?.llmcontext.txt").intersects(&text));
        assert!(glob("[!x].llmcontext.txt").intersects(&text));
        assert!(!glob("*.rs").intersects(&text));
        assert!(!glob("notes").intersects(&text));
        assert!(!glob("*.llmcontext.md").intersects(&text));
        assert!(Glob::literal(".dctx").intersects(&glob(".dctx")));
        assert!(!Glob::literal(".dctx").intersects(&glob(".dctxx")));
    }

    #[test]
    fn unsupported_forms_are_reported() {
        assert_eq!(Glob::parse("[abc"), Err(Unsupported));
        assert_eq!(Glob::parse("a\\"), Err(Unsupported));
        assert_eq!(Glob::parse("a/b"), Err(Unsupported));
        assert_eq!(Glob::parse("[z-a]"), Err(Unsupported));
    }
}
