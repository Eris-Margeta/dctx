//! HTML block starts and end conditions, so raw HTML is never read as Markdown.
//! Implements CommonMark 0.31 §4.6 (all seven start conditions, simplified type 7).
//! Pure line tests; used by the block scanner.

/// Condition that ends an open HTML block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HtmlEnd {
    /// Type 1: a line containing a closing raw-text tag.
    RawText,
    /// Types 2–5: a line containing this marker.
    Marker(&'static str),
    /// Types 6–7: a blank line.
    Blank,
}

const RAW_TEXT: [&str; 4] = ["pre", "script", "style", "textarea"];

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// HTML block start; type 7 (any other tag) cannot interrupt a paragraph.
pub(crate) fn start(text: &str, in_paragraph: bool) -> Option<HtmlEnd> {
    let rest = text.strip_prefix('<')?;
    if rest.starts_with("!--") {
        return Some(HtmlEnd::Marker("-->"));
    }
    if rest.starts_with('?') {
        return Some(HtmlEnd::Marker("?>"));
    }
    if rest.starts_with("![CDATA[") {
        return Some(HtmlEnd::Marker("]]>"));
    }
    if rest
        .strip_prefix('!')
        .is_some_and(|bang| bang.starts_with(|c: char| c.is_ascii_alphabetic()))
    {
        return Some(HtmlEnd::Marker(">"));
    }
    let (closing, name_part) = match rest.strip_prefix('/') {
        Some(name_part) => (true, name_part),
        None => (false, rest),
    };
    let name_len = name_part
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if name_len == 0 || !name_part.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let name = name_part[..name_len].to_ascii_lowercase();
    let after = &name_part[name_len..];
    let ends_name = after.is_empty() || after.starts_with([' ', '\t', '>']);
    if !closing && ends_name && RAW_TEXT.contains(&name.as_str()) {
        return Some(HtmlEnd::RawText);
    }
    if (ends_name || after.starts_with("/>")) && BLOCK_TAGS.contains(&name.as_str()) {
        return Some(HtmlEnd::Blank);
    }
    let whole_tag = text
        .find('>')
        .is_some_and(|close| super::lines::is_blank(&text[close + 1..]));
    (!in_paragraph && whole_tag && (ends_name || after.starts_with('/'))).then_some(HtmlEnd::Blank)
}

/// Whether `text` satisfies a type 1–5 end condition.
pub(crate) fn ends(text: &str, end: HtmlEnd) -> bool {
    match end {
        HtmlEnd::RawText => {
            let lower = text.to_ascii_lowercase();
            RAW_TEXT
                .iter()
                .any(|tag| lower.contains(&format!("</{tag}>")))
        }
        HtmlEnd::Marker(marker) => text.contains(marker),
        HtmlEnd::Blank => false,
    }
}

/// Whether the start line already closes the block it opens.
pub(crate) fn ends_on_start(text: &str, end: HtmlEnd) -> bool {
    match end {
        HtmlEnd::Marker(marker) => {
            let skip = match marker {
                "-->" => 4,
                "]]>" => 9,
                _ => 2,
            };
            text.get(skip..).is_some_and(|rest| rest.contains(marker))
        }
        _ => ends(text, end),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_cover_the_seven_kinds() {
        assert_eq!(start("<script>", true), Some(HtmlEnd::RawText));
        assert_eq!(start("<!-- note", true), Some(HtmlEnd::Marker("-->")));
        assert_eq!(start("<?php", true), Some(HtmlEnd::Marker("?>")));
        assert_eq!(start("<!DOCTYPE html>", true), Some(HtmlEnd::Marker(">")));
        assert_eq!(start("<![CDATA[", true), Some(HtmlEnd::Marker("]]>")));
        assert_eq!(start("<div class=\"x\">", true), Some(HtmlEnd::Blank));
        assert_eq!(start("</DIV>", true), Some(HtmlEnd::Blank));
        assert_eq!(start("<custom-tag>", false), Some(HtmlEnd::Blank));
        assert_eq!(start("<custom-tag>", true), None);
        assert_eq!(start("<a href=\"x\">link</a> text", false), None);
        assert_eq!(start("<a href=\"x\">link</a>", false), None);
        assert_eq!(start("<a href=\"x\">  ", false), Some(HtmlEnd::Blank));
        assert_eq!(start("< div>", false), None);
        assert_eq!(start("<1>", false), None);
        assert_eq!(start("plain", false), None);
    }

    #[test]
    fn end_conditions_match_their_kind() {
        assert!(ends("x </SCRIPT> y", HtmlEnd::RawText));
        assert!(!ends("<script>", HtmlEnd::RawText));
        assert!(ends("done -->", HtmlEnd::Marker("-->")));
        assert!(!ends("", HtmlEnd::Blank));
        assert!(ends_on_start("<!-- one line -->", HtmlEnd::Marker("-->")));
        assert!(!ends_on_start("<!-->", HtmlEnd::Marker("-->")));
        assert!(ends_on_start("<pre>x</pre>", HtmlEnd::RawText));
    }
}
