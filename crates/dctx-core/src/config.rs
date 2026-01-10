use std::env;
use std::path::PathBuf;

// --- KONSTANTE ---

/// Magic Bytes koji identificiraju naš format (8 bytes).
pub const DIRCONTXT_MAGIC_BYTES: &[u8; 8] = b"DIRCTXTV";

/// Ekstenzija binarne datoteke (uvijek .dctx).
pub const BINARY_FILE_EXTENSION: &str = "dctx";

/// Ekstenzija tekstualnog konteksta za LLM.
pub const LLM_CONTEXT_EXTENSION: &str = "llmcontext.txt";

/// Ime datoteke za ignoriranje (lokalno u projektu).
pub const DEFAULT_IGNORE_FILENAME: &str = ".dctxignore";

/// Ime mape aplikacije (za config i cache direktorije na sustavu).
pub const APP_NAME: &str = "dctx";

/// Ime datoteke globalnih pravila unutar config direktorija.
pub const GLOBAL_IGNORE_FILENAME: &str = "ignore";

/// Ime datoteke koja služi kao baza podataka (registry) projekata.
pub const REGISTRY_FILENAME: &str = "projects.db";

// --- LAYER 1: HARDCODED RULES ---
// Ova pravila su najjača "baza". Mogu se poništiti (negirati) u Layer 2 ili 3.
pub const HARDCODED_IGNORE_RULES: &[&str] = &[
    // Version Control
    ".git/",
    ".svn/",
    ".hg/",
    
    // Dependencies
    "node_modules/",
    "bower_components/",
    
    // System & IDE
    ".DS_Store",
    "Thumbs.db",
    
    // Build Output (Generic)
    "dist/",
    "build/",
    "target/", // Rust specific
    "bin/",
    "obj/",
    
    // Python
    ".venv/",
    "venv/",
    "__pycache__/",
    "*.pyc",
    
    // Self-referential (ne želimo da alat skenira svoje interne fajlove ako su u folderu)
    ".dctxignore", // Ignoriramo ignore file
    "*.dctx",      // Ignoriramo binarne fajlove (ako se slučajno nađu lokalno)
];

// --- LOGIKA ZA PUTANJE (System Paths) ---

pub enum StorageStrategy {
    Local,      // .dctx unutar projekta (Legacy/Optional)
    SystemCache // ~/.cache/dctx/HASH.dctx (Default/Recommended)
}

/// Vraća putanju do konfiguracijskog direktorija ovisno o OS-u.
/// Linux/Mac: ~/.config/dctx
/// Windows:   %APPDATA%\dctx
pub fn get_config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var("APPDATA").ok().map(|p| PathBuf::from(p).join(APP_NAME))
    } else {
        // Unix-like (Linux, macOS) - XDG standard
        // Prvo probaj XDG_CONFIG_HOME, ako nema, koristi HOME/.config
        if let Ok(xdg_config) = env::var("XDG_CONFIG_HOME") {
            Some(PathBuf::from(xdg_config).join(APP_NAME))
        } else {
            env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config").join(APP_NAME))
        }
    }
}

/// Vraća putanju do cache direktorija ovisno o OS-u.
/// Linux: ~/.cache/dctx
/// Mac:   ~/Library/Caches/dctx
/// Windows: %LOCALAPPDATA%\dctx
pub fn get_cache_dir() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env::var("LOCALAPPDATA").ok().map(|p| PathBuf::from(p).join(APP_NAME))
    } else if cfg!(target_os = "macos") {
        env::var("HOME").ok().map(|h| PathBuf::from(h).join("Library").join("Caches").join(APP_NAME))
    } else {
        // Linux / ostali Unixi
        if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
            Some(PathBuf::from(xdg_cache).join(APP_NAME))
        } else {
            env::var("HOME").ok().map(|h| PathBuf::from(h).join(".cache").join(APP_NAME))
        }
    }
}

/// Vraća punu putanju do globalnog ignore fajla.
pub fn get_global_ignore_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(GLOBAL_IGNORE_FILENAME))
}

/// Vraća punu putanju do registry baze (popis projekata).
pub fn get_project_registry_path() -> Option<PathBuf> {
    get_config_dir().map(|p| p.join(REGISTRY_FILENAME))
}
