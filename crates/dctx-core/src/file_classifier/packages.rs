//! Extension classification for documents, archives, executables and compiled artifacts.
//! Implements the pack rule that these types appear in the tree with content omitted.
//! Depends on [`super::FileContentHandling`]; pure over a lowercased extension.
//! Used by [`super::classify_file`] when the extension is not a media type.
//! Unknown extensions return `None` so the caller falls through to a byte sniff.

use super::FileContentHandling;

/// Classification for a lowercased package extension, or `None` when it is not one of these.
pub(super) fn classify(ext: &str) -> Option<FileContentHandling> {
    let handling = match ext {
        "pdf" => "PDF document",
        "doc" | "docx" => "Word document",
        "xls" | "xlsx" => "Excel spreadsheet",
        "ppt" | "pptx" => "PowerPoint presentation",
        "odt" => "OpenDocument text",
        "ods" => "OpenDocument spreadsheet",
        "odp" => "OpenDocument presentation",
        "zip" => "ZIP archive",
        "tar" => "TAR archive",
        "gz" | "gzip" => "Gzip archive",
        "bz2" => "Bzip2 archive",
        "xz" => "XZ archive",
        "7z" => "7-Zip archive",
        "rar" => "RAR archive",
        "exe" => "Windows executable",
        "dll" => "DLL library",
        "so" => "Shared library",
        "dylib" => "Dynamic library",
        "bin" => "Binary file",
        "app" => "Application bundle",
        "db" | "sqlite" | "sqlite3" => "SQLite database",
        "o" | "obj" => "Object file",
        "a" | "lib" => "Static library",
        "pyc" => "Python bytecode",
        "class" => "Java bytecode",
        _ => return None,
    };
    Some(FileContentHandling::OmitBinary {
        file_type: handling.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_archive_and_unknown_extensions() {
        assert_eq!(
            classify("pdf"),
            Some(FileContentHandling::OmitBinary {
                file_type: "PDF document".to_string()
            })
        );
        assert_eq!(
            classify("a"),
            Some(FileContentHandling::OmitBinary {
                file_type: "Static library".to_string()
            })
        );
        assert_eq!(classify("rs"), None);
    }
}
