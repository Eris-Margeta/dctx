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
    write_content_recursive(&mut writer, root)?;

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
fn write_content_recursive<W: Write>(writer: &mut W, node: &DirContextTreeNode) -> io::Result<()> {
    if node.node_type == NodeType::File {
        if let Some(content) = &node.file_content {
            writer.write_all(content)?;
        } else if node.content_size > 0 {
            // Fallback: If size > 0 but no content in memory (shouldn't happen in standard flow),
            // write zeros to preserve offset integrity.
            eprintln!(
                "Warning: Node '{}' has size {} but no content in memory. Writing zeros.",
                node.relative_path, node.content_size
            );
            let zeros = vec![0u8; node.content_size as usize];
            writer.write_all(&zeros)?;
        }
    } else {
        for child in &node.children {
            write_content_recursive(writer, child)?;
        }
    }
    Ok(())
}
