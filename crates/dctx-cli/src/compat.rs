//! Host IO for the legacy alias, per D3-COMPATIBILITY and D3-LEGACY-REMOVAL §5 D3-04.
//! Owns directory discovery, FNV/projects.db mapping, DIRCTXTV state and export retention.
//! Uses pure core settings/selection and the pack codec; rule reads use the filesystem adapter.
//! Resets select V1 without deleting existing exports. Only explicit clipboard requests spawn.

use dctx_adapter_fs::{SourceFs, StdFs};
use dctx_core::{
    datatypes::DirContextTreeNode,
    selection::{RuleTexts, SelectionPolicy},
    settings,
    version::Version,
};
use dctx_pack::{decode_legacy, encode_legacy, LegacyBodies, LegacyLimits};
use std::{
    env, fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Legacy directories are separate from the v2 managed-cache root.
pub fn cache_dir() -> Option<PathBuf> {
    directory("LOCALAPPDATA", "XDG_CACHE_HOME", ".cache")
}

fn config_dir() -> Option<PathBuf> {
    directory("APPDATA", "XDG_CONFIG_HOME", ".config")
}

fn directory(windows: &str, xdg: &str, fallback: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        env::var(windows).ok().map(PathBuf::from)
    } else {
        env::var(xdg)
            .ok()
            .map(PathBuf::from)
            .or_else(|| env::var("HOME").ok().map(|home| PathBuf::from(home).join(fallback)))
    };
    base.map(|path| path.join(settings::APP_NAME))
}

/// Missing or unreadable config retains the historic defaults.
pub fn app_config() -> settings::AppConfig {
    config_dir()
        .and_then(|dir| fs::read_to_string(dir.join(settings::CONFIG_FILENAME)).ok())
        .map(|text| settings::parse_config(&text))
        .unwrap_or_default()
}

/// Stable UTF-8 FNV-1a id, including the legacy 16-digit lower-case representation.
pub fn path_id(path: &str) -> String {
    let hash =
        path.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

/// Existing registry rows win over hashing; the final row for a path wins.
pub fn project_id(cache: &Path, path: &Path) -> io::Result<String> {
    fs::create_dir_all(cache)?;
    let registry = cache.join("projects.db");
    let text = match fs::read_to_string(&registry) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    let path = path.to_string_lossy();
    let existing = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('|'))
        .rfind(|(_, stored)| stored.trim() == path);
    if let Some((id, _)) = existing {
        return Ok(id.trim().to_string());
    }
    let id = path_id(&path);
    let mut file = fs::OpenOptions::new().create(true).append(true).open(registry)?;
    writeln!(file, "{id} | {path}")?;
    Ok(id)
}

fn read_state(path: &Path) -> Result<(DirContextTreeNode, Version), String> {
    let limits = LegacyLimits::default();
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(limits.max_total_bytes as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    let document = decode_legacy(&bytes, LegacyBodies::MetadataOnly, limits).map_err(|error| error.to_string())?;
    Ok((document.root, document.header.version))
}

/// Metadata-only history preserves the old reader and folder-recreation heuristic.
pub fn load_state(root: &Path, binary: &Path, reset: bool) -> Option<(DirContextTreeNode, Version)> {
    if reset {
        println!("🔄 Force Reset enabled.");
        return None;
    }
    if !binary.exists() {
        println!("ℹ️ No existing binary state found (Fresh Start).");
        return None;
    }
    println!("📝 Found existing binary state at {binary:?}");
    match read_state(binary) {
        Ok((tree, version)) => {
            // The legacy heuristic compares disk seconds to stored timestamps verbatim.
            let disk_seconds = fs::metadata(root)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|time| time.as_secs());
            if disk_seconds.is_some_and(|seconds| seconds.saturating_sub(tree.last_modified_timestamp) > 1) {
                eprintln!("[INFO] Detected recreated target folder. Resetting state to V1.");
                return None;
            }
            println!("✅ State Loaded: {version}");
            Some((tree, version))
        }
        Err(error) => {
            eprintln!("[WARN] Corrupted or invalid state file detected: {error}");
            println!("⚠️ State load resulted in FreshV1 (Reset)");
            None
        }
    }
}

/// TUI history uses the same legacy registry and binary codec as the alias.
pub fn last_version(path: &Path) -> Option<String> {
    let root = fs::canonicalize(path).ok()?;
    let cache = cache_dir()?;
    let id = project_id(&cache, &root).ok()?;
    read_state(&cache.join(format!("{id}.dctx"))).ok().map(|(_, version)| version.to_string())
}

/// Encode before touching the previous binary, so codec refusal leaves it intact.
pub fn save_state(path: &Path, tree: &DirContextTreeNode, version: &Version) -> Result<(), String> {
    let bytes = encode_legacy(tree, version, LegacyLimits::default()).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn rule(path: &Path) -> Result<Option<String>, String> {
    StdFs.read_policy(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Preserve the alias's three scan layers; local omit rules historically affect rendering only.
pub fn scan_policy(root: &Path, verbose: bool) -> Result<SelectionPolicy, String> {
    let global_ignore = config_dir().map(|dir| rule(&dir.join("ignore"))).transpose()?.flatten();
    let global_omit = config_dir().map(|dir| rule(&dir.join("omitcontent"))).transpose()?.flatten();
    let local_ignore = rule(&root.join(".dctxignore"))?;
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        global_ignore: global_ignore.as_deref(),
        global_omit: global_omit.as_deref(),
        local_ignore: local_ignore.as_deref(),
        local_omit: None,
    });
    let ignore = policy.ignore_layers();
    let omit = policy.omit_layers();
    eprintln!("🔧 Loaded {} ignore + {} omit rules (from 3 sources)", ignore.total(), omit.total());
    if verbose {
        for (name, counts) in [("IGNORE", ignore), ("OMIT", omit)] {
            eprintln!(
                "   {name} RULES:\n     • Hardcoded: {}\n     • Global: {}\n     • Local: {}",
                counts.hardcoded, counts.global, counts.local
            );
        }
    }
    Ok(policy)
}

/// Rendering skipped unreadable omit files in the legacy host.
pub fn output_policy(root: &Path) -> SelectionPolicy {
    let global = config_dir().and_then(|dir| rule(&dir.join("omitcontent")).ok().flatten());
    let local = rule(&root.join(".dctxomitcontent")).ok().flatten();
    SelectionPolicy::from_texts(&RuleTexts {
        global_omit: global.as_deref(),
        local_omit: local.as_deref(),
        ..RuleTexts::default()
    })
}

/// Write one named export. No reset, version bump or corruption path sweeps old exports.
pub fn export(path: &Path, bytes: &[u8], diff: bool) {
    match fs::write(path, bytes) {
        Ok(()) => println!(
            "Legacy {} wrote {} ({} bytes, retention=legacy-project-text).",
            if diff { "diff" } else { "compatibility alias" },
            path.display(),
            bytes.len()
        ),
        Err(error) => eprintln!("❌ Write error for {}: {error}", path.display()),
    }
}

/// Resolve clipboard tools without launching a command-discovery child.
pub fn copy_to_clipboard(bytes: &[u8]) -> Result<(), String> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &["pbcopy"]
    } else if cfg!(windows) {
        &["clip.exe"]
    } else if cfg!(target_os = "linux") {
        &["wl-copy", "xclip"]
    } else {
        return Err("Clipboard not supported on this OS.".into());
    };
    let path = env::var_os("PATH").unwrap_or_default();
    let executable = candidates
        .iter()
        .find_map(|name| env::split_paths(&path).map(|dir| dir.join(name)).find(|path| executable(path)))
        .ok_or("Clipboard utility not found. Please install a clipboard tool.")?;
    let mut command = Command::new(&executable);
    if executable.file_name().is_some_and(|name| name == "xclip") {
        command.args(["-selection", "clipboard"]);
    }
    let mut child =
        command.stdin(Stdio::piped()).spawn().map_err(|error| format!("Failed to spawn clipboard command: {error}"))?;
    let write = match child.stdin.take() {
        Some(mut stdin) => stdin.write_all(bytes),
        None => Err(io::Error::other("clipboard stdin missing")),
    };
    let status = child.wait().map_err(|error| error.to_string())?;
    write.map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Clipboard command returned non-zero exit code.".into())
    }
}

/// Interactive reset preserves exports; explicit purge goes through the managed cache API.
pub(crate) fn run_cache_menu() {
    use crate::tui::{create_cache_menu, show_error, show_message};
    use crate::{run_headless_mode, SnapshotConfig};
    loop {
        match create_cache_menu().show() {
            Ok('1') => {
                run_headless_mode(SnapshotConfig { debug_ignored: true, tree_only: true, ..Default::default() });
                let _ = show_message("Check output above for ignored folders.");
            }
            Ok('2') => {
                run_headless_mode(SnapshotConfig { force_reset: true, ..Default::default() });
                let _ = show_message("Project state reset to V1.");
            }
            Ok('3') => {
                let args = ["cache", "purge", "--all", "--yes"].map(str::to_string);
                match crate::cache::run(&args) {
                    Ok(()) => {
                        let _ = show_message("Global cache cleared successfully.");
                    }
                    Err(error) => {
                        let _ = show_error(&format!("Failed to clear cache: {error}"));
                    }
                }
            }
            Ok('0') => return,
            Ok(_) => {
                let _ = show_error("Invalid selection. Try again.");
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        }
    }
}

fn executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
