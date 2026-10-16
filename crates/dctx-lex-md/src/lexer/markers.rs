//! Line-start markers: ATX and setext headings, code fences, thematic breaks, list items.
//! Implements CommonMark 0.31 §4.1–4.5 and §5.2 recognition rules.
//! Each function reads text after container prefixes and at most three columns of indent.

/// ATX heading level and title without the closing `#` sequence.
pub(crate) fn atx(text: &str) -> Option<(u8, &str)> {
    let hashes = text.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &text[hashes..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let title = rest.trim_matches([' ', '\t']);
    let open = title.trim_end_matches('#');
    let title = if open.is_empty() {
        ""
    } else if open.ends_with([' ', '\t']) {
        open.trim_end_matches([' ', '\t'])
    } else {
        title
    };
    Some((hashes as u8, title))
}

/// Setext underline level: a run of `=` (1) or `-` (2) and trailing whitespace only.
pub(crate) fn setext(text: &str) -> Option<u8> {
    let first = *text.as_bytes().first()?;
    if first != b'=' && first != b'-' {
        return None;
    }
    let run = text.bytes().take_while(|byte| *byte == first).count();
    super::lines::is_blank(&text[run..]).then_some(if first == b'=' { 1 } else { 2 })
}

/// Three or more `*`, `-` or `_`, optionally separated by spaces or tabs.
pub(crate) fn thematic_break(text: &str) -> bool {
    let Some(&first) = text.as_bytes().first() else {
        return false;
    };
    if !matches!(first, b'*' | b'-' | b'_') {
        return false;
    }
    let mut count = 0;
    for byte in text.bytes() {
        match byte {
            b' ' | b'\t' => {}
            _ if byte == first => count += 1,
            _ => return false,
        }
    }
    count >= 3
}

/// Opening code fence: its character, length and trimmed info string.
pub(crate) fn fence_open(text: &str) -> Option<(u8, usize, &str)> {
    let first = *text.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let len = text.bytes().take_while(|byte| *byte == first).count();
    let info = text[len..].trim_matches([' ', '\t']);
    (len >= 3 && !(first == b'`' && info.contains('`'))).then_some((first, len, info))
}

/// Closing fence for an open fence of `fence` repeated `len` times.
pub(crate) fn fence_close(text: &str, fence: u8, len: usize) -> bool {
    let run = text.bytes().take_while(|byte| *byte == fence).count();
    run >= len && super::lines::is_blank(&text[run..])
}

/// A bullet or ordered list marker at the start of the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListMarker {
    /// Marker width in bytes, such as 1 for `-` or 3 for `10.`.
    pub width: usize,
    /// Whether nothing but whitespace follows the marker.
    pub empty: bool,
    /// Whether the item may interrupt a paragraph (§5.2).
    pub interrupts: bool,
}

pub(crate) fn list_marker(text: &str) -> Option<ListMarker> {
    let bytes = text.as_bytes();
    let (width, starts_at_one) = match bytes.first()? {
        b'-' | b'+' | b'*' => (1, true),
        _ => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            if !(1..=9).contains(&digits) || !matches!(bytes.get(digits), Some(b'.' | b')')) {
                return None;
            }
            (digits + 1, text[..digits].trim_start_matches('0') == "1")
        }
    };
    let rest = &text[width..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let empty = super::lines::is_blank(rest);
    Some(ListMarker {
        width,
        empty,
        interrupts: !empty && starts_at_one,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atx_follows_commonmark_examples() {
        assert_eq!(atx("# foo"), Some((1, "foo")));
        assert_eq!(atx("###### foo"), Some((6, "foo")));
        assert_eq!(atx("####### foo"), None);
        assert_eq!(atx("#5 bolt"), None);
        assert_eq!(atx("#hashtag"), None);
        assert_eq!(atx("#\tfoo"), Some((1, "foo")));
        assert_eq!(
            atx("#                  foo                     "),
            Some((1, "foo"))
        );
        assert_eq!(atx("## foo ##"), Some((2, "foo")));
        assert_eq!(
            atx("# foo ##################################"),
            Some((1, "foo"))
        );
        assert_eq!(atx("### foo ###     "), Some((3, "foo")));
        assert_eq!(atx("### foo ### b"), Some((3, "foo ### b")));
        assert_eq!(atx("# foo#"), Some((1, "foo#")));
        assert_eq!(atx("### foo \\###"), Some((3, "foo \\###")));
        assert_eq!(atx("#"), Some((1, "")));
        assert_eq!(atx("### ###"), Some((3, "")));
        assert_eq!(atx("## "), Some((2, "")));
    }

    #[test]
    fn setext_underlines_are_single_runs() {
        assert_eq!(setext("==="), Some(1));
        assert_eq!(setext("-"), Some(2));
        assert_eq!(setext("---   "), Some(2));
        assert_eq!(setext("= ="), None);
        assert_eq!(setext("--- -"), None);
        assert_eq!(setext("=-"), None);
        assert_eq!(setext("text"), None);
    }

    #[test]
    fn thematic_breaks_need_three_like_characters() {
        assert!(thematic_break("***"));
        assert!(thematic_break("- - -"));
        assert!(thematic_break("_____________________________________"));
        assert!(!thematic_break("--"));
        assert!(!thematic_break("*-*"));
        assert!(!thematic_break("--- a"));
        assert!(!thematic_break("+++"));
    }

    #[test]
    fn fences_need_three_characters_and_matching_closers() {
        assert_eq!(fence_open("```rust"), Some((b'`', 3, "rust")));
        assert_eq!(fence_open("~~~~ a ` b"), Some((b'~', 4, "a ` b")));
        assert_eq!(fence_open("``` a`b"), None);
        assert_eq!(fence_open("``"), None);
        assert!(fence_close("````  ", b'`', 3));
        assert!(!fence_close("``", b'`', 3));
        assert!(!fence_close("~~~", b'`', 3));
        assert!(!fence_close("``` x", b'`', 3));
    }

    #[test]
    fn list_markers_know_when_they_interrupt_paragraphs() {
        let bullet = list_marker("- item").unwrap();
        assert_eq!(
            (bullet.width, bullet.empty, bullet.interrupts),
            (1, false, true)
        );
        let ordered = list_marker("10) x").unwrap();
        assert_eq!((ordered.width, ordered.interrupts), (3, false));
        assert!(list_marker("1. x").unwrap().interrupts);
        assert!(list_marker("-").unwrap().empty);
        assert!(!list_marker("- ").unwrap().interrupts);
        assert_eq!(list_marker("-item"), None);
        assert_eq!(list_marker("1234567890. x"), None);
        assert_eq!(list_marker("x. y"), None);
    }
}
