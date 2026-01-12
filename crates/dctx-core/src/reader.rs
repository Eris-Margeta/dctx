use crate::config::DIRCONTXT_MAGIC_BYTES;
use crate::datatypes::{DirContextTreeNode, NodeType};
use crate::version::Version; // Added import
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

/// Reads binary file and returns (RootNode, Version)
pub fn read_dircontxt_file(file_path: &Path) -> io::Result<(DirContextTreeNode, Version)> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::new(file);

    // 1. Signature
    let mut magic_buffer = [0u8; 8];
    reader.read_exact(&mut magic_buffer)?;
    if &magic_buffer != DIRCONTXT_MAGIC_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid signature",
        ));
    }

    // 2. Version Header (NEW)
    let mut major_buf = [0u8; 4];
    reader.read_exact(&mut major_buf)?;
    let major = u32::from_le_bytes(major_buf);

    let mut minor_buf = [0u8; 4];
    reader.read_exact(&mut minor_buf)?;
    let minor = u32::from_le_bytes(minor_buf);

    let version = Version { major, minor };

    // 3. Tree Metadata
    let root = deserialize_node_recursive(&mut reader)?;

    Ok((root, version))
}

fn deserialize_node_recursive<R: Read>(reader: &mut R) -> io::Result<DirContextTreeNode> {
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

    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf)?;
    let path_len = u16::from_le_bytes(len_buf) as usize;

    let mut path_buf = vec![0u8; path_len];
    reader.read_exact(&mut path_buf)?;
    let relative_path = String::from_utf8(path_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Invalid UTF-8: {}", e)))?;

    let mut ts_buf = [0u8; 8];
    reader.read_exact(&mut ts_buf)?;
    let last_modified = u64::from_le_bytes(ts_buf);

    let mut node = DirContextTreeNode::new(node_type, &relative_path, PathBuf::new());
    node.last_modified_timestamp = last_modified;

    match node_type {
        NodeType::File => {
            let mut off_buf = [0u8; 8];
            reader.read_exact(&mut off_buf)?;
            node.content_offset_in_data_section = u64::from_le_bytes(off_buf);

            let mut size_buf = [0u8; 8];
            reader.read_exact(&mut size_buf)?;
            node.content_size = u64::from_le_bytes(size_buf);
        }
        NodeType::Directory => {
            let mut child_count_buf = [0u8; 4];
            reader.read_exact(&mut child_count_buf)?;
            let child_count = u32::from_le_bytes(child_count_buf);

            for _ in 0..child_count {
                let child = deserialize_node_recursive(reader)?;
                node.children.push(child);
            }
        }
    }
    Ok(node)
}
