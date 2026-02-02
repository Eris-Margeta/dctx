use crate::config::DIRCONTXT_MAGIC_BYTES;
use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::version::Version;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Main function to write the .dircontxt binary file.
///
/// Process:
/// 1. Updates offsets in the tree (Preparation).
/// 2. Writes Signature (Magic Bytes).
/// 3. Writes Version Header (Major, Minor).
/// 4. Writes Tree Metadata (Header section).
/// 5. Writes File Content (Data section).
pub fn write_dircontxt_file(
    output_path: &Path,
    root: &mut DirContextTreeNode,
    version: &Version,
    debug_verbose: bool,
) -> io::Result<()> {
    let file = File::create(output_path)?;
    // Use BufWriter to minimize syscalls for small metadata writes
    let mut writer = BufWriter::new(file);

    // 1. Preparation: Calculate offsets for Data Section
    // We must know where every file starts before we write the Header.
    let mut current_offset = 0u64;
    calculate_offsets_recursive(root, &mut current_offset);

    // 2. Write Signature (8 bytes)
    writer.write_all(DIRCONTXT_MAGIC_BYTES)?;

    // 3. Write Version Header (8 bytes total)
    // Format: [Major: u32][Minor: u32] in Little Endian
    writer.write_all(&version.major.to_le_bytes())?;
    writer.write_all(&version.minor.to_le_bytes())?;

    // 4. Write Header (Tree Metadata)
    serialize_node_recursive(&mut writer, root)?;

    // 5. Write Data Section (File Contents)
    write_content_recursive(&mut writer, root, debug_verbose)?;

    // Ensure everything is flushed to disk
    writer.flush()?;
    Ok(())
}

/// Recursively traverses tree to set `content_offset_in_data_section`.
/// Also ensures `content_size` matches the actual data in memory.
fn calculate_offsets_recursive(node: &mut DirContextTreeNode, current_offset: &mut u64) {
    if node.node_type == NodeType::File {
        node.content_offset_in_data_section = *current_offset;

        // If content is loaded in memory, update size to be exact.
        if let Some(content) = &node.file_content {
            node.content_size = content.len() as u64;
        }

        *current_offset += node.content_size;
    } else {
        for child in &mut node.children {
            calculate_offsets_recursive(child, current_offset);
        }
    }
}

/// Serializes a single node and its children (Header format).
/// Uses Little Endian (LE) for portability.
fn serialize_node_recursive<W: Write>(writer: &mut W, node: &DirContextTreeNode) -> io::Result<()> {
    // 1. Node Type (u8)
    let type_byte = match node.node_type {
        NodeType::File => 0u8,
        NodeType::Directory => 1u8,
    };
    writer.write_all(&[type_byte])?;

    // 2. Relative Path Length (u16)
    let path_bytes = node.relative_path.as_bytes();
    if path_bytes.len() > u16::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Path too long: {}", node.relative_path),
        ));
    }
    writer.write_all(&(path_bytes.len() as u16).to_le_bytes())?;

    // 3. Relative Path (bytes)
    if !path_bytes.is_empty() {
        writer.write_all(path_bytes)?;
    }

    // 4. Last Modified Timestamp (u64)
    writer.write_all(&node.last_modified_timestamp.to_le_bytes())?;

    match node.node_type {
        NodeType::File => {
            // 5. Content Offset (u64)
            writer.write_all(&node.content_offset_in_data_section.to_le_bytes())?;
            // 6. Content Size (u64)
            writer.write_all(&node.content_size.to_le_bytes())?;
        }
        NodeType::Directory => {
            // 5. Number of Children (u32)
            if node.children.len() > u32::MAX as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too many children in directory",
                ));
            }
            writer.write_all(&(node.children.len() as u32).to_le_bytes())?;

            // Recursively write children
            for child in &node.children {
                serialize_node_recursive(writer, child)?;
            }
        }
    }

    Ok(())
}

/// Recursively writes file contents to the Data section.
fn write_content_recursive<W: Write>(
    writer: &mut W,
    node: &DirContextTreeNode,
    debug_verbose: bool,
) -> io::Result<()> {
    if node.node_type == NodeType::File {
        if let Some(content) = &node.file_content {
            writer.write_all(content)?;
        } else if node.content_size > 0 {
            // Phase 10.8.3: Suppress warning for intentionally omitted content
            // Only warn if this is an Include file that should have content but doesn't
            use crate::domain::FileHandlingDecision;
            let should_warn = matches!(node.handling_decision, FileHandlingDecision::Include);

            if should_warn && debug_verbose {
                // Actual problem: File should have content but doesn't (DEBUG only)
                eprintln!(
                    "Warning: Node '{}' has size {} but no content in memory. Writing zeros.",
                    node.relative_path, node.content_size
                );
            }
            // Write zeros to preserve offset integrity (for all cases)
            let zeros = vec![0u8; node.content_size as usize];
            writer.write_all(&zeros)?;
        }
    } else {
        for child in &node.children {
            write_content_recursive(writer, child, debug_verbose)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

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

    fn create_dir_node(path: &str) -> DirContextTreeNode {
        let mut node = DirContextTreeNode::new(
            NodeType::Directory,
            path,
            PathBuf::from(format!("/test/{}", path)),
        );
        node.last_modified_timestamp = 1234567890;
        node
    }

    // --- CALCULATE_OFFSETS TESTS ---

    #[test]
    fn test_calculate_offsets_single_file() {
        let mut node = create_file_node("test.txt", b"Hello");
        let mut offset = 0u64;
        calculate_offsets_recursive(&mut node, &mut offset);

        assert_eq!(node.content_offset_in_data_section, 0);
        assert_eq!(node.content_size, 5);
        assert_eq!(offset, 5);
    }

    #[test]
    fn test_calculate_offsets_multiple_files() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("a.txt", b"AAA"));
        root.children.push(create_file_node("b.txt", b"BBBBB"));

        let mut offset = 0u64;
        calculate_offsets_recursive(&mut root, &mut offset);

        assert_eq!(root.children[0].content_offset_in_data_section, 0);
        assert_eq!(root.children[0].content_size, 3);
        assert_eq!(root.children[1].content_offset_in_data_section, 3);
        assert_eq!(root.children[1].content_size, 5);
        assert_eq!(offset, 8);
    }

    #[test]
    fn test_calculate_offsets_nested() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("a.txt", b"AA"));

        let mut subdir = create_dir_node("sub");
        subdir.children.push(create_file_node("sub/b.txt", b"BBB"));
        root.children.push(subdir);

        let mut offset = 0u64;
        calculate_offsets_recursive(&mut root, &mut offset);

        assert_eq!(root.children[0].content_offset_in_data_section, 0);
        assert_eq!(
            root.children[1].children[0].content_offset_in_data_section,
            2
        );
        assert_eq!(offset, 5);
    }

    // --- SERIALIZE_NODE TESTS ---

    #[test]
    fn test_serialize_file_node() {
        let node = create_file_node("test.txt", b"Hello");
        let mut buffer = Vec::new();

        serialize_node_recursive(&mut buffer, &node).unwrap();

        // Check that buffer is not empty
        assert!(!buffer.is_empty());

        // First byte should be 0 (File type)
        assert_eq!(buffer[0], 0);
    }

    #[test]
    fn test_serialize_directory_node() {
        let node = create_dir_node("mydir");
        let mut buffer = Vec::new();

        serialize_node_recursive(&mut buffer, &node).unwrap();

        // First byte should be 1 (Directory type)
        assert_eq!(buffer[0], 1);
    }

    #[test]
    fn test_serialize_directory_with_children() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("a.txt", b"A"));
        root.children.push(create_file_node("b.txt", b"B"));

        let mut buffer = Vec::new();
        serialize_node_recursive(&mut buffer, &root).unwrap();

        // Should contain data for root + 2 children
        assert!(buffer.len() > 20);
    }

    // --- WRITE_CONTENT TESTS ---

    #[test]
    fn test_write_content_single_file() {
        let node = create_file_node("test.txt", b"Hello World");
        let mut buffer = Vec::new();

        write_content_recursive(&mut buffer, &node, false).unwrap();

        assert_eq!(buffer, b"Hello World");
    }

    #[test]
    fn test_write_content_multiple_files() {
        let mut root = create_dir_node("");
        root.children.push(create_file_node("a.txt", b"AAA"));
        root.children.push(create_file_node("b.txt", b"BBB"));

        let mut buffer = Vec::new();
        write_content_recursive(&mut buffer, &root, false).unwrap();

        assert_eq!(buffer, b"AAABBB");
    }

    #[test]
    fn test_write_content_empty_file() {
        let node = create_file_node("empty.txt", b"");
        let mut buffer = Vec::new();

        write_content_recursive(&mut buffer, &node, false).unwrap();

        assert!(buffer.is_empty());
    }

    // --- INTEGRATION TESTS ---

    #[test]
    fn test_write_dircontxt_file_creates_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("test.txt", b"Test content"));

        let version = Version::new_v1();
        write_dircontxt_file(path, &mut root, &version, false).unwrap();

        // File should exist and have content
        let metadata = std::fs::metadata(path).unwrap();
        assert!(metadata.len() > 0);
    }

    #[test]
    fn test_write_dircontxt_file_magic_bytes() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        let version = Version::new_v1();
        write_dircontxt_file(path, &mut root, &version, false).unwrap();

        // Read first 8 bytes and verify magic
        let content = std::fs::read(path).unwrap();
        assert!(content.len() >= 8);
        assert_eq!(&content[0..8], DIRCONTXT_MAGIC_BYTES);
    }

    #[test]
    fn test_write_dircontxt_file_version_header() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        let version = Version { major: 2, minor: 5 };
        write_dircontxt_file(path, &mut root, &version, false).unwrap();

        let content = std::fs::read(path).unwrap();
        // Bytes 8-12: major (u32 LE), bytes 12-16: minor (u32 LE)
        let major = u32::from_le_bytes([content[8], content[9], content[10], content[11]]);
        let minor = u32::from_le_bytes([content[12], content[13], content[14], content[15]]);

        assert_eq!(major, 2);
        assert_eq!(minor, 5);
    }

    #[test]
    fn test_write_dircontxt_file_with_content() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        root.children
            .push(create_file_node("hello.txt", b"Hello, World!"));

        let version = Version::new_v1();
        write_dircontxt_file(path, &mut root, &version, false).unwrap();

        let content = std::fs::read(path).unwrap();
        // Content should be somewhere in the file
        let content_str = String::from_utf8_lossy(&content);
        assert!(content_str.contains("Hello, World!"));
    }
}
