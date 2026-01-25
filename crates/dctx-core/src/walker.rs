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
) -> io::Result<DirContextTreeNode> {
    let mut ignored_folders = Vec::new();
    walk_directory_structure_with_debug(root_disk_path, ignore_rules, &mut ignored_folders)
}

pub fn walk_directory_structure_with_debug(
    root_disk_path: &Path,
    ignore_rules: &[IgnoreRule],
    ignored_folders: &mut Vec<String>,
) -> io::Result<DirContextTreeNode> {
    if !root_disk_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Root path not found",
        ));
    }
    walk_recursive(root_disk_path, Path::new(""), ignore_rules, ignored_folders)
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

                    if should_ignore_item(
                        &child_relative.to_string_lossy(),
                        &file_name_str,
                        is_dir,
                        rules,
                    ) {
                        // Collect ignored folders for debugging
                        if is_dir {
                            ignored_folders.push(child_relative.to_string_lossy().to_string());
                        }
                        continue;
                    }

                    if let Ok(child_node) = walk_recursive(&child_path, &child_relative, rules, ignored_folders) {
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

fn collect_file_paths(node: &DirContextTreeNode, collector: &mut Vec<PathBuf>) {
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
