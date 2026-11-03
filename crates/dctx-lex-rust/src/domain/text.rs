// SPDX-License-Identifier: Apache-2.0
//! Name and doc text rules shared by both extraction paths.
//! Implements the oracle's naming (SPEC-MAP-MODE §5.1): `::` scopes, compact type text.
//! Pure string functions; used by the syn adapter and the line scan.

/// Longest kept doc line, in bytes, cut on a character boundary.
pub(crate) const MAX_DOC_BYTES: usize = 240;

pub(crate) fn qualify(scope: &str, name: &str) -> String {
    if scope.is_empty() {
        name.into()
    } else {
        format!("{scope}::{name}")
    }
}

pub(crate) fn compact(value: &str) -> String {
    value.chars().filter(|c| !c.is_whitespace()).collect()
}

pub(crate) fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub(crate) fn unraw(ident: &str) -> &str {
    ident.strip_prefix("r#").unwrap_or(ident)
}

/// First non-empty trimmed line of doc text, bounded to [`MAX_DOC_BYTES`].
pub(crate) fn first_doc_line<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let line = lines
        .into_iter()
        .flat_map(str::lines)
        .map(|line| line.trim().trim_start_matches('*').trim())
        .find(|line| !line.is_empty())?;
    let mut end = line.len().min(MAX_DOC_BYTES);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    Some(line[..end].to_owned())
}

/// Scope that names an impl's items: a path self type loses its generics (`Vec<T>` → `Vec`).
pub(crate) fn impl_scope(self_type: &str) -> &str {
    if self_type.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == ':') {
        self_type.split('<').next().unwrap_or(self_type)
    } else {
        self_type
    }
}

/// `test` or `<path>::test`, the attribute paths that mark a test function.
pub(crate) fn is_test_path(path: &str) -> bool {
    path == "test" || path.ends_with("::test")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_docs_follow_the_oracle_rules() {
        assert_eq!(qualify("", "a"), "a");
        assert_eq!(qualify("m", "a"), "m::a");
        assert_eq!(compact("Vec < T >"), "Vec<T>");
        assert_eq!(unraw("r#type"), "type");
        assert_eq!(
            first_doc_line(["", "  First line.\nSecond", "x"]),
            Some("First line.".into())
        );
        assert_eq!(first_doc_line(["", " "]), None);
        assert_eq!(impl_scope("crate::Vec<T>"), "crate::Vec");
        assert_eq!(impl_scope("&'aFoo<T>"), "&'aFoo<T>");
        assert!(is_test_path("tokio::test"));
        assert!(!is_test_path("contest"));
    }

    #[test]
    fn long_docs_are_cut_on_a_char_boundary() {
        let long = "é".repeat(MAX_DOC_BYTES);
        let line = first_doc_line([long.as_str()]).unwrap_or_default();
        assert!(line.len() <= MAX_DOC_BYTES);
        assert!(line.chars().all(|c| c == 'é'));
    }
}
