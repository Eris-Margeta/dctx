// SPDX-License-Identifier: Apache-2.0
//! Content facts: how read bytes and debug-visible subtrees are recorded on tree nodes.
//! Implements the content classification of the pack format (text kept, binaries omitted,
//! SVG kept but labelled) and the `[IGNORED]` subtree marking of `--debug-ignored`.
//! Depends on [`crate::datatypes`] and [`crate::file_classifier`]; pure.
//! Used by the legacy content reader and walker and by the dctx-pack snapshot engine.

#![deny(missing_docs)]

use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::file_classifier::{classify_file, FileContentHandling};

/// Records `content` on a file node: sets its size and classifies it by extension and bytes.
/// Directory nodes are left unchanged.
pub fn apply_content(node: &mut DirContextTreeNode, content: &[u8]) {
    if node.node_type != NodeType::File {
        return;
    }
    node.content_size = content.len() as u64;
    match classify_file(&node.disk_path, content) {
        FileContentHandling::IncludeContent => {
            node.file_content = Some(content.to_vec());
            node.is_binary_file = false;
            node.binary_file_type = None;
        }
        FileContentHandling::OmitBinary { file_type } => {
            node.file_content = None;
            node.is_binary_file = true;
            node.binary_file_type = Some(file_type);
        }
        FileContentHandling::OmitSvgContent => {
            node.file_content = Some(content.to_vec());
            node.is_binary_file = false;
            node.binary_file_type = Some("SVG file".to_string());
        }
    }
}

/// Marks `node` and every descendant as ignored (shown, never read).
pub fn mark_subtree_ignored(node: &mut DirContextTreeNode) {
    node.is_ignored = true;
    for child in &mut node.children {
        mark_subtree_ignored(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file(path: &str) -> DirContextTreeNode {
        DirContextTreeNode::new(NodeType::File, path, PathBuf::from(path))
    }

    #[test]
    fn text_is_kept_and_sized() {
        let mut node = file("a.rs");
        apply_content(&mut node, b"fn main() {}\n");
        assert_eq!(node.content_size, 13);
        assert_eq!(node.file_content.as_deref(), Some(&b"fn main() {}\n"[..]));
        assert!(!node.is_binary_file);
    }

    #[test]
    fn binaries_are_omitted_and_labelled() {
        let mut node = file("logo.png");
        apply_content(&mut node, &[0x89, b'P', b'N', b'G', 0, 1]);
        assert!(node.file_content.is_none());
        assert!(node.is_binary_file);
        assert_eq!(node.binary_file_type.as_deref(), Some("PNG image"));
        assert_eq!(node.content_size, 6);
    }

    #[test]
    fn svg_is_kept_and_labelled() {
        let mut node = file("icon.svg");
        apply_content(&mut node, b"<svg/>");
        assert!(node.file_content.is_some());
        assert_eq!(node.binary_file_type.as_deref(), Some("SVG file"));
    }

    #[test]
    fn directories_ignore_content_and_subtrees_mark() {
        let mut dir = DirContextTreeNode::new(NodeType::Directory, "d", PathBuf::from("d"));
        apply_content(&mut dir, b"x");
        assert_eq!(dir.content_size, 0);
        dir.children.push(file("d/a"));
        mark_subtree_ignored(&mut dir);
        assert!(dir.is_ignored && dir.children[0].is_ignored);
    }
}
