use crate::config::DIRCONTXT_MAGIC_BYTES;
use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::version::Version;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

/// Attempts to load and parse a .dctx binary file.
///
/// Returns:
/// - `Ok((root_node, version))`: Successfully parsed tree and version.
/// - `Err`: If file missing, corrupted, wrong signature, or invalid format.
pub fn read_dircontxt_file(file_path: &Path) -> io::Result<(DirContextTreeNode, Version)> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    // 1. Verify Signature (Magic Bytes)
    let mut magic_buffer = [0u8; 8];
    reader.read_exact(&mut magic_buffer)?;

    if &magic_buffer != DIRCONTXT_MAGIC_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Invalid file signature. Expected {:?}, got {:?}",
                DIRCONTXT_MAGIC_BYTES, magic_buffer
            ),
        ));
    }

    // 2. Read Version Header (NEW)
    // [Major: u32][Minor: u32]
    let mut major_buf = [0u8; 4];
    reader.read_exact(&mut major_buf)?;
    let major = u32::from_le_bytes(major_buf);

    let mut minor_buf = [0u8; 4];
    reader.read_exact(&mut minor_buf)?;
    let minor = u32::from_le_bytes(minor_buf);

    let version = Version { major, minor };

    // 3. Deserialize Tree (Header section)
    // Paths in tree are relative. Root node will not have disk_path set here.
    let root = deserialize_node_recursive(&mut reader)?;

    Ok((root, version))
}

/// Recursively reads nodes from the stream.
fn deserialize_node_recursive<R: Read>(reader: &mut R) -> io::Result<DirContextTreeNode> {
    // 1. Node Type (u8)
    let mut type_buf = [0u8; 1];
    reader.read_exact(&mut type_buf)?;
    let node_type = match type_buf[0] {
        0 => NodeType::File,
        1 => NodeType::Directory,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown node type",
            ))
        }
    };

    // 2. Relative Path Length (u16)
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf)?;
    let path_len = u16::from_le_bytes(len_buf) as usize;

    // 3. Relative Path (bytes)
    let mut path_buf = vec![0u8; path_len];
    reader.read_exact(&mut path_buf)?;
    let relative_path = String::from_utf8(path_buf).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid UTF-8 path: {}", e),
        )
    })?;

    // 4. Last Modified Timestamp (u64)
    let mut ts_buf = [0u8; 8];
    reader.read_exact(&mut ts_buf)?;
    let last_modified = u64::from_le_bytes(ts_buf);

    // Create the node structure
    let mut node = DirContextTreeNode::new(
        node_type,
        &relative_path,
        PathBuf::new(), // Disk path is not stored in binary, it is relative
    );
    node.last_modified_timestamp = last_modified;

    match node_type {
        NodeType::File => {
            // 5. Content Offset (u64)
            let mut off_buf = [0u8; 8];
            reader.read_exact(&mut off_buf)?;
            node.content_offset_in_data_section = u64::from_le_bytes(off_buf);

            // 6. Content Size (u64)
            let mut size_buf = [0u8; 8];
            reader.read_exact(&mut size_buf)?;
            node.content_size = u64::from_le_bytes(size_buf);
        }
        NodeType::Directory => {
            // 5. Number of Children (u32)
            let mut child_count_buf = [0u8; 4];
            reader.read_exact(&mut child_count_buf)?;
            let child_count = u32::from_le_bytes(child_count_buf);

            // Recursively read children
            for _ in 0..child_count {
                let child = deserialize_node_recursive(reader)?;
                node.children.push(child);
            }
        }
    }

    Ok(node)
}
