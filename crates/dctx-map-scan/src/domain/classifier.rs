// SPDX-License-Identifier: Apache-2.0
//! Binary and SVG content classification.
//! Ports dctx-core classifier behavior (ADR-009/015).
//! Pure byte rules used after traversal authorizes a content read.

use std::path::Path;

/// Determines how file content should be handled in the output
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentHandling {
    /// Include full content (text files)
    IncludeContent,
    /// Binary file - omit content with message
    OmitBinary {
        /// Human readable binary format.
        file_type: String,
    },
    /// SVG file - configurable omission
    OmitSvgContent,
}

/// Classifies a file to determine content handling strategy
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn classify_file(path: &Path, content: &[u8]) -> FileContentHandling {
    // 1. Check extension first (most reliable)
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_lowercase();

        // Images
        match ext_lower.as_str() {
            "png" => return binary("PNG image"),
            "jpg" | "jpeg" => return binary("JPEG image"),
            "gif" => return binary("GIF image"),
            "bmp" => return binary("BMP image"),
            "ico" => return binary("ICO icon"),
            "webp" => return binary("WebP image"),
            "avif" => return binary("AVIF image"),
            "tiff" | "tif" => return binary("TIFF image"),

            // SVG (special handling - configurable)
            "svg" => return FileContentHandling::OmitSvgContent,

            // Documents
            "pdf" => return binary("PDF document"),
            "doc" | "docx" => return binary("Word document"),
            "xls" | "xlsx" => return binary("Excel spreadsheet"),
            "ppt" | "pptx" => return binary("PowerPoint presentation"),
            "odt" => return binary("OpenDocument text"),
            "ods" => return binary("OpenDocument spreadsheet"),
            "odp" => return binary("OpenDocument presentation"),

            // Archives
            "zip" => return binary("ZIP archive"),
            "tar" => return binary("TAR archive"),
            "gz" | "gzip" => return binary("Gzip archive"),
            "bz2" => return binary("Bzip2 archive"),
            "xz" => return binary("XZ archive"),
            "7z" => return binary("7-Zip archive"),
            "rar" => return binary("RAR archive"),

            // Videos
            "mp4" => return binary("MP4 video"),
            "mov" => return binary("QuickTime video"),
            "avi" => return binary("AVI video"),
            "mkv" => return binary("Matroska video"),
            "webm" => return binary("WebM video"),
            "flv" => return binary("Flash video"),
            "wmv" => return binary("Windows Media video"),
            "m4v" => return binary("M4V video"),

            // Audio
            "mp3" => return binary("MP3 audio"),
            "wav" => return binary("WAV audio"),
            "ogg" => return binary("OGG audio"),
            "flac" => return binary("FLAC audio"),
            "aac" => return binary("AAC audio"),
            "m4a" => return binary("M4A audio"),
            "wma" => return binary("WMA audio"),

            // Executables
            "exe" => return binary("Windows executable"),
            "dll" => return binary("DLL library"),
            "so" => return binary("Shared library"),
            "dylib" => return binary("Dynamic library"),
            "bin" => return binary("Binary file"),
            "app" => return binary("Application bundle"),

            // Fonts
            "ttf" => return binary("TrueType font"),
            "otf" => return binary("OpenType font"),
            "woff" | "woff2" => return binary("Web font"),
            "eot" => return binary("EOT font"),

            // Database files
            "db" | "sqlite" | "sqlite3" => return binary("SQLite database"),

            // Compiled/Object files
            "o" | "obj" => return binary("Object file"),
            "a" | "lib" => return binary("Static library"),
            "pyc" => return binary("Python bytecode"),
            "class" => return binary("Java bytecode"),

            _ => {}
        }
    }

    // 2. Content-based detection (for files without recognized extensions)
    if is_binary_content(content) {
        return FileContentHandling::OmitBinary {
            file_type: "Binary file".to_string(),
        };
    }

    // Default: include content (text file)
    FileContentHandling::IncludeContent
}

fn binary(file_type: &str) -> FileContentHandling {
    FileContentHandling::OmitBinary {
        file_type: file_type.to_owned(),
    }
}

/// Analyzes file content to determine if it's binary
#[allow(clippy::cast_precision_loss, reason = "Both counts are at most the 8192-byte sample size and are represented exactly by f64.")]
fn is_binary_content(content: &[u8]) -> bool {
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
