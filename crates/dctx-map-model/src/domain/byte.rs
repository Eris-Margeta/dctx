//! Half-open byte spans and their one-based line projections.
//! Implements ARCHITECTURE-PROPOSAL §3 and PARSERS-v1 span rules.
//! Depends only on the standard library. Facts and the future wire adapter carry these values.
//! Line numbers count `\n` and do not allocate.

/// Maximum addressable byte in a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(u64);

impl ByteOffset {
    /// Offset in bytes from the start of the authorized input.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Raw byte index.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Count of bytes, including zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteLength(u64);

impl ByteLength {
    /// A length that was already checked by the caller.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Raw byte count.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Why a byte span cannot be built or projected onto text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidByteSpan {
    /// `end` is before `start`.
    Inverted,
    /// The span does not fit in the text.
    OutOfRange,
    /// `start` or `end` splits a UTF-8 scalar.
    NotUtf8Boundary,
    /// The projected line number does not fit in `u32`.
    LineOverflow,
}

impl std::fmt::Display for InvalidByteSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Inverted => "byte span end is before start",
            Self::OutOfRange => "byte span is outside the text",
            Self::NotUtf8Boundary => "byte span is not on a utf-8 boundary",
            Self::LineOverflow => "line number does not fit in u32",
        })
    }
}

impl std::error::Error for InvalidByteSpan {}

/// Half-open byte interval `[start, end)` inside one authorized input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteSpan {
    start: ByteOffset,
    end: ByteOffset,
}

impl ByteSpan {
    /// Builds `[start, end)`. The end is exclusive.
    pub const fn new(start: ByteOffset, end: ByteOffset) -> Result<Self, InvalidByteSpan> {
        if end.get() < start.get() {
            Err(InvalidByteSpan::Inverted)
        } else {
            Ok(Self { start, end })
        }
    }

    /// Span of `text` whose ends are UTF-8 boundaries.
    pub fn on_utf8(text: &str, start: ByteOffset, end: ByteOffset) -> Result<Self, InvalidByteSpan> {
        let span = Self::new(start, end)?;
        span.lines_in(text)?;
        Ok(span)
    }

    /// Inclusive start.
    pub const fn start(self) -> ByteOffset {
        self.start
    }

    /// Exclusive end.
    pub const fn end(self) -> ByteOffset {
        self.end
    }

    /// Number of bytes covered.
    pub const fn len(self) -> ByteLength {
        ByteLength(self.end.0 - self.start.0)
    }

    /// True when the span covers no bytes.
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }

    /// One-based inclusive line projection. Lines split on `\n`.
    pub fn lines_in(self, text: &str) -> Result<[u32; 2], InvalidByteSpan> {
        let start = usize::try_from(self.start.0).map_err(|_| InvalidByteSpan::OutOfRange)?;
        let end = usize::try_from(self.end.0).map_err(|_| InvalidByteSpan::OutOfRange)?;
        if end > text.len() {
            return Err(InvalidByteSpan::OutOfRange);
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(InvalidByteSpan::NotUtf8Boundary);
        }
        let start_line = line_number(text, start)?;
        let end_line = if start == end {
            start_line
        } else {
            line_number(text, end - 1)?
        };
        Ok([start_line, end_line])
    }
}

fn line_number(text: &str, byte: usize) -> Result<u32, InvalidByteSpan> {
    let newlines = text[..byte].bytes().filter(|value| *value == b'\n').count();
    let count = u32::try_from(newlines).map_err(|_| InvalidByteSpan::LineOverflow)?;
    count.checked_add(1).ok_or(InvalidByteSpan::LineOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_spans_project_lines_and_refusals_name_the_fault() {
        let text = "a\nbc\né";
        let span = ByteSpan::on_utf8(text, ByteOffset::new(2), ByteOffset::new(4)).unwrap();
        assert_eq!(span.len().get(), 2);
        assert!(!span.is_empty());
        assert_eq!(span.lines_in(text).unwrap(), [2, 2]);
        let whole = ByteSpan::on_utf8(text, ByteOffset::new(0), ByteOffset::new(4)).unwrap();
        assert_eq!(whole.lines_in(text).unwrap(), [1, 2]);
        let empty = ByteSpan::on_utf8(text, ByteOffset::new(0), ByteOffset::new(0)).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.lines_in(text).unwrap(), [1, 1]);
        assert_eq!(
            ByteSpan::new(ByteOffset::new(3), ByteOffset::new(1)),
            Err(InvalidByteSpan::Inverted)
        );
        assert_eq!(
            ByteSpan::on_utf8(text, ByteOffset::new(0), ByteOffset::new(99)),
            Err(InvalidByteSpan::OutOfRange)
        );
        let split = text.find('é').unwrap() as u64 + 1;
        assert_eq!(
            ByteSpan::on_utf8(text, ByteOffset::new(split), ByteOffset::new(split + 1)),
            Err(InvalidByteSpan::NotUtf8Boundary)
        );
    }
}
