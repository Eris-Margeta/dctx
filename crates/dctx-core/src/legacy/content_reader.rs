//! Legacy compatibility (DX-B0 bridge): reads file contents from disk in parallel threads.
//! Privacy guarantee: only files whose decision (and every ancestor's) is Include are read;
//! ignored and omitted files are never read. Classification delegates to [`crate::facts::content`].
//! Used by dctx-cli and the desktop; the port-based replacement is dctx-pack's snapshot engine.

use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::facts::content::apply_content;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;

/// Content Reader - Handles parallel file content reading and tree population
pub struct ContentReader;

impl ContentReader {
    /// Populate file contents in the tree using parallel I/O
    ///
    /// This is the main entry point. It collects readable files, reads them in parallel,
    /// and injects the content back into the tree.
    ///
    /// # Privacy Guarantee
    /// Only files where `handling_decision.should_read_content()` returns true are read.
    /// This ensures ignored files (.env, credentials) are never loaded into memory.
    pub fn populate(tree: &mut DirContextTreeNode) {
        let readable_files = Self::collect_readable_files(tree);

        if readable_files.is_empty() {
            return;
        }

        let content_map = Self::read_parallel(&readable_files);
        Self::inject_content(tree, &content_map);
    }

    /// Collect paths of files that should have their content read
    ///
    /// # Privacy Guarantee
    /// This function respects `handling_decision.should_read_content()` and skips:
    /// - Ignored files (FileHandlingDecision::DebugVisible) - PRIVACY
    /// - Omitted files (FileHandlingDecision::OmitContent) - lockfiles, binaries
    ///
    /// Only files with FileHandlingDecision::Include are collected.
    fn collect_readable_files(node: &DirContextTreeNode) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        Self::collect_readable_files_recursive(node, &mut paths);
        paths
    }

    fn collect_readable_files_recursive(node: &DirContextTreeNode, collector: &mut Vec<PathBuf>) {
        // CRITICAL: Privacy and performance guarantee
        // Skip files that shouldn't be read according to their handling decision
        if !node.handling_decision.should_read_content() {
            return; // Skip ignored files (privacy) and omitted files (lockfiles)
        }

        if node.node_type == NodeType::File {
            collector.push(node.disk_path.clone());
        } else {
            // Recurse into directories
            for child in &node.children {
                Self::collect_readable_files_recursive(child, collector);
            }
        }
    }

    /// Read file contents in parallel using thread pool
    ///
    /// Returns a HashMap mapping file paths to their raw byte content.
    /// Files that fail to read are logged to stderr but don't block the process.
    fn read_parallel(file_paths: &[PathBuf]) -> HashMap<PathBuf, Vec<u8>> {
        let task_count = file_paths.len();
        let max_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let num_threads = std::cmp::min(max_threads, task_count);

        let (tx, rx) = mpsc::channel();
        let tasks_arc = Arc::new(file_paths.to_vec());

        // Spawn worker threads
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

        // Collect results
        let mut content_map = HashMap::new();
        for (path, content) in rx {
            content_map.insert(path, content);
        }

        content_map
    }

    /// Inject file contents from the map into the tree nodes
    ///
    /// This function recursively traverses the tree and updates file nodes with their content.
    /// It also performs file classification to determine if content should be included or omitted.
    fn inject_content(node: &mut DirContextTreeNode, content_map: &HashMap<PathBuf, Vec<u8>>) {
        if node.node_type == NodeType::File {
            if let Some(content) = content_map.get(&node.disk_path) {
                apply_content(node, content);
            }
        } else {
            // Recurse into directory children
            for child in &mut node.children {
                Self::inject_content(child, content_map);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FileHandlingDecision;

    #[test]
    fn test_collect_readable_files_skips_ignored() {
        let mut root = DirContextTreeNode::new(
            NodeType::Directory,
            "root",
            PathBuf::from("/root"),
        );

        // Normal file (should be collected)
        let mut normal_file = DirContextTreeNode::new(
            NodeType::File,
            "normal.txt",
            PathBuf::from("/root/normal.txt"),
        );
        normal_file.handling_decision = FileHandlingDecision::Include;

        // Ignored file (should be skipped - PRIVACY GUARANTEE)
        let mut ignored_file = DirContextTreeNode::new(
            NodeType::File,
            "secret.env",
            PathBuf::from("/root/secret.env"),
        );
        ignored_file.is_ignored = true;
        ignored_file.handling_decision = FileHandlingDecision::DebugVisible;

        root.children.push(normal_file);
        root.children.push(ignored_file);

        let paths = ContentReader::collect_readable_files(&root);

        // CRITICAL ASSERTION: Only 1 file should be collected
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/root/normal.txt"));
        // secret.env should NOT be in paths - privacy guarantee enforced
        assert!(!paths.contains(&PathBuf::from("/root/secret.env")));
    }

    #[test]
    fn test_collect_readable_files_skips_omitted() {
        let mut root = DirContextTreeNode::new(
            NodeType::Directory,
            "root",
            PathBuf::from("/root"),
        );

        // Normal file (should be collected)
        let mut normal_file = DirContextTreeNode::new(
            NodeType::File,
            "src.rs",
            PathBuf::from("/root/src.rs"),
        );
        normal_file.handling_decision = FileHandlingDecision::Include;

        // Omitted file (should be skipped - lockfile)
        let mut omitted_file = DirContextTreeNode::new(
            NodeType::File,
            "Cargo.lock",
            PathBuf::from("/root/Cargo.lock"),
        );
        omitted_file.handling_decision = FileHandlingDecision::OmitContent;

        root.children.push(normal_file);
        root.children.push(omitted_file);

        let paths = ContentReader::collect_readable_files(&root);

        // Only source file collected, lockfile skipped
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/root/src.rs"));
        assert!(!paths.contains(&PathBuf::from("/root/Cargo.lock")));
    }

    #[test]
    fn test_collect_readable_files_skips_ignored_subtree() {
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
        normal_dir.handling_decision = FileHandlingDecision::Include;

        let mut normal_file = DirContextTreeNode::new(
            NodeType::File,
            "main.rs",
            PathBuf::from("/root/src/main.rs"),
        );
        normal_file.handling_decision = FileHandlingDecision::Include;
        normal_dir.children.push(normal_file);

        // Ignored folder with files (entire subtree should be skipped)
        let mut ignored_dir = DirContextTreeNode::new(
            NodeType::Directory,
            ".git",
            PathBuf::from("/root/.git"),
        );
        ignored_dir.is_ignored = true;
        ignored_dir.handling_decision = FileHandlingDecision::DebugVisible;

        let mut ignored_file = DirContextTreeNode::new(
            NodeType::File,
            "config",
            PathBuf::from("/root/.git/config"),
        );
        ignored_file.is_ignored = true;
        ignored_file.handling_decision = FileHandlingDecision::DebugVisible;
        ignored_dir.children.push(ignored_file);

        root.children.push(normal_dir);
        root.children.push(ignored_dir);

        let paths = ContentReader::collect_readable_files(&root);

        // CRITICAL: Only normal files collected, ignored subtree completely skipped
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/root/src/main.rs"));
        assert!(!paths.contains(&PathBuf::from("/root/.git/config")));
    }

    #[test]
    fn test_inject_content_classifies_files() {
        use std::collections::HashMap;

        let mut file = DirContextTreeNode::new(
            NodeType::File,
            "test.txt",
            PathBuf::from("/test.txt"),
        );

        let mut content_map = HashMap::new();
        content_map.insert(PathBuf::from("/test.txt"), b"Hello, world!".to_vec());

        ContentReader::inject_content(&mut file, &content_map);

        assert_eq!(file.content_size, 13);
        assert!(file.file_content.is_some());
        assert!(!file.is_binary_file);
    }

    #[test]
    fn test_read_parallel_handles_errors() {
        // Test with non-existent files
        let paths = vec![
            PathBuf::from("/nonexistent/file1.txt"),
            PathBuf::from("/nonexistent/file2.txt"),
        ];

        let content_map = ContentReader::read_parallel(&paths);

        // Should return empty map without panicking
        assert_eq!(content_map.len(), 0);
    }
}
