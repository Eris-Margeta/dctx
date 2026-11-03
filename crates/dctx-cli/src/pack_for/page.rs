// SPDX-License-Identifier: Apache-2.0
//! The budgeted page of `dctx pack --for`: whole lines only, sections in the order they are added, and an end line.
//! A line that would push the page over the budget is dropped and counted; a later, shorter line may still fit.
//! The estimate is the `dctx map --budget` one, ceil(2 * UTF-8 bytes / 7); it is not a tokenizer guarantee.
//! Pure: no index, no IO. Used by the parent module.

/// Bytes kept free for the end line `# end body_tokens=<n> budget=<n> dropped=<n>` (at most 85 bytes with its newline).
const END_RESERVE: usize = 96;

/// Estimated tokens of `bytes` UTF-8 bytes: ceil(2 * bytes / 7), the estimate of `dctx map --budget`.
pub(super) fn estimate_tokens(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX).saturating_mul(2).div_ceil(7)
}

/// A page under construction. Every line it holds plus [`END_RESERVE`] stays inside `budget` estimated tokens.
pub(super) struct Page {
    budget: u32,
    text: String,
    dropped: usize,
}

impl Page {
    /// A page that starts with `header`. The caller's minimum budget leaves room for a short header.
    pub(super) fn new(budget: u32, header: &str) -> Self {
        let mut page = Self { budget, text: String::new(), dropped: 0 };
        if !page.push(header) {
            page.dropped += 1;
        }
        page
    }

    /// Appends `title`, then each of `lines` that still fits. The title goes in only together with the first line that fits,
    /// so a section is never a bare title. Returns the number of lines kept.
    pub(super) fn section(&mut self, title: &str, lines: &[&str]) -> usize {
        let mut titled = false;
        let mut kept = 0;
        for line in lines {
            if !titled {
                if !self.fits(title.len() + 1 + line.len() + 1) {
                    self.dropped += 1;
                    continue;
                }
                self.append(title);
                titled = true;
            }
            if self.push(line) {
                kept += 1;
            } else {
                self.dropped += 1;
            }
        }
        kept
    }

    /// The finished page: its lines and the end line, all inside the budget.
    pub(super) fn finish(mut self) -> String {
        let body_tokens = estimate_tokens(self.text.len());
        let end = format!("# end body_tokens={body_tokens} budget={} dropped={}", self.budget, self.dropped);
        debug_assert!(end.len() < END_RESERVE);
        self.append(&end);
        self.text
    }

    /// True when `extra` more bytes and the end-line reserve stay inside the budget.
    fn fits(&self, extra: usize) -> bool {
        estimate_tokens(self.text.len() + extra + END_RESERVE) <= u64::from(self.budget)
    }

    /// Appends `line` when it fits; false when it does not.
    fn push(&mut self, line: &str) -> bool {
        if !self.fits(line.len() + 1) {
            return false;
        }
        self.append(line);
        true
    }

    fn append(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_estimate_is_the_map_budget_one() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(7), 2);
        assert_eq!(estimate_tokens(8), 3);
        assert_eq!(estimate_tokens(10_500), 3000);
        assert_eq!(estimate_tokens(10_501), 3001);
    }

    #[test]
    fn a_page_never_exceeds_its_budget_and_keeps_whole_lines() {
        for budget in [100_u32, 101, 300, 1000, 3000] {
            let mut page = Page::new(budget, "# dctx pack --for budget=x terms=3");
            let long = "y".repeat(500);
            let lines: Vec<String> = (0..400).map(|n| if n % 7 == 0 { long.clone() } else { format!("s:a.rs#f{n}\tfn\tL{n}") }).collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            page.section("## find f", &refs);
            let text = page.finish();
            assert!(estimate_tokens(text.len()) <= u64::from(budget), "budget {budget}: {} tokens", estimate_tokens(text.len()));
            assert!(text.ends_with('\n'));
            for line in text.lines().filter(|line| line.starts_with("s:")) {
                assert!(lines.iter().any(|whole| whole == line), "cut line {line:?}");
            }
            assert!(text.lines().last().is_some_and(|end| end.starts_with("# end body_tokens=")), "{text}");
        }
    }

    #[test]
    fn sections_keep_their_order_and_a_full_page_counts_what_it_drops() {
        let mut page = Page::new(100, "# h");
        assert_eq!(page.section("## first", &["one", "two"]), 2);
        let big = "z".repeat(400);
        assert_eq!(page.section("## big", &[big.as_str()]), 0);
        assert_eq!(page.section("## last", &["three"]), 1);
        let text = page.finish();
        assert_eq!(text, "# h\n## first\none\ntwo\n## last\nthree\n# end body_tokens=10 budget=100 dropped=1\n");
    }

    #[test]
    fn an_empty_section_adds_nothing() {
        let mut page = Page::new(100, "# h");
        assert_eq!(page.section("## none", &[]), 0);
        assert_eq!(page.finish(), "# h\n# end body_tokens=2 budget=100 dropped=0\n");
    }
}
