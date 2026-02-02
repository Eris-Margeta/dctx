// crates/dctx-core/src/walker.rs
use crate::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use crate::file_classifier::{classify_file, FileContentHandling};
use crate::filter::FilterConfig;
use crate::ignore::should_ignore_item;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::SystemTime;

// --- API ---

pub fn walk_directory_structure(
    root_disk_path: &Path,
    ignore_rules: &[IgnoreRule],
    debug_ignored: bool,
) -> io::Result<DirContextTreeNode> {
    let mut ignored_folders = Vec::new();
    walk_directory_structure_with_debug(root_disk_path, ignore_rules, &mut ignored_folders, debug_ignored)
}

pub fn walk_directory_structure_with_debug(
    root_disk_path: &Path,
    ignore_rules: &[IgnoreRule],
    ignored_folders: &mut Vec<String>,
    debug_ignored: bool,
) -> io::Result<DirContextTreeNode> {
    if !root_disk_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Root path not found",
        ));
    }
    walk_recursive(root_disk_path, Path::new(""), ignore_rules, ignored_folders, debug_ignored)
}

pub fn populate_file_contents_parallel(root: &mut DirContextTreeNode) {
    let mut tasks: Vec<PathBuf> = Vec::new();
    collect_file_paths(root, &mut tasks);

    if tasks.is_empty() {
        return;
    }

    let task_count = tasks.len();
    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let num_threads = std::cmp::min(max_threads, task_count);

    let (tx, rx) = mpsc::channel();
    let tasks_arc = Arc::new(tasks);

    for i in 0..num_threads {
        let tx_thread = tx.clone();
        let tasks_ref = tasks_arc.clone();

        thread::spawn(move || {
            let mut idx = i;
            while idx < tasks_ref.len() {
                let path = &tasks_ref[idx];
                let result = match fs::read(path) {
                    Ok(content) => Some((path.clone(), content)),
                    Err(e) => {
                        eprintln!("Error reading {:?}: {}", path, e);
                        None
                    }
                };

                if let Some(data) = result {
                    let _ = tx_thread.send(data);
                }
                idx += num_threads;
            }
        });
    }
    drop(tx);

    let mut content_map: HashMap<PathBuf, Vec<u8>> = HashMap::new();
    for (path, content) in rx {
        content_map.insert(path, content);
    }

    fill_tree_from_map(root, &mut content_map);
}

// --- Helperi ---

fn walk_recursive(
    current_disk_path: &Path,
    current_relative_path: &Path,
    rules: &[IgnoreRule],
    ignored_folders: &mut Vec<String>,
    debug_ignored: bool,
) -> io::Result<DirContextTreeNode> {
    let metadata = fs::metadata(current_disk_path)?;
    let node_type = if metadata.is_dir() {
        NodeType::Directory
    } else {
        NodeType::File
    };

    let mut node = DirContextTreeNode::new(
        node_type,
        &current_relative_path.to_string_lossy(),
        current_disk_path.to_path_buf(),
    );

    if let Ok(mtime) = metadata.modified() {
        // FIX: Use as_millis() for higher precision to detect rapid changes
        node.last_modified_timestamp = mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }
    if node_type == NodeType::File {
        node.content_size = metadata.len();
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

                    let is_ignored = should_ignore_item(
                        &child_relative.to_string_lossy(),
                        &file_name_str,
                        is_dir,
                        rules,
                    );

                    // DEBUG: Log ignored directory detection
                    if is_dir && is_ignored && (file_name_str.contains("turbo") || file_name_str.contains("localstack") || file_name_str.contains("pnpm")) {
                        eprintln!("🔍 DEBUG: Found ignored dir: name='{}', rel_path='{}', is_ignored={}, debug_mode={}",
                            file_name_str, child_relative.display(), is_ignored, debug_ignored);
                    }

                    if is_ignored {
                        // Collect ignored folders for debugging
                        if is_dir {
                            ignored_folders.push(child_relative.to_string_lossy().to_string());
                        }

                        // In default mode: skip entirely (preserve current behavior)
                        if !debug_ignored {
                            // DEBUG: Confirm we're skipping
                            if is_dir && (file_name_str.contains("turbo") || file_name_str.contains("localstack") || file_name_str.contains("pnpm")) {
                                eprintln!("✅ DEBUG: SKIPPING ignored dir: '{}'", file_name_str);
                            }
                            continue;
                        }

                        // In debug mode: recurse but mark as ignored
                        // Fall through to create node and recurse
                    }

                    // DEBUG: Log when we DO recurse
                    if is_dir && (file_name_str.contains("turbo") || file_name_str.contains("localstack") || file_name_str.contains("pnpm")) {
                        eprintln!("⚠️  DEBUG: RECURSING into dir: name='{}', is_ignored={}", file_name_str, is_ignored);
                    }

                    if let Ok(mut child_node) =
                        walk_recursive(&child_path, &child_relative, rules, ignored_folders, debug_ignored)
                    {
                        if is_ignored {
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
    node.is_ignored = true;
    for child in &mut node.children {
        mark_subtree_as_ignored(child);
    }
}

fn collect_file_paths(node: &DirContextTreeNode, collector: &mut Vec<PathBuf>) {
    // CRITICAL: Skip ignored files - NEVER read them
    if node.is_ignored {
        return;
    }

    if node.node_type == NodeType::File {
        collector.push(node.disk_path.clone());
    } else {
        for child in &node.children {
            collect_file_paths(child, collector);
        }
    }
}

fn fill_tree_from_map(node: &mut DirContextTreeNode, map: &mut HashMap<PathBuf, Vec<u8>>) {
    if node.node_type == NodeType::File {
        if let Some(content) = map.remove(&node.disk_path) {
            node.content_size = content.len() as u64;

            // Classify the file to determine content handling
            let handling = classify_file(&node.disk_path, &content);

            match handling {
                FileContentHandling::IncludeContent => {
                    // Normal text file - include content
                    node.file_content = Some(content);
                    node.is_binary_file = false;
                    node.binary_file_type = None;
                }
                FileContentHandling::OmitBinary { file_type } => {
                    // Binary file - omit content
                    node.file_content = None;
                    node.is_binary_file = true;
                    node.binary_file_type = Some(file_type);
                }
                FileContentHandling::OmitSvgContent => {
                    // SVG - special handling (store content but mark for conditional omission)
                    node.file_content = Some(content);
                    node.is_binary_file = false;
                    node.binary_file_type = Some("SVG file".to_string());
                }
            }
        }
    } else {
        for child in &mut node.children {
            fill_tree_from_map(child, map);
        }
    }
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

    #[test]
    fn test_collect_file_paths_skips_ignored() {
        let mut root = DirContextTreeNode::new(
            NodeType::Directory,
            "root",
            PathBuf::from("/root"),
        );

        // Normal file (should be collected)
        let normal_file = DirContextTreeNode::new(
            NodeType::File,
            "normal.txt",
            PathBuf::from("/root/normal.txt"),
        );

        // Ignored file (should be skipped - PRIVACY GUARANTEE)
        let mut ignored_file = DirContextTreeNode::new(
            NodeType::File,
            "secret.env",
            PathBuf::from("/root/secret.env"),
        );
        ignored_file.is_ignored = true;

        root.children.push(normal_file);
        root.children.push(ignored_file);

        let mut paths = Vec::new();
        collect_file_paths(&root, &mut paths);

        // CRITICAL ASSERTION: Only 1 file should be collected
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/root/normal.txt"));
        // secret.env should NOT be in paths - privacy guarantee enforced
        assert!(!paths.contains(&PathBuf::from("/root/secret.env")));
    }

    #[test]
    fn test_collect_file_paths_skips_ignored_subtree() {
        let mut root = DirContextTreeNode::new(
            NodeType::Directory,
            "root",
            PathBuf::from("/root"),
        );

        // Normal folder with files
        let mut normal_dir = DirContextTreeNode::new(
            NodeType::Directory,
            "src",
            PathBuf::from("/root/src"),
        );
        let normal_file = DirContextTreeNode::new(
            NodeType::File,
            "main.rs",
            PathBuf::from("/root/src/main.rs"),
        );
        normal_dir.children.push(normal_file);

        // Ignored folder with files (entire subtree should be skipped)
        let mut ignored_dir = DirContextTreeNode::new(
            NodeType::Directory,
            ".git",
            PathBuf::from("/root/.git"),
        );
        ignored_dir.is_ignored = true;

        let mut ignored_file = DirContextTreeNode::new(
            NodeType::File,
            "config",
            PathBuf::from("/root/.git/config"),
        );
        ignored_file.is_ignored = true;
        ignored_dir.children.push(ignored_file);

        root.children.push(normal_dir);
        root.children.push(ignored_dir);

        let mut paths = Vec::new();
        collect_file_paths(&root, &mut paths);

        // CRITICAL: Only normal files collected, ignored subtree completely skipped
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/root/src/main.rs"));
        assert!(!paths.contains(&PathBuf::from("/root/.git/config")));
    }

    /// Regression test for bug: ignored directories being recursed into
    ///
    /// This test verifies that directories matching ignore patterns (like .turbo/,
    /// .pnpm-store/, .localstack/) are completely skipped during directory traversal,
    /// preventing performance issues from scanning large cache directories.
    ///
    /// Bug Report: docs/DOCUMENTS/BUG-REPORT-IGNORED-DIRECTORIES-RECURSION.md
    #[test]
    fn test_ignored_directories_never_recursed() {
        use crate::ignore::parse_ignore_pattern_line;
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

        // Parse ignore rules (simulating global-ignore file)
        let ignore_patterns = vec![
            ".turbo/",
            ".pnpm-store/",
            ".localstack/",
            ".git/",
        ];

        let ignore_rules: Vec<IgnoreRule> = ignore_patterns
            .iter()
            .filter_map(|pattern| parse_ignore_pattern_line(pattern))
            .collect();

        // Walk the directory with ignore rules
        let tree = walk_directory_structure(root, &ignore_rules, false).unwrap();

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
}
