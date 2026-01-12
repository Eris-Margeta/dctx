use crate::config::DIRCONTXT_MAGIC_BYTES;
use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::version::Version; // Added import
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub fn write_dircontxt_file(
    output_path: &Path,
    root: &mut DirContextTreeNode,
    version: &Version, // Added argument
) -> io::Result<()> {
    let file = File::create(output_path)?;
    let mut writer = BufWriter::new(file);

    // 1. Priprema: Offsets
    let mut current_offset = 0u64;
    calculate_offsets_recursive(root, &mut current_offset);

    // 2. Signature (8 bytes)
    writer.write_all(DIRCONTXT_MAGIC_BYTES)?;

    // 3. Version Header (NEW)
    // We write: [Major: u32][Minor: u32] (Little Endian)
    writer.write_all(&version.major.to_le_bytes())?;
    writer.write_all(&version.minor.to_le_bytes())?;

    // 4. Tree Metadata
    serialize_node_recursive(&mut writer, root)?;

    // 5. Content Data
    write_content_recursive(&mut writer, root)?;

    writer.flush()?;
    Ok(())
}

fn calculate_offsets_recursive(node: &mut DirContextTreeNode, current_offset: &mut u64) {
    if node.node_type == NodeType::File {
        node.content_offset_in_data_section = *current_offset;
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

fn serialize_node_recursive<W: Write>(writer: &mut W, node: &DirContextTreeNode) -> io::Result<()> {
    // Node Type (u8)
    let type_byte = match node.node_type {
        NodeType::File => 0u8,
        NodeType::Directory => 1u8,
    };
    writer.write_all(&[type_byte])?;

    // Path Length (u16)
    let path_bytes = node.relative_path.as_bytes();
    if path_bytes.len() > u16::MAX as usize {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path too long"));
    }
    writer.write_all(&(path_bytes.len() as u16).to_le_bytes())?;

    // Path
    if !path_bytes.is_empty() {
        writer.write_all(path_bytes)?;
    }

    // Timestamp (u64)
    writer.write_all(&node.last_modified_timestamp.to_le_bytes())?;

    match node.node_type {
        NodeType::File => {
            writer.write_all(&node.content_offset_in_data_section.to_le_bytes())?;
            writer.write_all(&node.content_size.to_le_bytes())?;
        }
        NodeType::Directory => {
            if node.children.len() > u32::MAX as usize {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Too many children",
                ));
            }
            writer.write_all(&(node.children.len() as u32).to_le_bytes())?;
            for child in &node.children {
                serialize_node_recursive(writer, child)?;
            }
        }
    }
    Ok(())
}

fn write_content_recursive<W: Write>(writer: &mut W, node: &DirContextTreeNode) -> io::Result<()> {
    if node.node_type == NodeType::File {
        if let Some(content) = &node.file_content {
            writer.write_all(content)?;
        } else if node.content_size > 0 {
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
