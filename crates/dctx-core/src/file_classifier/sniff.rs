//! Content sniff for files whose extension is not a known binary type.
//! Implements the null-byte and non-printable ratio check used when the extension is inconclusive.
//! Depends on nothing; pure over bytes already read under a grant.
//! Used by [`super::classify_file`].
//! Empty input is text; a null byte, or over 30% non-printable in the first 8KB, is binary.

/// Analyzes file content to determine if it's binary
#[allow(clippy::cast_precision_loss, reason = "Both counts are at most 8192, within the exact integer range of f64.")]
pub(super) fn is_binary_content(content: &[u8]) -> bool {
    // Empty files are not binary
    if content.is_empty() {
        return false;
    }

    // Check for null bytes (strong indicator of binary)
    if content.contains(&0) {
        return true;
    }

    // Analyze first 8KB (or entire file if smaller)
    let sample_size = content.len().min(8192);
    let sample = &content[..sample_size];

    // Count non-printable characters (excluding common whitespace)
    let non_printable_count = sample
        .iter()
        .filter(|&&b| {
            // Allow: tab, newline, carriage return, printable ASCII
            b < 32 && b != b'\t' && b != b'\n' && b != b'\r'
        })
        .count();

    // If more than 30% non-printable → binary
    let ratio = non_printable_count as f64 / sample_size as f64;
    ratio > 0.30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_text_and_a_null_byte_is_binary() {
        assert!(!is_binary_content(b""));
        assert!(is_binary_content(b"text\x00binary"));
    }
}
