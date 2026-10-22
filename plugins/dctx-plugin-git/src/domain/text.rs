//! A validated `.gitignore` body: bounded, UTF-8, no NUL, with its newline style and lines.
//! Implements plugin-api git.md "invalid UTF-8/NUL => typed refusal" and "preserve all bytes, newline
//! style and old markers" (G-COVERAGE, G-REPLACE). Pure; the bytes come from the broker's file.read.
//! Used by `domain::plan` and `domain::coverage`.

/// Largest `.gitignore` the v1 profile reads (the manifest's `file.read.max_bytes`).
pub const MAX_IGNORE_BYTES: u64 = 1_048_576;

/// Why a `.gitignore` body was refused before any planning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRefusal {
    /// Larger than the read ceiling.
    Oversized,
    /// Not valid UTF-8.
    InvalidUtf8,
    /// Contains a NUL byte.
    ContainsNul,
}

impl TextRefusal {
    /// Stable safe reason name for wire error details.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Oversized => "ignore-oversized",
            Self::InvalidUtf8 => "ignore-invalid-utf8",
            Self::ContainsNul => "ignore-contains-nul",
        }
    }
}

/// The line ending used for appended lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Newline {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    /// The ending's bytes.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// A validated ignore-file body; the original bytes are kept verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreText {
    text: String,
}

impl IgnoreText {
    /// Validates `bytes` (size first, then NUL, then UTF-8).
    /// # Errors
    /// Refuses oversized text, NUL bytes, or invalid UTF-8.
    pub fn parse(bytes: &[u8], max_bytes: u64) -> Result<Self, TextRefusal> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > max_bytes {
            return Err(TextRefusal::Oversized);
        }
        if bytes.contains(&0) {
            return Err(TextRefusal::ContainsNul);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| TextRefusal::InvalidUtf8)?;
        Ok(Self { text: text.to_owned() })
    }

    /// The original text.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Byte length of the original text.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the file is empty.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Style of the first line ending; LF when the file has none.
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn newline(&self) -> Newline {
        match self.text.find('\n') {
            Some(index) if index > 0 && self.text.as_bytes()[index - 1] == b'\r' => Newline::CrLf,
            _ => Newline::Lf,
        }
    }

    /// Whether appending needs a separating line ending first (nonempty, unterminated last line).
    #[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
    pub fn needs_separator(&self) -> bool {
        !self.text.is_empty() && !self.text.ends_with('\n')
    }

    /// Logical lines without their `\n` or `\r\n` ending.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        let body = self.text.strip_suffix('\n').unwrap_or(&self.text);
        let empty = self.text.is_empty();
        body.split('\n').filter(move |_| !empty).map(|line| line.strip_suffix('\r').unwrap_or(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_have_positive_controls() {
        assert!(IgnoreText::parse(b"notes\n", MAX_IGNORE_BYTES).is_ok());
        assert_eq!(IgnoreText::parse(b"a\0b", MAX_IGNORE_BYTES), Err(TextRefusal::ContainsNul));
        assert_eq!(IgnoreText::parse(&[0xff, b'\n'], MAX_IGNORE_BYTES), Err(TextRefusal::InvalidUtf8));
        assert_eq!(IgnoreText::parse(b"abcd", 3), Err(TextRefusal::Oversized));
        assert!(IgnoreText::parse(b"abc", 3).is_ok());
        assert!(IgnoreText::parse("ünïcödé\n".as_bytes(), MAX_IGNORE_BYTES).is_ok());
    }

    #[test]
    fn newline_style_and_lines() {
        let crlf = IgnoreText::parse(b"a\r\nb\r\n", MAX_IGNORE_BYTES).expect("valid");
        assert_eq!(crlf.newline(), Newline::CrLf);
        assert_eq!(crlf.lines().collect::<Vec<_>>(), ["a", "b"]);
        assert!(!crlf.needs_separator());
        let bare = IgnoreText::parse(b"notes", MAX_IGNORE_BYTES).expect("valid");
        assert_eq!(bare.newline(), Newline::Lf);
        assert!(bare.needs_separator());
        assert_eq!(bare.lines().collect::<Vec<_>>(), ["notes"]);
        let empty = IgnoreText::parse(b"", MAX_IGNORE_BYTES).expect("valid");
        assert_eq!(empty.lines().count(), 0);
        assert!(!empty.needs_separator());
        let blank = IgnoreText::parse(b"\n\nx", MAX_IGNORE_BYTES).expect("valid");
        assert_eq!(blank.lines().collect::<Vec<_>>(), ["", "", "x"]);
    }
}
