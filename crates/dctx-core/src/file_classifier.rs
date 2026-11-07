// SPDX-License-Identifier: Apache-2.0
//! Binary file detection and content handling (extension first, then content sniffing).
//! Implements the pack rule that binaries appear in the tree with their content omitted.
//! Depends on its media, packages and sniff modules; pure over a path and bytes already read.
//! Used by [`crate::facts::content::apply_content`].

mod media;
mod packages;
mod sniff;

use std::path::Path;

/// Determines how file content should be handled in the output
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentHandling {
    /// Include full content (text files)
    IncludeContent,
    /// Binary file - omit content with message
    OmitBinary {
        /// Human-readable type label shown in place of the binary content.
        file_type: String,
    },
    /// SVG file - configurable omission
    OmitSvgContent,
}

/// Classifies a file to determine content handling strategy
#[allow(
    clippy::must_use_candidate,
    reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
)]
pub fn classify_file(path: &Path, content: &[u8]) -> FileContentHandling {
    // Extension first (most reliable); unrecognized extensions fall through to a byte sniff.
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_lowercase();
        if let Some(handling) = media::classify(&ext_lower) {
            return handling;
        }
        if let Some(handling) = packages::classify(&ext_lower) {
            return handling;
        }
    }
    if sniff::is_binary_content(content) {
        return FileContentHandling::OmitBinary {
            file_type: "Binary file".to_string(),
        };
    }
    FileContentHandling::IncludeContent
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_classify_png() {
        let path = PathBuf::from("test.png");
        let content = b"\x89PNG\r\n\x1a\n";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "PNG image".to_string()
            }
        );
    }

    #[test]
    fn test_classify_jpg() {
        let path = PathBuf::from("photo.jpg");
        let content = b"\xFF\xD8\xFF";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "JPEG image".to_string()
            }
        );
    }

    #[test]
    fn test_classify_svg() {
        let path = PathBuf::from("icon.svg");
        let content = b"<svg>";

        let result = classify_file(&path, content);
        assert_eq!(result, FileContentHandling::OmitSvgContent);
    }

    #[test]
    fn test_classify_pdf() {
        let path = PathBuf::from("doc.pdf");
        let content = b"%PDF-1.4";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "PDF document".to_string()
            }
        );
    }

    #[test]
    fn test_classify_text_file() {
        let path = PathBuf::from("readme.txt");
        let content = b"Hello, world!\nThis is text.";

        let result = classify_file(&path, content);
        assert_eq!(result, FileContentHandling::IncludeContent);
    }

    #[test]
    fn test_classify_rust_file() {
        let path = PathBuf::from("main.rs");
        let content = b"fn main() {\n    println!(\"Hello\");\n}";

        let result = classify_file(&path, content);
        assert_eq!(result, FileContentHandling::IncludeContent);
    }

    #[test]
    fn test_binary_content_with_null_bytes() {
        let path = PathBuf::from("unknown");
        let content = b"text\x00binary";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "Binary file".to_string()
            }
        );
    }

    #[test]
    fn test_binary_content_high_non_printable_ratio() {
        let path = PathBuf::from("unknown");
        // 50% non-printable bytes (>30% threshold)
        let content = vec![0x01, 0x02, 0x03, 0x04, b'a', b'b', b'c', b'd'];

        let result = classify_file(&path, &content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "Binary file".to_string()
            }
        );
    }

    #[test]
    fn test_empty_file() {
        let path = PathBuf::from("empty.txt");
        let content = b"";

        let result = classify_file(&path, content);
        assert_eq!(result, FileContentHandling::IncludeContent);
    }

    #[test]
    fn test_static_library() {
        let path = PathBuf::from("libpdfium.a");
        let content = b"!<arch>\n";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "Static library".to_string()
            }
        );
    }

    #[test]
    fn test_executable() {
        let path = PathBuf::from("program.exe");
        let content = b"MZ\x90\x00";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "Windows executable".to_string()
            }
        );
    }

    #[test]
    fn test_whitespace_is_not_binary() {
        let path = PathBuf::from("spaces.txt");
        let content = b"line1\n\nline2\r\n\tindented";

        let result = classify_file(&path, content);
        assert_eq!(result, FileContentHandling::IncludeContent);
    }

    #[test]
    fn test_case_insensitive_extension() {
        let path = PathBuf::from("IMAGE.PNG");
        let content = b"";

        let result = classify_file(&path, content);
        assert_eq!(
            result,
            FileContentHandling::OmitBinary {
                file_type: "PNG image".to_string()
            }
        );
    }
}
