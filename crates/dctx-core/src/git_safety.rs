//! Compile-compatible no-op after removal of the `.gitignore` writer.
//! Records decision 078 (16.10.2026): dctx must not use Git, and must not
//! write or stage `.gitignore`. Callers in the CLI keep this function.
//! Reading `.gitignore` as plain-text ignore rules stays in the scan crates.
//! This module no longer touches the filesystem or any process.

use std::{io, path::Path};

/// Does not create, modify, or stage `.gitignore`.
///
/// The argument is ignored. The signature stays so existing callers compile.
/// decision 078 removed the pattern append and the `git add .gitignore` invocation.
pub fn ensure_local_gitignore_is_safe(_target_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn missing_gitignore_is_not_created() {
        let temp = tempfile::tempdir().unwrap();
        assert!(ensure_local_gitignore_is_safe(temp.path()).is_ok());
        assert!(!temp.path().join(".gitignore").exists());
        assert!(ensure_local_gitignore_is_safe(temp.path()).is_ok());
        assert!(!temp.path().join(".gitignore").exists());
    }

    #[test]
    fn existing_gitignore_bytes_stay_unchanged() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".gitignore");
        let original = "node_modules\n";
        fs::write(&path, original).unwrap();
        assert!(ensure_local_gitignore_is_safe(temp.path()).is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn git_metadata_is_not_added_or_staged() {
        let temp = tempfile::tempdir().unwrap();
        let gitignore = temp.path().join(".gitignore");
        fs::write(&gitignore, "keep\n").unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        assert!(ensure_local_gitignore_is_safe(temp.path()).is_ok());
        assert_eq!(fs::read_to_string(&gitignore).unwrap(), "keep\n");
        assert!(!temp.path().join(".git/index").exists());
    }
}
