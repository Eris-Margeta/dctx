//! Binary file detection and content handling (extension first, then content sniffing).
//! Implements the pack rule that binaries appear in the tree with their content omitted.
//! Depends on nothing; pure over a path (as data) and bytes already read under a grant.
//! Used by [`crate::facts::content::apply_content`].

use std::path::Path;

/// Determines how file content should be handled in the output
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContentHandling {
    /// Include full content (text files)
    IncludeContent,
    /// Binary file - omit content with message
    OmitBinary { file_type: String },
    /// SVG file - configurable omission
    OmitSvgContent,
}

/// Classifies a file to determine content handling strategy
pub fn classify_file(path: &Path, content: &[u8]) -> FileContentHandling {
    // 1. Check extension first (most reliable)
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_lowercase();

        // Images
        match ext_lower.as_str() {
            "png" => {
                return FileContentHandling::OmitBinary {
                    file_type: "PNG image".to_string(),
                }
            }
            "jpg" | "jpeg" => {
                return FileContentHandling::OmitBinary {
                    file_type: "JPEG image".to_string(),
                }
            }
            "gif" => {
                return FileContentHandling::OmitBinary {
                    file_type: "GIF image".to_string(),
                }
            }
            "bmp" => {
                return FileContentHandling::OmitBinary {
                    file_type: "BMP image".to_string(),
                }
            }
            "ico" => {
                return FileContentHandling::OmitBinary {
                    file_type: "ICO icon".to_string(),
                }
            }
            "webp" => {
                return FileContentHandling::OmitBinary {
                    file_type: "WebP image".to_string(),
                }
            }
            "avif" => {
                return FileContentHandling::OmitBinary {
                    file_type: "AVIF image".to_string(),
                }
            }
            "tiff" | "tif" => {
                return FileContentHandling::OmitBinary {
                    file_type: "TIFF image".to_string(),
                }
            }

            // SVG (special handling - configurable)
            "svg" => return FileContentHandling::OmitSvgContent,

            // Documents
            "pdf" => {
                return FileContentHandling::OmitBinary {
                    file_type: "PDF document".to_string(),
                }
            }
            "doc" | "docx" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Word document".to_string(),
                }
            }
            "xls" | "xlsx" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Excel spreadsheet".to_string(),
                }
            }
            "ppt" | "pptx" => {
                return FileContentHandling::OmitBinary {
                    file_type: "PowerPoint presentation".to_string(),
                }
            }
            "odt" => {
                return FileContentHandling::OmitBinary {
                    file_type: "OpenDocument text".to_string(),
                }
            }
            "ods" => {
                return FileContentHandling::OmitBinary {
                    file_type: "OpenDocument spreadsheet".to_string(),
                }
            }
            "odp" => {
                return FileContentHandling::OmitBinary {
                    file_type: "OpenDocument presentation".to_string(),
                }
            }

            // Archives
            "zip" => {
                return FileContentHandling::OmitBinary {
                    file_type: "ZIP archive".to_string(),
                }
            }
            "tar" => {
                return FileContentHandling::OmitBinary {
                    file_type: "TAR archive".to_string(),
                }
            }
            "gz" | "gzip" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Gzip archive".to_string(),
                }
            }
            "bz2" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Bzip2 archive".to_string(),
                }
            }
            "xz" => {
                return FileContentHandling::OmitBinary {
                    file_type: "XZ archive".to_string(),
                }
            }
            "7z" => {
                return FileContentHandling::OmitBinary {
                    file_type: "7-Zip archive".to_string(),
                }
            }
            "rar" => {
                return FileContentHandling::OmitBinary {
                    file_type: "RAR archive".to_string(),
                }
            }

            // Videos
            "mp4" => {
                return FileContentHandling::OmitBinary {
                    file_type: "MP4 video".to_string(),
                }
            }
            "mov" => {
                return FileContentHandling::OmitBinary {
                    file_type: "QuickTime video".to_string(),
                }
            }
            "avi" => {
                return FileContentHandling::OmitBinary {
                    file_type: "AVI video".to_string(),
                }
            }
            "mkv" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Matroska video".to_string(),
                }
            }
            "webm" => {
                return FileContentHandling::OmitBinary {
                    file_type: "WebM video".to_string(),
                }
            }
            "flv" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Flash video".to_string(),
                }
            }
            "wmv" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Windows Media video".to_string(),
                }
            }
            "m4v" => {
                return FileContentHandling::OmitBinary {
                    file_type: "M4V video".to_string(),
                }
            }

            // Audio
            "mp3" => {
                return FileContentHandling::OmitBinary {
                    file_type: "MP3 audio".to_string(),
                }
            }
            "wav" => {
                return FileContentHandling::OmitBinary {
                    file_type: "WAV audio".to_string(),
                }
            }
            "ogg" => {
                return FileContentHandling::OmitBinary {
                    file_type: "OGG audio".to_string(),
                }
            }
            "flac" => {
                return FileContentHandling::OmitBinary {
                    file_type: "FLAC audio".to_string(),
                }
            }
            "aac" => {
                return FileContentHandling::OmitBinary {
                    file_type: "AAC audio".to_string(),
                }
            }
            "m4a" => {
                return FileContentHandling::OmitBinary {
                    file_type: "M4A audio".to_string(),
                }
            }
            "wma" => {
                return FileContentHandling::OmitBinary {
                    file_type: "WMA audio".to_string(),
                }
            }

            // Executables
            "exe" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Windows executable".to_string(),
                }
            }
            "dll" => {
                return FileContentHandling::OmitBinary {
                    file_type: "DLL library".to_string(),
                }
            }
            "so" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Shared library".to_string(),
                }
            }
            "dylib" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Dynamic library".to_string(),
                }
            }
            "bin" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Binary file".to_string(),
                }
            }
            "app" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Application bundle".to_string(),
                }
            }

            // Fonts
            "ttf" => {
                return FileContentHandling::OmitBinary {
                    file_type: "TrueType font".to_string(),
                }
            }
            "otf" => {
                return FileContentHandling::OmitBinary {
                    file_type: "OpenType font".to_string(),
                }
            }
            "woff" | "woff2" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Web font".to_string(),
                }
            }
            "eot" => {
                return FileContentHandling::OmitBinary {
                    file_type: "EOT font".to_string(),
                }
            }

            // Database files
            "db" | "sqlite" | "sqlite3" => {
                return FileContentHandling::OmitBinary {
                    file_type: "SQLite database".to_string(),
                }
            }

            // Compiled/Object files
            "o" | "obj" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Object file".to_string(),
                }
            }
            "a" | "lib" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Static library".to_string(),
                }
            }
            "pyc" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Python bytecode".to_string(),
                }
            }
            "class" => {
                return FileContentHandling::OmitBinary {
                    file_type: "Java bytecode".to_string(),
                }
            }

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

/// Analyzes file content to determine if it's binary
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
