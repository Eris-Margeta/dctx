// SPDX-License-Identifier: Apache-2.0
//! Template-argument depth, separate from comparisons and shifts.
//! Implements PARSERS-v1 nesting limits for C and C++ angle brackets.
//! Depends only on token spellings; the lexer and the declarator scan use it.
//! A `<` counts only when a closer matches it before `;`, `{`, or an outer `)`.

/// Parenthesis depth and the template `<` opened at each parenthesis depth.
#[derive(Debug, Clone)]
pub(crate) struct AngleDepth {
    paren: i32,
    /// Parenthesis depth at which each unmatched template `<` was opened.
    opens: Vec<i32>,
}

impl AngleDepth {
    pub(crate) fn new() -> Self {
        Self {
            paren: 0,
            opens: Vec::new(),
        }
    }

    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    pub(crate) fn depth(&self) -> u32 {
        self.opens.len() as u32
    }

    /// True when a comma or semicolon is a declarator separator.
    pub(crate) fn at_statement_level(&self) -> bool {
        self.paren == 0 && self.opens.is_empty()
    }

    /// Parentheses plus open template lists. Declarator names use one level of either.
    #[allow(clippy::cast_possible_truncation, reason = "Source offsets, line counts and token counts fit u32 because the public extractor enforces the 8 MiB input ceiling.")]
    #[allow(clippy::cast_possible_wrap, reason = "Template stack size is bounded by the 8 MiB input ceiling and therefore fits i32.")]
    pub(crate) fn nest(&self) -> i32 {
        self.paren + self.opens.len() as i32
    }

    /// Drop template lists that leaked across a statement. Not used inside `skip_angles`.
    pub(crate) fn boundary(&mut self) {
        if self.paren == 0 {
            self.opens.clear();
        }
    }

    /// `opens_template` is read only for `<`.
    pub(crate) fn observe(&mut self, spelling: &str, opens_template: bool) {
        match spelling {
            "(" | "[" | "{" => self.paren += 1,
            ")" | "]" | "}" => {
                if self.paren > 0 {
                    self.paren -= 1;
                    while self.opens.last().copied().is_some_and(|at| at > self.paren) {
                        self.opens.pop();
                    }
                }
            }
            "<" if opens_template => self.opens.push(self.paren),
            ">" => {
                if self.opens.last().copied() == Some(self.paren) {
                    self.opens.pop();
                }
            }
            // `>>` closes two template lists, or one when the rest belongs to an outer list.
            // A shift at depth zero pops nothing. A shift inside parentheses is not at this depth.
            ">>" => {
                let mut closed = 0;
                while closed < 2 && self.opens.last().copied() == Some(self.paren) {
                    self.opens.pop();
                    closed += 1;
                }
            }
            _ => {}
        }
    }
}

/// Whether `text(lt)` is a `<` that a template closer matches before a boundary.
///
/// `>>` closes two open lists, or one list when the other `>` belongs outside.
/// Nothing open means the `>>` is a shift. Parentheses hide angles inside them.
pub(crate) fn opens_template_args<'a>(
    text: impl Fn(usize) -> &'a str,
    len: usize,
    lt: usize,
) -> bool {
    if lt >= len || text(lt) != "<" {
        return false;
    }
    let mut angle = 1i32;
    let mut paren = 0i32;
    let mut index = lt + 1;
    while index < len {
        let spelling = text(index);
        if paren == 0 {
            match spelling {
                "<" => angle += 1,
                ">" => {
                    angle -= 1;
                    if angle == 0 {
                        return true;
                    }
                }
                ">>" if angle >= 2 => {
                    angle -= 2;
                    if angle == 0 {
                        return true;
                    }
                }
                // `Foo<Bar<int>>` is two `>` tokens fused by max-munch. One open list
                // still matches; the extra `>` belongs to the outer list.
                ">>" if angle == 1 => return true,
                "(" | "[" => paren += 1,
                "{" | ";" | ")" | "]" | "}" => return false,
                _ => {}
            }
        } else {
            match spelling {
                "(" | "[" | "{" => paren += 1,
                ")" | "]" | "}" => paren -= 1,
                _ => {}
            }
        }
        index += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(tokens: &'a [&'a str]) -> impl Fn(usize) -> &'a str {
        move |index| tokens.get(index).copied().unwrap_or("")
    }

    fn peak(tokens: &[&str], max_depth: u32) -> Result<u32, u32> {
        let mut angles = AngleDepth::new();
        let mut high = 0u32;
        for index in 0..tokens.len() {
            let opens = opens_template_args(text(tokens), tokens.len(), index);
            let before = angles.depth();
            if tokens[index] == ";" || tokens[index] == "{" {
                angles.boundary();
            }
            angles.observe(tokens[index], opens);
            high = high.max(angles.depth());
            if tokens[index] == "<" && angles.depth() > before && angles.depth() > max_depth {
                return Err(angles.depth());
            }
        }
        Ok(high)
    }

    #[test]
    fn comparisons_and_shifts_do_not_open_template_depth() {
        let compared = ["if", "(", "a", "<", "1", ")", "a", "++", ";"];
        assert!(!opens_template_args(text(&compared), compared.len(), 3));
        assert_eq!(peak(&compared, 1), Ok(0));
        let chained = ["a", "<", "b", "<", "c", "<", "d", ";"];
        assert_eq!(peak(&chained, 1), Ok(0));
        let shifted = ["a", ">>", "b", ",", "c", ";"];
        assert_eq!(peak(&shifted, 1), Ok(0));
        // `>>` after one `<` closes that list. Depth stays within one, so it is not TooDeep.
        let mixed = ["a", "<", "b", ">>", "c", ";"];
        assert!(opens_template_args(text(&mixed), mixed.len(), 1));
        assert_eq!(peak(&mixed, 1), Ok(1));
    }

    #[test]
    fn nested_templates_count_and_a_parenthesized_shift_does_not_close_them() {
        let deep = [
            "template", "<", "class", "A", "=", "B", "<", "C", "<", "D", "<", "int", ">>", ">>",
        ];
        assert!(opens_template_args(text(&deep), deep.len(), 1));
        assert_eq!(peak(&deep, 3), Err(4));
        assert_eq!(peak(&deep, 4), Ok(4));
        let shifted = [
            "template", "<", "int", "N", "=", "(", "1", ">>", "2", ")", ",", "class", "A", "=", "B",
            "<", "C", "<", "int", ">>", ">",
        ];
        assert_eq!(peak(&shifted, 2), Err(3));
        assert_eq!(peak(&shifted, 3), Ok(3));
    }
}
