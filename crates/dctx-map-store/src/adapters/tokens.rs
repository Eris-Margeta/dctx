//! Lexical normalization of identifiers and file paths.
//! Implements spec section 6.4 camelCase and snake_case splitting.
//! Pure helper used when SQLite FTS rows are rebuilt.
pub(super) fn tokens(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut result = String::new();
    for (i, &ch) in chars.iter().enumerate() {
        if !ch.is_alphanumeric() {
            result.push(' ');
            continue;
        }
        if i > 0
            && ch.is_uppercase()
            && (chars[i - 1].is_lowercase()
                || chars[i - 1].is_numeric()
                || (chars[i - 1].is_uppercase()
                    && chars.get(i + 1).is_some_and(|c| c.is_lowercase())))
        {
            result.push(' ');
        }
        result.extend(ch.to_lowercase());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splits_identifiers_and_paths() {
        assert_eq!(
            tokens("src/HTTPServer/read_file.rs"),
            "src http server read file rs"
        );
    }
}
