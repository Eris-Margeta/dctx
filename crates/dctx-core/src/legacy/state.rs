//! Legacy compatibility (DX-B0 bridge): loads the previous pack state or resets it on corruption.
//! Reads and deletes files on disk, so outside the pure core.
//! Depends on `reader`. Used by dctx-cli; dctx-pack's `FactStore` port replaces it.

use crate::config::LLM_CONTEXT_EXTENSION;
use crate::datatypes::DirContextTreeNode;
use crate::reader::read_dircontxt_file;
use crate::version::Version;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

/// Result of attempting to load the state.
pub enum StateLoadResult {
    Loaded(DirContextTreeNode, Version), // Successfully loaded tree AND version
    FreshV1,                             // No previous state or corruption detected
}

/// Main function to load state. If it fails or detects corruption/reset conditions,
/// it performs a hard reset (cleanup) and returns FreshV1.
pub fn load_or_reset_state(
    target_dir: &Path,
    binary_path: &Path,
    output_txt_dir: Option<&Path>,
    project_name: &str,
) -> StateLoadResult {
    // 1. If binary does not exist, it's a fresh start.
    if !binary_path.exists() {
        return StateLoadResult::FreshV1;
    }

    // 2. Attempt to read the binary file (Tree + Version)
    match read_dircontxt_file(binary_path) {
        Ok((loaded_root, loaded_version)) => {
            // 3. Integrity Check: Recreated Folder
            // If the folder on disk is "younger" than our history or completely mismatched,
            // we should reset.
            if is_folder_recreated(target_dir, &loaded_root) {
                eprintln!("[INFO] Detected recreated target folder. Resetting state to V1.");
                perform_full_cleanup(binary_path, output_txt_dir, project_name);
                return StateLoadResult::FreshV1;
            }

            // Everything is OK
            StateLoadResult::Loaded(loaded_root, loaded_version)
        }
        Err(e) => {
            // 4. Corruption Handling
            eprintln!("[WARN] Corrupted or invalid state file detected: {}", e);
            eprintln!("[INFO] Performing automatic cleanup and resetting to V1.");

            perform_full_cleanup(binary_path, output_txt_dir, project_name);
            StateLoadResult::FreshV1
        }
    }
}

/// Checks if the target directory has been deleted and recreated since the last run.
fn is_folder_recreated(target_dir: &Path, loaded_root: &DirContextTreeNode) -> bool {
    // We attempt to get metadata from the disk.
    if let Ok(metadata) = fs::metadata(target_dir) {
        if let Ok(mtime) = metadata.modified() {
            let disk_ts = mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // Compare disk timestamp with loaded root timestamp.
            // If disk_ts is significantly NEWER than the root node's timestamp,
            // it implies the folder was deleted and recreated.
            //
            // We use a threshold of 1 second to account for filesystem timestamp precision.
            // If the directory on disk is newer AND the difference is substantial (>1s),
            // we consider it recreated.
            if disk_ts > loaded_root.last_modified_timestamp {
                let diff = disk_ts - loaded_root.last_modified_timestamp;
                // Threshold: 1 second (accounts for FS timestamp granularity)
                if diff > 1 {
                    return true;
                }
            }

            return false;
        }
    }
    // If we can't read disk metadata, the walker will likely fail later anyway.
    false
}

/// Deletes the binary file AND all associated text outputs (Main context + Diffs).
fn perform_full_cleanup(binary_path: &Path, output_txt_dir: Option<&Path>, project_name: &str) {
    // 1. Delete binary file (.dircontxt or .dctx)
    if binary_path.exists() {
        if let Err(e) = fs::remove_file(binary_path) {
            eprintln!(
                "[ERROR] Failed to delete corrupted binary {}: {}",
                binary_path.display(),
                e
            );
        } else {
            eprintln!("[INFO] Deleted old binary state.");
        }
    }

    // 2. Delete text outputs (.llmcontext.txt and *-diff.txt)
    if let Some(out_dir) = output_txt_dir {
        if !out_dir.exists() {
            return;
        }

        // Define patterns to look for
        let main_file_name = format!("{}.{}", project_name, LLM_CONTEXT_EXTENSION);
        let diff_prefix = format!("{}.{}-", project_name, LLM_CONTEXT_EXTENSION); // e.g. "MyProj.llmcontext.txt-"

        // Iterate over the directory
        if let Ok(entries) = fs::read_dir(out_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name_os) = path.file_name() {
                    let name = name_os.to_string_lossy();

                    // A) Main Context File OR B) Diff Files (check prefix and suffix)
                    let should_delete = name == main_file_name
                        || (name.starts_with(&diff_prefix) && name.ends_with("-diff.txt"));

                    if should_delete {
                        if let Err(e) = fs::remove_file(&path) {
                            eprintln!(
                                "[WARN] Failed to delete output file {}: {}",
                                path.display(),
                                e
                            );
                        } else {
                            eprintln!("[INFO] Cleaned up: {}", name);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datatypes::NodeType;
    use crate::writer::write_dircontxt_file;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_dir_node(path: &str) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::Directory,
            path,
            PathBuf::from(format!("/test/{}", path)),
        );
        node.last_modified_timestamp = 1234567890;
        node
    }

    fn create_file_node(path: &str, content: &[u8]) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::File,
            path,
            PathBuf::from(format!("/test/{}", path)),
        );
        node.content_size = content.len() as u64;
        node.file_content = Some(content.to_vec());
        node.last_modified_timestamp = 1234567890;
        node
    }

    // --- LOAD_OR_RESET_STATE TESTS ---

    #[test]
    fn test_load_or_reset_state_no_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("nonexistent.dctx");

        let result = load_or_reset_state(temp_dir.path(), &binary_path, None, "TestProject");

        match result {
            StateLoadResult::FreshV1 => {} // Expected
            StateLoadResult::Loaded(_, _) => panic!("Expected FreshV1 when binary doesn't exist"),
        }
    }

    #[test]
    fn test_load_or_reset_state_valid_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");

        // Create a valid binary file with CURRENT timestamp to avoid recreation detection
        let mut root =
            DirContextTreeNode::new(NodeType::Directory, "", temp_dir.path().to_path_buf());

        // Set timestamp to current (matches disk, won't trigger recreation)
        if let Ok(metadata) = fs::metadata(temp_dir.path()) {
            if let Ok(mtime) = metadata.modified() {
                root.last_modified_timestamp = mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
            }
        }

        root.children.push(create_file_node("test.txt", b"Hello"));
        let version = Version { major: 1, minor: 5 };
        write_dircontxt_file(&binary_path, &mut root, &version, false).unwrap();

        let result = load_or_reset_state(temp_dir.path(), &binary_path, None, "TestProject");

        match result {
            StateLoadResult::Loaded(loaded_root, loaded_version) => {
                assert_eq!(loaded_version.major, 1);
                assert_eq!(loaded_version.minor, 5);
                assert_eq!(loaded_root.children.len(), 1);
            }
            StateLoadResult::FreshV1 => panic!("Expected Loaded with valid binary"),
        }
    }

    #[test]
    fn test_load_or_reset_state_corrupted_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("corrupted.dctx");

        // Create a corrupted binary file
        fs::write(&binary_path, b"INVALID_CORRUPTED_DATA").unwrap();

        let result = load_or_reset_state(temp_dir.path(), &binary_path, None, "TestProject");

        match result {
            StateLoadResult::FreshV1 => {
                // Binary should have been deleted
                assert!(!binary_path.exists());
            }
            StateLoadResult::Loaded(_, _) => panic!("Expected FreshV1 with corrupted binary"),
        }
    }

    #[test]
    fn test_load_or_reset_state_corrupted_cleans_text_files() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("corrupted.dctx");
        let output_dir = temp_dir.path().join(".dctx");
        fs::create_dir(&output_dir).unwrap();

        // Create corrupted binary
        fs::write(&binary_path, b"INVALID_DATA").unwrap();

        // Create text output files that should be cleaned up
        let main_file = output_dir.join("TestProject.llmcontext.txt");
        let diff_file = output_dir.join("TestProject.llmcontext.txt-V1.1-diff.txt");
        fs::write(&main_file, "main content").unwrap();
        fs::write(&diff_file, "diff content").unwrap();

        let result = load_or_reset_state(
            temp_dir.path(),
            &binary_path,
            Some(&output_dir),
            "TestProject",
        );

        match result {
            StateLoadResult::FreshV1 => {
                // All files should have been cleaned up
                assert!(!binary_path.exists());
                assert!(!main_file.exists());
                assert!(!diff_file.exists());
            }
            StateLoadResult::Loaded(_, _) => panic!("Expected FreshV1"),
        }
    }

    // --- IS_FOLDER_RECREATED TESTS ---

    #[test]
    fn test_is_folder_recreated_detects_recreation() {
        let temp_dir = TempDir::new().unwrap();
        let root = create_dir_node("");

        // The temp_dir has current timestamp (2026), but root has timestamp from 2009.
        // This should be detected as recreation (disk is much newer).
        let result = is_folder_recreated(temp_dir.path(), &root);
        assert!(
            result,
            "Should detect folder recreation when disk is significantly newer"
        );
    }

    #[test]
    fn test_is_folder_recreated_same_timestamp() {
        let temp_dir = TempDir::new().unwrap();

        // Create a root node with current timestamp (simulating no recreation)
        let mut root =
            DirContextTreeNode::new(NodeType::Directory, "", temp_dir.path().to_path_buf());

        // Set timestamp to current time (within 1 second threshold)
        if let Ok(metadata) = fs::metadata(temp_dir.path()) {
            if let Ok(mtime) = metadata.modified() {
                root.last_modified_timestamp = mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
            }
        }

        // Should NOT detect recreation when timestamps are similar
        let result = is_folder_recreated(temp_dir.path(), &root);
        assert!(
            !result,
            "Should not detect recreation when timestamps match"
        );
    }

    #[test]
    fn test_is_folder_recreated_nonexistent_dir() {
        let root = create_dir_node("");

        // Non-existent directory should return false
        let result = is_folder_recreated(Path::new("/nonexistent/path"), &root);
        assert!(!result);
    }

    // --- PERFORM_FULL_CLEANUP TESTS ---

    #[test]
    fn test_perform_full_cleanup_deletes_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");

        // Create a binary file
        fs::write(&binary_path, b"dummy content").unwrap();
        assert!(binary_path.exists());

        perform_full_cleanup(&binary_path, None, "TestProject");

        assert!(!binary_path.exists());
    }

    #[test]
    fn test_perform_full_cleanup_deletes_main_context_file() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");
        let output_dir = temp_dir.path().join(".dctx");
        fs::create_dir(&output_dir).unwrap();

        // Create main context file
        let main_file = output_dir.join("TestProject.llmcontext.txt");
        fs::write(&main_file, "content").unwrap();
        assert!(main_file.exists());

        perform_full_cleanup(&binary_path, Some(&output_dir), "TestProject");

        assert!(!main_file.exists());
    }

    #[test]
    fn test_perform_full_cleanup_deletes_diff_files() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");
        let output_dir = temp_dir.path().join(".dctx");
        fs::create_dir(&output_dir).unwrap();

        // Create diff files
        let diff1 = output_dir.join("TestProject.llmcontext.txt-V1.1-diff.txt");
        let diff2 = output_dir.join("TestProject.llmcontext.txt-V1.2-diff.txt");
        fs::write(&diff1, "diff1").unwrap();
        fs::write(&diff2, "diff2").unwrap();
        assert!(diff1.exists());
        assert!(diff2.exists());

        perform_full_cleanup(&binary_path, Some(&output_dir), "TestProject");

        assert!(!diff1.exists());
        assert!(!diff2.exists());
    }

    #[test]
    fn test_perform_full_cleanup_preserves_unrelated_files() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");
        let output_dir = temp_dir.path().join(".dctx");
        fs::create_dir(&output_dir).unwrap();

        // Create unrelated files that should NOT be deleted
        let unrelated1 = output_dir.join("README.md");
        let unrelated2 = output_dir.join("OtherProject.llmcontext.txt");
        let unrelated3 = output_dir.join("random.txt");
        fs::write(&unrelated1, "readme").unwrap();
        fs::write(&unrelated2, "other project").unwrap();
        fs::write(&unrelated3, "random").unwrap();

        // Create target files that SHOULD be deleted
        let main_file = output_dir.join("TestProject.llmcontext.txt");
        fs::write(&main_file, "main").unwrap();

        perform_full_cleanup(&binary_path, Some(&output_dir), "TestProject");

        // Unrelated files should still exist
        assert!(unrelated1.exists());
        assert!(unrelated2.exists());
        assert!(unrelated3.exists());
        // Target file should be deleted
        assert!(!main_file.exists());
    }

    #[test]
    fn test_perform_full_cleanup_handles_nonexistent_output_dir() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("test.dctx");
        let nonexistent_dir = temp_dir.path().join("nonexistent");

        // Should not panic when output dir doesn't exist
        perform_full_cleanup(&binary_path, Some(&nonexistent_dir), "TestProject");
        // No assertion needed - just verifying it doesn't panic
    }

    #[test]
    fn test_perform_full_cleanup_handles_nonexistent_binary() {
        let temp_dir = TempDir::new().unwrap();
        let binary_path = temp_dir.path().join("nonexistent.dctx");

        // Should not panic when binary doesn't exist
        perform_full_cleanup(&binary_path, None, "TestProject");
        // No assertion needed - just verifying it doesn't panic
    }

    // --- STATE_LOAD_RESULT TESTS ---

    #[test]
    fn test_state_load_result_fresh_v1() {
        let result = StateLoadResult::FreshV1;
        match result {
            StateLoadResult::FreshV1 => {} // Expected
            StateLoadResult::Loaded(_, _) => panic!("Expected FreshV1"),
        }
    }

    #[test]
    fn test_state_load_result_loaded() {
        let root = create_dir_node("");
        let version = Version::new_v1();
        let result = StateLoadResult::Loaded(root, version);
        match result {
            StateLoadResult::Loaded(loaded_root, loaded_version) => {
                assert_eq!(loaded_root.node_type, NodeType::Directory);
                assert_eq!(loaded_version.major, 1);
            }
            StateLoadResult::FreshV1 => panic!("Expected Loaded"),
        }
    }
}
