//! Hidden-name and extension helpers for pack file filters.
//! Implements the name rules used by [`super::include`] (leading dot, last extension).
//! Depends on nothing; pure over a file or directory name.
//! Used by [`super::include`] and, for the secret-shaped rule, by the pack scan.
//! A leading-dot name such as `.gitignore` has extension `gitignore`.

/// Check if a file/folder name is hidden (starts with '.')
pub(super) fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

/// True for env-shaped file names that pack never prints by default: `.env`, `.env.*`, `*.env`, `.envrc`.
/// The match is ASCII case-insensitive and looks at the final path component only.
#[must_use]
pub fn is_secret_shaped_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == ".env" || lower == ".envrc" || lower.starts_with(".env.") || lower.ends_with(".env")
}

/// Extract file extension from filename
/// For files like ".gitignore", returns "gitignore"
/// For files like "file.txt", returns "txt"
/// For files like "archive.tar.gz", returns "gz"
pub(super) fn get_extension(file_name: &str) -> Option<&str> {
    // Find the last dot
    if let Some(pos) = file_name.rfind('.') {
        // If dot is at position 0 (e.g., ".gitignore"), treat everything after as extension
        if pos == 0 && file_name.len() > 1 {
            return Some(&file_name[1..]);
        }
        // Normal case: dot is not at the start or end
        if pos > 0 && pos < file_name.len() - 1 {
            return Some(&file_name[pos + 1..]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_hidden_function() {
        assert!(is_hidden(".gitignore"));
        assert!(is_hidden(".env"));
        assert!(is_hidden(".config"));
        assert!(!is_hidden("file.txt"));
        assert!(!is_hidden(".")); // Current dir marker
        assert!(!is_hidden("..")); // Parent dir marker
    }

    #[test]
    fn secret_shaped_names_cover_env_files_only() {
        for name in [".env", ".env.local", ".env.production", "settings.env", ".envrc", ".ENV", "prod.ENV"] {
            assert!(is_secret_shaped_name(name), "{name}");
        }
        for name in ["environment.rs", "env.rs", "envrc", "src", ".environment", "app.py", "myenv"] {
            assert!(!is_secret_shaped_name(name), "{name}");
        }
    }

    #[test]
    fn test_get_extension_function() {
        assert_eq!(get_extension("file.txt"), Some("txt"));
        assert_eq!(get_extension("archive.tar.gz"), Some("gz"));
        assert_eq!(get_extension("Makefile"), None);
        assert_eq!(get_extension(".gitignore"), Some("gitignore"));
        assert_eq!(get_extension("file."), None);
        assert_eq!(get_extension(".hidden"), Some("hidden"));
    }
}
