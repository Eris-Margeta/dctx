//! Legacy compatibility (DX-B0 bridge): walks the real filesystem into a pack tree.
//! Calls `std::fs` directly; decisions come from the pure policy through `IgnoreEngine`.
//! The port-based equivalent is `dctx_pack::snapshot`, which reproduces this tree over `Source`.
//! Used by dctx-cli and the desktop until they move to the facade.

use crate::content_reader::ContentReader;
use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::domain::{FileHandlingDecision, ItemContext};
use crate::filter::FilterConfig;
use crate::ignore::IgnoreEngine;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// --- API ---

pub fn walk_directory_structure(
    root_disk_path: &Path,
    engine: &IgnoreEngine,
    debug_ignored: bool,
) -> io::Result<DirContextTreeNode> {
    if !root_disk_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Root path not found",
        ));
    }

    // Determine if root is a directory (single metadata call for root)
    let root_metadata = fs::metadata(root_disk_path)?;
    let is_root_dir = root_metadata.is_dir();

    // Root is always included
    walk_recursive(
        root_disk_path,
        Path::new(""),
        engine,
        debug_ignored,
        FileHandlingDecision::Include,
        is_root_dir,
    )
}

/// Populate file contents in parallel
///
/// # Deprecated
/// This function is deprecated. Use `ContentReader::populate()` instead.
/// This wrapper is kept for backward compatibility and will be removed in a future version.
#[deprecated(
    since = "0.10.8",
    note = "Use ContentReader::populate() instead for better separation of concerns"
)]
pub fn populate_file_contents_parallel(root: &mut DirContextTreeNode) {
    ContentReader::populate(root);
}

// --- Helperi ---

fn walk_recursive(
    current_disk_path: &Path,
    current_relative_path: &Path,
    engine: &IgnoreEngine,
    debug_ignored: bool,
    decision: FileHandlingDecision,
    is_directory: bool,
) -> io::Result<DirContextTreeNode> {
    // Determine node type from parameter (avoids redundant metadata call)
    let node_type = if is_directory {
        NodeType::Directory
    } else {
        NodeType::File
    };

    let mut node = DirContextTreeNode::new(
        node_type,
        &current_relative_path.to_string_lossy(),
        current_disk_path.to_path_buf(),
    );

    // PERFORMANCE OPTIMIZATION: Only read metadata if needed
    // Based on Phase 10.8.1 profiling: fs::metadata() takes ~17μs per call.
    // Skipping this for excluded files saves 5-10 seconds on large scans.
    if decision.needs_metadata() {
        // Metadata is needed for files shown in tree (size, mtime display)
        if let Ok(metadata) = fs::metadata(current_disk_path) {
            if let Ok(mtime) = metadata.modified() {
                node.last_modified_timestamp = mtime
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
            }
            if node_type == NodeType::File {
                node.content_size = metadata.len();
            }
        }
        // If metadata read fails, continue with defaults (0 size, 0 timestamp)
    } else {
        // Metadata not needed (excluded files) - use defaults
        node.last_modified_timestamp = 0;
        node.content_size = 0;
    }

    if node_type == NodeType::Directory {
        match fs::read_dir(current_disk_path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let file_name_str = file_name.to_string_lossy();
                    let child_path = entry.path();

                    let child_relative = if current_relative_path.as_os_str().is_empty() {
                        PathBuf::from(&file_name)
                    } else {
                        current_relative_path.join(&file_name)
                    };

                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);

                    // SINGLE DECISION POINT - Use IgnoreEngine
                    let context = ItemContext {
                        relative_path: &child_relative.to_string_lossy(),
                        name: &file_name_str,
                        is_directory: is_dir,
                        debug_mode: debug_ignored,
                    };

                    let decision = engine.decide(&context);

                    // Simple, linear control flow
                    if !decision.show_in_tree() {
                        continue; // Excluded - skip entirely
                    }

                    // Recurse if needed
                    if let Ok(mut child_node) = walk_recursive(
                        &child_path,
                        &child_relative,
                        engine,
                        debug_ignored,
                        decision,
                        is_dir,
                    ) {
                        // Set handling decision on node
                        child_node.handling_decision = decision;

                        // Mark ignored nodes and subtrees
                        if decision == FileHandlingDecision::DebugVisible {
                            child_node.is_ignored = true;
                            mark_subtree_as_ignored(&mut child_node);
                        }

                        node.children.push(child_node);
                    }
                }
            }
            Err(e) => eprintln!("Error reading dir {:?}: {}", current_disk_path, e),
        }
        node.children
            .sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    }
    Ok(node)
}

/// Recursively marks all nodes in a subtree as ignored.
/// Used in debug mode to ensure ignored folder children are also marked.
fn mark_subtree_as_ignored(node: &mut DirContextTreeNode) {
    crate::facts::content::mark_subtree_ignored(node);
}


// --- Filtering ---

/// Apply filters to an existing tree, returning a filtered copy.
/// This function creates a new tree containing only items that pass the filter criteria.
/// Empty directories (after filtering children) are automatically pruned.
///
/// # Arguments
/// * `tree` - The tree to filter
/// * `filter` - The filter configuration to apply
///
/// # Returns
/// A new tree containing only filtered items, or None if everything was filtered out
pub fn filter_tree(tree: &DirContextTreeNode, filter: &FilterConfig) -> Option<DirContextTreeNode> {
    // If no filters are active, return a clone of the original tree
    if !filter.has_active_filters() {
        return Some(tree.clone());
    }

    match tree.node_type {
        NodeType::File => {
            // For files, check if they pass the filter
            let file_name = tree
                .disk_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if filter.should_include_file(
                file_name,
                tree.content_size,
                tree.is_binary_file,
                &tree.disk_path,
            ) {
                Some(tree.clone())
            } else {
                None
            }
        }
        NodeType::Directory => {
            let dir_name = tree
                .disk_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            // Check directory-level filters (e.g., hidden filter)
            // BUT don't filter out the root directory (empty relative_path)
            if !tree.relative_path.is_empty() && !filter.should_include_directory(dir_name) {
                return None;
            }

            // Recursively filter children
            let mut filtered_children = Vec::new();
            for child in &tree.children {
                if let Some(filtered_child) = filter_tree(child, filter) {
                    filtered_children.push(filtered_child);
                }
            }

            // If directory has no children after filtering, prune it
            // UNLESS it's the root directory (relative_path is empty)
            if filtered_children.is_empty() && !tree.relative_path.is_empty() {
                return None;
            }

            // Create filtered directory node
            let mut filtered_dir = tree.clone();
            filtered_dir.children = filtered_children;
            Some(filtered_dir)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for bug: ignored directories being recursed into
    ///
    /// This test verifies that directories matching ignore patterns (like .turbo/,
    /// .pnpm-store/, .localstack/) are completely skipped during directory traversal,
    /// preventing performance issues from scanning large cache directories.
    ///
    /// Bug Report: docs/DOCUMENTS/BUG-REPORT-IGNORED-DIRECTORIES-RECURSION.md
    #[test]
    fn test_ignored_directories_never_recursed() {
        use crate::ignore::IgnoreEngine;
        use tempfile::TempDir;

        // Create a temporary directory with ignored cache directories
        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create .turbo/cache/ with files (Turborepo cache)
        let turbo_dir = root.join(".turbo/cache");
        fs::create_dir_all(&turbo_dir).unwrap();
        fs::write(turbo_dir.join("build-1.tar.zst"), b"turbo cache data").unwrap();
        fs::write(turbo_dir.join("build-2.tar.zst"), b"turbo cache data").unwrap();

        // Create .pnpm-store/v10/files/ with files (pnpm content-addressable store)
        let pnpm_dir = root.join(".pnpm-store/v10/files/1d");
        fs::create_dir_all(&pnpm_dir).unwrap();
        fs::write(pnpm_dir.join("package-hash-1"), b"pnpm package data").unwrap();
        fs::write(pnpm_dir.join("package-hash-2"), b"pnpm package data").unwrap();

        // Create .localstack/state/ with files (LocalStack AWS emulator)
        let localstack_dir = root.join(".localstack/state/s3");
        fs::create_dir_all(&localstack_dir).unwrap();
        fs::write(localstack_dir.join("bucket-state.db"), b"localstack state").unwrap();
        fs::write(localstack_dir.join("objects.db"), b"localstack objects").unwrap();

        // Create normal files that should NOT be ignored
        fs::write(root.join("README.md"), b"# Project README").unwrap();
        fs::write(root.join("package.json"), b"{}").unwrap();

        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), b"fn main() {}").unwrap();

        // Build IgnoreEngine (will load global rules + any local .dctxignore)
        let engine = IgnoreEngine::build(root, false).unwrap();

        // Walk the directory with engine
        let tree = walk_directory_structure(root, &engine, false).unwrap();

        // Collect all paths in the tree
        let mut all_paths = Vec::new();
        fn collect_paths(node: &DirContextTreeNode, paths: &mut Vec<String>) {
            paths.push(node.relative_path.clone());
            for child in &node.children {
                collect_paths(child, paths);
            }
        }
        collect_paths(&tree, &mut all_paths);

        // CRITICAL ASSERTIONS: Ignored directories should NOT appear in tree
        for path in &all_paths {
            assert!(
                !path.contains(".turbo"),
                "Found .turbo in tree: '{}' - ignored directories should be completely skipped!",
                path
            );
            assert!(
                !path.contains(".pnpm-store"),
                "Found .pnpm-store in tree: '{}' - ignored directories should be completely skipped!",
                path
            );
            assert!(
                !path.contains(".localstack"),
                "Found .localstack in tree: '{}' - ignored directories should be completely skipped!",
                path
            );
        }

        // Verify that normal files ARE present
        let path_strings: Vec<&str> = all_paths.iter().map(|s| s.as_str()).collect();
        assert!(
            path_strings.contains(&"README.md"),
            "Normal files should be included in tree"
        );
        assert!(
            path_strings.contains(&"package.json"),
            "Normal files should be included in tree"
        );
        assert!(
            path_strings.iter().any(|p| p.contains("main.rs")),
            "Normal files in subdirectories should be included"
        );

        // Additional verification: tree should be small (only 4-5 nodes: root, README, package.json, src/, src/main.rs)
        // If ignored directories were recursed, we'd have 10+ nodes
        assert!(
            all_paths.len() <= 5,
            "Tree should only contain ~5 nodes (root + normal files), got {} - ignored dirs may have been included!",
            all_paths.len()
        );
    }

    /// Test that included files have metadata populated (size, mtime).
    ///
    /// Phase 10.8.3: Metadata Skip Optimization - Test 2
    #[test]
    fn test_included_files_read_metadata() {
        use crate::ignore::IgnoreEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create a normal file that should be included
        let test_content = b"Hello, world!";
        fs::write(root.join("test.txt"), test_content).unwrap();

        let engine = IgnoreEngine::build(root, false).unwrap();
        let tree = walk_directory_structure(root, &engine, false).unwrap();

        // Find the test.txt node
        let test_file = tree
            .children
            .iter()
            .find(|n| n.relative_path == "test.txt")
            .expect("test.txt should be in tree");

        // Verify metadata is populated
        assert_eq!(
            test_file.content_size,
            test_content.len() as u64,
            "Included files should have size metadata"
        );
        assert!(
            test_file.last_modified_timestamp > 0,
            "Included files should have mtime metadata"
        );
    }

    /// Test that debug visible files (ignored but shown with --debug-ignored) have metadata.
    ///
    /// Phase 10.8.3: Metadata Skip Optimization - Test 3
    #[test]
    fn test_debug_visible_files_read_metadata() {
        use crate::ignore::IgnoreEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create .git directory with config file (definitely ignored by hardcoded rules)
        fs::create_dir(root.join(".git")).unwrap();
        let config_content = b"[core]\n\trepositoryformatversion = 0";
        fs::write(root.join(".git/config"), config_content).unwrap();

        let engine = IgnoreEngine::build(root, false).unwrap();

        // Walk with debug_ignored=true to make ignored files visible
        let tree = walk_directory_structure(root, &engine, true).unwrap();

        // Find the .git directory node
        let git_dir = tree
            .children
            .iter()
            .find(|n| n.relative_path == ".git")
            .expect(".git should be visible in debug mode");

        // Verify it's marked as ignored
        assert!(
            git_dir.is_ignored,
            "Debug visible directories should have is_ignored=true"
        );

        // Verify metadata is populated (needed for tree display)
        assert!(
            git_dir.last_modified_timestamp > 0,
            "Debug visible directories should have mtime metadata for tree display"
        );

        // Find the .git/config file
        let config_file = git_dir
            .children
            .iter()
            .find(|n| n.relative_path == ".git/config")
            .expect(".git/config should be visible in debug mode");

        // Verify it's marked as ignored
        assert!(
            config_file.is_ignored,
            "Debug visible files should have is_ignored=true"
        );

        // Verify metadata is populated
        assert_eq!(
            config_file.content_size,
            config_content.len() as u64,
            "Debug visible files should have size metadata for tree display"
        );
        assert!(
            config_file.last_modified_timestamp > 0,
            "Debug visible files should have mtime metadata for tree display"
        );
    }

    /// Test that OmitContent files (shown in tree but content omitted) have metadata.
    ///
    /// Phase 10.8.3: Metadata Skip Optimization - Test 4
    #[test]
    fn test_omit_content_files_read_metadata() {
        use crate::ignore::IgnoreEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create a package-lock.json file (content omitted by default)
        let lock_content = b"{\"name\": \"test\", \"lockfileVersion\": 2}";
        fs::write(root.join("package-lock.json"), lock_content).unwrap();

        let engine = IgnoreEngine::build(root, false).unwrap();
        let tree = walk_directory_structure(root, &engine, false).unwrap();

        // Find the package-lock.json node
        let lock_file = tree
            .children
            .iter()
            .find(|n| n.relative_path == "package-lock.json")
            .expect("package-lock.json should be in tree");

        // Verify it has OmitContent decision
        assert_eq!(
            lock_file.handling_decision,
            FileHandlingDecision::OmitContent,
            "package-lock.json should have OmitContent decision"
        );

        // Verify metadata is populated (needed for tree display)
        assert_eq!(
            lock_file.content_size,
            lock_content.len() as u64,
            "OmitContent files should have size metadata for tree display"
        );
        assert!(
            lock_file.last_modified_timestamp > 0,
            "OmitContent files should have mtime metadata for tree display"
        );
    }

    /// Test that excluded files never appear in tree (metadata skip is implicit).
    ///
    /// Phase 10.8.3: Metadata Skip Optimization - Test 1
    ///
    /// This test verifies the performance optimization: excluded files never have
    /// fs::metadata() called because they're filtered out before walk_recursive
    /// is invoked. This saves ~17μs per excluded file.
    #[test]
    fn test_excluded_files_never_in_tree() {
        use crate::ignore::IgnoreEngine;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let root = temp_dir.path();

        // Create files that should be excluded
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(root.join("node_modules/package.json"), b"{}").unwrap();
        fs::write(root.join(".DS_Store"), b"binary").unwrap();

        // Create a normal file for comparison
        fs::write(root.join("README.md"), b"# Test").unwrap();

        let engine = IgnoreEngine::build(root, false).unwrap();
        let tree = walk_directory_structure(root, &engine, false).unwrap();

        // Collect all paths
        let mut all_paths = Vec::new();
        fn collect_paths(node: &DirContextTreeNode, paths: &mut Vec<String>) {
            paths.push(node.relative_path.clone());
            for child in &node.children {
                collect_paths(child, paths);
            }
        }
        collect_paths(&tree, &mut all_paths);

        // Verify excluded files are NOT in tree
        assert!(
            !all_paths.iter().any(|p| p.contains("node_modules")),
            "Excluded directories should never appear in tree"
        );
        assert!(
            !all_paths.iter().any(|p| p.contains(".DS_Store")),
            "Excluded files should never appear in tree"
        );

        // Verify normal files ARE in tree
        assert!(
            all_paths.contains(&"README.md".to_string()),
            "Normal files should appear in tree"
        );
    }
}
