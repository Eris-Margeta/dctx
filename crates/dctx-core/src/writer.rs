use crate::config::DIRCONTXT_MAGIC_BYTES;
use crate::datatypes::{DirContextTreeNode, NodeType};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

/// Glavna funkcija za pisanje .dircontxt binarne datoteke.
///
/// Proces:
/// 1. Ažurira offsete u stablu (priprema).
/// 2. Piše potpis (Signature).
/// 3. Piše zaglavlje (Header - serijalizirano stablo).
/// 4. Piše podatke (Data - sadržaj datoteka).
pub fn write_dircontxt_file(output_path: &Path, root: &mut DirContextTreeNode) -> io::Result<()> {
    let file = File::create(output_path)?;
    // Koristimo BufWriter da smanjimo broj syscall-ova kod pisanja malih metapodataka
    let mut writer = BufWriter::new(file);

    // 1. Priprema: Izračunaj offsete za Data sekciju
    // Moramo znati gdje svaki fajl počinje unutar Data sekcije prije nego zapišemo Header.
    let mut current_offset = 0u64;
    calculate_offsets_recursive(root, &mut current_offset);

    // 2. Pisanje Potpisa (8 bytes)
    writer.write_all(DIRCONTXT_MAGIC_BYTES)?;

    // 3. Pisanje Headera (Tree Metadata)
    serialize_node_recursive(&mut writer, root)?;

    // 4. Pisanje Data sekcije (File Contents)
    write_content_recursive(&mut writer, root)?;

    // Osiguraj da je sve zapisano na disk
    writer.flush()?;
    Ok(())
}

/// Rekurzivno prolazi stablo i postavlja `content_offset_in_data_section`.
/// Također osigurava da je `content_size` točan prema onome što imamo u memoriji.
fn calculate_offsets_recursive(node: &mut DirContextTreeNode, current_offset: &mut u64) {
    if node.node_type == NodeType::File {
        node.content_offset_in_data_section = *current_offset;
        
        // Ako imamo učitan sadržaj, veličina je duljina vektora.
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

/// Serijalizira jedan čvor i njegovu djecu (Header format).
/// Koristi Little Endian (LE) zapis brojeva radi portabilnosti.
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
            
            // Rekurzivno piši djecu
            for child in &node.children {
                serialize_node_recursive(writer, child)?;
            }
        }
    }

    Ok(())
}

/// Rekurzivno piše sadržaj datoteka u Data sekciju.
fn write_content_recursive<W: Write>(writer: &mut W, node: &DirContextTreeNode) -> io::Result<()> {
    if node.node_type == NodeType::File {
        if let Some(content) = &node.file_content {
            writer.write_all(content)?;
        } else if node.content_size > 0 {
            // Fallback: Ako piše da file ima veličinu, a nemamo sadržaj u memoriji,
            // pišemo nule da ne poremetimo offsete. Ovo se ne bi trebalo događati
            // ako je walker ispravno učitao podatke.
            eprintln!("Warning: Node '{}' has size {} but no content in memory. Writing zeros.", 
                node.relative_path, node.content_size);
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
