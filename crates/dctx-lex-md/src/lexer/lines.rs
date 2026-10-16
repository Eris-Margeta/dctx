//! Source lines with byte offsets, column-aware indentation, and front matter.
//! Implements CommonMark line and tab rules (tab stops of four) as far as headings need.
//! Pure string scanning; used by the block scanner.

/// One source line without its `\n` or `\r\n` terminator.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Line<'a> {
    pub text: &'a str,
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset after the terminator.
    pub end: usize,
    /// One-based line number.
    pub number: u32,
}

/// Lines of `source` from byte `pos`, numbered from `number`.
pub(crate) struct Lines<'a> {
    source: &'a str,
    pos: usize,
    number: u32,
}

impl<'a> Lines<'a> {
    pub fn new(source: &'a str, pos: usize, number: u32) -> Self {
        Self {
            source,
            pos,
            number,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    type Item = Line<'a>;

    fn next(&mut self) -> Option<Line<'a>> {
        let rest = self
            .source
            .get(self.pos..)
            .filter(|rest| !rest.is_empty())?;
        let (raw, len) = match rest.find('\n') {
            Some(index) => (&rest[..index], index + 1),
            None => (rest, rest.len()),
        };
        let line = Line {
            text: raw.strip_suffix('\r').unwrap_or(raw),
            start: self.pos,
            end: self.pos + len,
            number: self.number,
        };
        self.pos += len;
        self.number = self.number.saturating_add(1);
        Some(line)
    }
}

/// Position inside a line: byte offset and visual column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Cursor {
    pub pos: usize,
    pub col: usize,
}

impl Cursor {
    /// Text from the cursor to the end of the line.
    pub fn rest<'a>(&self, text: &'a str) -> &'a str {
        text.get(self.pos..).unwrap_or_default()
    }

    /// Indentation width in columns and the cursor after it.
    pub fn indent(&self, text: &str) -> (usize, Cursor) {
        let mut next = *self;
        for byte in self.rest(text).bytes() {
            match byte {
                b' ' => next.col += 1,
                b'\t' => next.col += 4 - next.col % 4,
                _ => break,
            }
            next.pos += 1;
        }
        (next.col - self.col, next)
    }

    /// Consumes indentation until column `target`; a tab may overshoot it.
    pub fn advance_to(&self, text: &str, target: usize) -> Cursor {
        let mut next = *self;
        for byte in self.rest(text).bytes() {
            if next.col >= target {
                break;
            }
            match byte {
                b' ' => next.col += 1,
                b'\t' => next.col += 4 - next.col % 4,
                _ => break,
            }
            next.pos += 1;
        }
        next
    }

    /// Moves past `bytes` ASCII bytes that occupy one column each.
    pub fn skip(&self, bytes: usize) -> Cursor {
        Cursor {
            pos: self.pos + bytes,
            col: self.col + bytes,
        }
    }
}

/// True when the text holds only spaces and tabs.
pub(crate) fn is_blank(text: &str) -> bool {
    text.bytes().all(|byte| byte == b' ' || byte == b'\t')
}

/// Byte range of `---`/`+++` front matter that opens the file and is closed.
pub(crate) fn front_matter(source: &str) -> Option<(usize, usize)> {
    let start = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let mut lines = Lines::new(source, start, 1);
    let first = lines.next()?;
    let open = first.text.trim_end();
    if open != "---" && open != "+++" {
        return None;
    }
    lines
        .find(|line| {
            let close = line.text.trim_end();
            close == open || (open == "---" && close == "...")
        })
        .map(|line| (start, line.end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_offsets_and_strip_terminators() {
        let lines: Vec<_> = Lines::new("a\r\nbc\n\nd", 0, 1)
            .map(|l| (l.text, l.start, l.end, l.number))
            .collect();
        assert_eq!(
            lines,
            [
                ("a", 0, 3, 1),
                ("bc", 3, 6, 2),
                ("", 6, 7, 3),
                ("d", 7, 8, 4)
            ]
        );
        assert_eq!(Lines::new("x\n", 0, 1).count(), 1);
        assert_eq!(Lines::new("", 0, 1).count(), 0);
    }

    #[test]
    fn tabs_advance_to_the_next_stop() {
        let (width, cursor) = Cursor::default().indent(" \tx");
        assert_eq!((width, cursor.pos, cursor.col), (4, 2, 4));
        let at = Cursor { pos: 1, col: 2 }.advance_to("> \t\tx", 4);
        assert_eq!((at.pos, at.col), (3, 4));
        assert!(is_blank(" \t") && !is_blank(" x"));
    }

    #[test]
    fn front_matter_must_open_the_file_and_close() {
        assert_eq!(front_matter("---\na: 1\n---\n# T"), Some((0, 13)));
        assert_eq!(front_matter("+++\na = 1\n+++"), Some((0, 13)));
        assert_eq!(front_matter("---\na: 1\n...\n"), Some((0, 13)));
        assert_eq!(front_matter("\u{feff}---\n---\n"), Some((3, 11)));
        assert_eq!(front_matter("---\nnever closed\n"), None);
        assert_eq!(front_matter("# T\n---\n---\n"), None);
    }
}
