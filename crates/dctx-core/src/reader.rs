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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::write_dircontxt_file;
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

    // --- INVALID FILE TESTS ---

    #[test]
    fn test_read_nonexistent_file() {
        let result = read_dircontxt_file(Path::new("/nonexistent/file.dctx"));
        assert!(result.is_err());
    }

    #[test]
    fn test_read_invalid_magic_bytes() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), b"INVALID_").unwrap();

        let result = read_dircontxt_file(temp_file.path());
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_read_truncated_file() {
        let temp_file = NamedTempFile::new().unwrap();
        // Write only magic bytes, no version header
        std::fs::write(temp_file.path(), DIRCONTXT_MAGIC_BYTES).unwrap();

        let result = read_dircontxt_file(temp_file.path());
        assert!(result.is_err());
    }

    // --- ROUNDTRIP TESTS (Write -> Read) ---

    #[test]
    fn test_roundtrip_empty_directory() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        let version = Version::new_v1();
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, loaded_version) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.node_type, NodeType::Directory);
        assert_eq!(loaded.relative_path, "");
        assert!(loaded.children.is_empty());
        assert_eq!(loaded_version, version);
    }

    #[test]
    fn test_roundtrip_single_file() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        original.children.push(create_file_node("test.txt", b"Content"));
        let version = Version { major: 1, minor: 5 };
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, loaded_version) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.children.len(), 1);
        assert_eq!(loaded.children[0].relative_path, "test.txt");
        assert_eq!(loaded.children[0].content_size, 7);
        assert_eq!(loaded_version, version);
    }

    #[test]
    fn test_roundtrip_multiple_files() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        original.children.push(create_file_node("a.txt", b"AAA"));
        original.children.push(create_file_node("b.txt", b"BBBBB"));
        original.children.push(create_file_node("c.txt", b"C"));
        let version = Version::new_v1();
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, _) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.children.len(), 3);
        assert_eq!(loaded.children[0].content_size, 3);
        assert_eq!(loaded.children[1].content_size, 5);
        assert_eq!(loaded.children[2].content_size, 1);
    }

    #[test]
    fn test_roundtrip_nested_directories() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        original.children.push(create_file_node("root.txt", b"Root"));

        let mut subdir = create_dir_node("src");
        subdir.children.push(create_file_node("src/main.rs", b"fn main() {}"));
        subdir.children.push(create_file_node("src/lib.rs", b"// lib"));
        original.children.push(subdir);

        let version = Version { major: 2, minor: 0 };
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, loaded_version) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.children.len(), 2);
        assert_eq!(loaded.children[1].relative_path, "src");
        assert_eq!(loaded.children[1].children.len(), 2);
        assert_eq!(loaded.children[1].children[0].relative_path, "src/main.rs");
        assert_eq!(loaded_version, version);
    }

    #[test]
    fn test_roundtrip_preserves_timestamp() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        let mut file = create_file_node("test.txt", b"Data");
        file.last_modified_timestamp = 9876543210;
        original.children.push(file);

        let version = Version::new_v1();
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, _) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.children[0].last_modified_timestamp, 9876543210);
    }

    #[test]
    fn test_roundtrip_preserves_offsets() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write
        let mut original = create_dir_node("");
        original.children.push(create_file_node("a.txt", b"AAAA")); // 4 bytes
        original.children.push(create_file_node("b.txt", b"BBBBBB")); // 6 bytes

        let version = Version::new_v1();
        write_dircontxt_file(path, &mut original, &version).unwrap();

        // Read
        let (loaded, _) = read_dircontxt_file(path).unwrap();

        assert_eq!(loaded.children[0].content_offset_in_data_section, 0);
        assert_eq!(loaded.children[1].content_offset_in_data_section, 4);
    }

    #[test]
    fn test_roundtrip_deep_nesting() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create deeply nested structure: root/a/b/c/file.txt
        let mut root = create_dir_node("");
        let mut a = create_dir_node("a");
        let mut b = create_dir_node("a/b");
        let mut c = create_dir_node("a/b/c");
        c.children.push(create_file_node("a/b/c/file.txt", b"Deep content"));
        b.children.push(c);
        a.children.push(b);
        root.children.push(a);

        let version = Version::new_v1();
        write_dircontxt_file(path, &mut root, &version).unwrap();

        // Read
        let (loaded, _) = read_dircontxt_file(path).unwrap();

        // Navigate to the deep file
        let file = &loaded.children[0].children[0].children[0].children[0];
        assert_eq!(file.relative_path, "a/b/c/file.txt");
        assert_eq!(file.content_size, 12);
    }

    #[test]
    fn test_roundtrip_version_v1() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        let version = Version::new_v1();
        write_dircontxt_file(path, &mut root, &version).unwrap();

        let (_, loaded_version) = read_dircontxt_file(path).unwrap();
        assert_eq!(loaded_version.major, 1);
        assert_eq!(loaded_version.minor, 0);
    }

    #[test]
    fn test_roundtrip_version_with_minor() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut root = create_dir_node("");
        let version = Version { major: 3, minor: 42 };
        write_dircontxt_file(path, &mut root, &version).unwrap();

        let (_, loaded_version) = read_dircontxt_file(path).unwrap();
        assert_eq!(loaded_version.major, 3);
        assert_eq!(loaded_version.minor, 42);
    }
}
