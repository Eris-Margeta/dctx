use crate::config;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

// --- CONSTANTS ---
const DB_SEPARATOR: &str = "|";

// --- STRUCTS ---

/// Handles the mapping between project paths and their unique Hash IDs.
/// Corresponds to ADR-001 Section 2.
pub struct ProjectRegistry {
    /// Path to the registry file (e.g., ~/.config/dctx/projects.db)
    registry_path: PathBuf,
    /// In-memory cache of the registry (Path -> HashID)
    /// Key: Absolute Path string, Value: Hash ID string
    entries: HashMap<String, String>,
}

impl ProjectRegistry {
    /// Initializes the registry.
    /// Ensures the config directory exists.
    /// Loads existing entries from disk.
    pub fn new() -> io::Result<Self> {
        let registry_path = config::get_project_registry_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Could not determine registry path (HOME/APPDATA missing)",
            )
        })?;

        // Ensure parent directory exists
        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut registry = Self {
            registry_path,
            entries: HashMap::new(),
        };

        registry.load()?;
        Ok(registry)
    }

    /// Loads the registry database from disk.
    /// Format: HASH_ID | ABSOLUTE_PATH
    fn load(&mut self) -> io::Result<()> {
        if !self.registry_path.exists() {
            return Ok(());
        }

        let file = fs::File::open(&self.registry_path)?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some((hash, path)) = trimmed.split_once(DB_SEPARATOR) {
                let hash = hash.trim().to_string();
                let path = path.trim().to_string();
                // Store mapped by Path for lookup
                self.entries.insert(path, hash);
            }
        }
        Ok(())
    }

    /// Gets the existing Hash ID for a project path, or creates a new one if it doesn't exist.
    /// Returns the Hash ID.
    pub fn get_or_create_id(&mut self, project_abs_path: &Path) -> io::Result<String> {
        let path_str = project_abs_path.to_string_lossy().to_string();

        // 1. Check in-memory cache
        if let Some(hash) = self.entries.get(&path_str) {
            return Ok(hash.clone());
        }

        // 2. Generate new Hash ID (FNV-1a)
        let new_hash = generate_fnv1a_hash(&path_str);

        // 3. Append to file
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.registry_path)?;

        writeln!(file, "{} {} {}", new_hash, DB_SEPARATOR, path_str)?;

        // 4. Update memory
        self.entries.insert(path_str, new_hash.clone());

        Ok(new_hash)
    }
}

// --- HASHING UTILS (Zero Dep) ---

/// Generates a deterministic 64-bit FNV-1a hash formatted as a hex string.
/// Used to create unique, filesystem-safe identifiers for project paths.
fn generate_fnv1a_hash(text: &str) -> String {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET_BASIS;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }

    format!("{:016x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FNV1A HASH TESTS ---

    #[test]
    fn test_fnv1a_stability() {
        let input = "/home/user/projects/my-app";
        let hash1 = generate_fnv1a_hash(input);
        let hash2 = generate_fnv1a_hash(input);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
    }

    #[test]
    fn test_fnv1a_different_inputs() {
        let hash1 = generate_fnv1a_hash("/path/one");
        let hash2 = generate_fnv1a_hash("/path/two");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_fnv1a_empty_string() {
        let hash = generate_fnv1a_hash("");
        assert_eq!(hash.len(), 16);
        // FNV-1a of empty string is the offset basis
        assert_eq!(hash, "cbf29ce484222325");
    }

    #[test]
    fn test_fnv1a_single_char() {
        let hash = generate_fnv1a_hash("a");
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn test_fnv1a_long_path() {
        let long_path = "/very/long/path/that/goes/deep/into/the/filesystem/structure/project";
        let hash = generate_fnv1a_hash(long_path);
        assert_eq!(hash.len(), 16);
        // Should still be stable
        assert_eq!(hash, generate_fnv1a_hash(long_path));
    }

    #[test]
    fn test_fnv1a_special_characters() {
        let path_with_spaces = "/path/with spaces/and-dashes/and_underscores";
        let hash = generate_fnv1a_hash(path_with_spaces);
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn test_fnv1a_unicode() {
        let unicode_path = "/путь/к/проекту"; // Russian characters
        let hash = generate_fnv1a_hash(unicode_path);
        assert_eq!(hash.len(), 16);
    }

    #[test]
    fn test_fnv1a_case_sensitive() {
        let hash1 = generate_fnv1a_hash("/Path/To/Project");
        let hash2 = generate_fnv1a_hash("/path/to/project");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_fnv1a_similar_paths() {
        // Paths that are very similar should still produce different hashes
        let hash1 = generate_fnv1a_hash("/project/a");
        let hash2 = generate_fnv1a_hash("/project/b");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_fnv1a_hex_format() {
        let hash = generate_fnv1a_hash("test");
        // Should be valid hexadecimal
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // --- DB_SEPARATOR TESTS ---

    #[test]
    fn test_db_separator_is_pipe() {
        assert_eq!(DB_SEPARATOR, "|");
    }
}
