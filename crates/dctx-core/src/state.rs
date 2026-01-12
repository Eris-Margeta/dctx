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
fn is_folder_recreated(target_dir: &Path, _loaded_root: &DirContextTreeNode) -> bool {
    // We attempt to get metadata from the disk.
    if let Ok(metadata) = fs::metadata(target_dir) {
        if let Ok(mtime) = metadata.modified() {
            let _disk_ts = mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            // TODO: In the future, we can compare _disk_ts with _loaded_root.last_modified_timestamp.
            // If disk_ts is significantly NEWER than the root node's timestamp from history,
            // it implies the folder was recreated.
            // For now, we return false to be conservative.
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

                    let mut should_delete = false;

                    // A) Main Context File
                    if name == main_file_name {
                        should_delete = true;
                    }
                    // B) Diff Files (check prefix and suffix)
                    else if name.starts_with(&diff_prefix) && name.ends_with("-diff.txt") {
                        should_delete = true;
                    }

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
