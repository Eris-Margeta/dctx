use crate::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use crate::ignore::should_ignore_item;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc}; // Maknut Mutex
use std::thread;
use std::time::SystemTime;

// --- API ---

pub fn walk_directory_structure(
    root_disk_path: &Path,
    ignore_rules: &[IgnoreRule],
) -> io::Result<DirContextTreeNode> {
    if !root_disk_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Root path not found",
        ));
    }
    walk_recursive(root_disk_path, Path::new(""), ignore_rules)
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
        node.last_modified_timestamp = mtime
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
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
                        continue;
                    }

                    if let Ok(child_node) = walk_recursive(&child_path, &child_relative, rules) {
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
            node.file_content = Some(content);
        }
    } else {
        for child in &mut node.children {
            fill_tree_from_map(child, map);
        }
    }
}
