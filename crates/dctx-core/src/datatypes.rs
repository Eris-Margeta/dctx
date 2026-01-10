use std::path::PathBuf;

/// Ekvivalent za NodeType iz C-a
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    File,
    Directory,
}

/// Ekvivalent za PatternType iz C-a
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternType {
    Invalid,
    Basename, // Matches only the file/dir name (e.g., "node_modules")
    Path,     // Matches the full relative path (e.g., "src/main.c")
    Suffix,   // Matches a suffix wildcard (e.g., "*.log")
    Prefix,   // Matches a prefix wildcard (e.g., "build/*")
    
    // NOVO: Za složene uzorke
    Glob,     // Matches complex patterns ("foo/**/bar", "config-*-prod.json")
}

/// Ekvivalent za IgnoreRule iz C-a
#[derive(Debug, Clone)]
pub struct IgnoreRule {
    pub pattern: String,
    pub pattern_type: PatternType,
    pub is_dir_only: bool,
    pub is_negation: bool, // Set to true if the pattern starts with '!'
}

impl IgnoreRule {
    /// Konstruktor za stvaranje praznog pravila
    pub fn new() -> Self {
        Self {
            pattern: String::new(),
            pattern_type: PatternType::Invalid,
            is_dir_only: false,
            is_negation: false,
        }
    }
}

/// Ekvivalent za DirContextTreeNode iz C-a
#[derive(Debug, Clone)]
pub struct DirContextTreeNode {
    pub node_type: NodeType,

    /// Relativna putanja unutar arhive (npr. "src/main.rs")
    pub relative_path: String,

    /// Unix timestamp zadnje modifikacije
    pub last_modified_timestamp: u64,

    // --- Za datoteke ---
    /// Offset gdje počinje sadržaj u binarnoj datoteci
    pub content_offset_in_data_section: u64,

    /// Veličina sadržaja datoteke
    pub content_size: u64,

    /// Apsolutna putanja na disku
    pub disk_path: PathBuf,

    // --- Za direktorije ---
    /// Djeca ovog čvora.
    pub children: Vec<DirContextTreeNode>,

    // --- Za LLM Formatter ---
    /// Generirani ID (npr. "F001", "D002")
    pub generated_id_for_llm: String,

    // --- Cache za sadržaj ---
    pub file_content: Option<Vec<u8>>,
}

impl DirContextTreeNode {
    pub fn new(node_type: NodeType, relative_path: &str, disk_path: PathBuf) -> Self {
        Self {
            node_type,
            relative_path: relative_path.to_string(),
            last_modified_timestamp: 0,
            content_offset_in_data_section: 0,
            content_size: 0,
            disk_path,
            children: Vec::new(),
            generated_id_for_llm: String::new(),
            file_content: None,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.node_type == NodeType::Directory
    }
}
