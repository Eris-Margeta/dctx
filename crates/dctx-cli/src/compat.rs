// SPDX-License-Identifier: Apache-2.0
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
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

/// Legacy directories are separate from the v2 managed-cache root.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
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
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn app_config() -> settings::AppConfig {
    config_dir()
        .and_then(|dir| fs::read_to_string(dir.join(settings::CONFIG_FILENAME)).ok())
        .map(|text| settings::parse_config(&text))
        .unwrap_or_default()
}

/// Stable UTF-8 FNV-1a id, including the legacy 16-digit lower-case representation.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn path_id(path: &str) -> String {
    let hash =
        path.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// Existing registry rows win over hashing; the final row for a path wins.
/// # Errors
/// Returns filesystem I/O errors or an invalid existing/generated project ID.
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
        return checked_id(id.trim());
    }
    let id = path_id(&path);
    checked_id(&id)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).open(registry)?;
    writeln!(file, "{id} | {path}")?;
    Ok(id)
}

/// Historical registry ids are one path component. Absolute ids and `..` discard the cache prefix.
fn checked_id(id: &str) -> io::Result<String> {
    if single_component(id) {
        Ok(id.to_string())
    } else {
        Err(io::Error::other(format!("invalid project id '{id}'")))
    }
}

fn single_component(id: &str) -> bool {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains('\0') {
        return false;
    }
    let path = Path::new(id);
    if path.is_absolute() {
        return false;
    }
    let mut components = path.components();
    let Some(Component::Normal(name)) = components.next() else {
        return false;
    };
    components.next().is_none() && name == id
}

/// `{id}.dctx` directly inside `cache`. A traversal id fails before any create or replace.
pub(crate) fn state_path(cache: &Path, id: &str) -> io::Result<PathBuf> {
    checked_id(id)?;
    let path = cache.join(format!("{id}.dctx"));
    if path.parent() != Some(cache) {
        return Err(io::Error::other("project state escapes the cache"));
    }
    Ok(path)
}

/// `.dctx` must be a real directory inside the canonical project root, never a link.
pub(crate) fn prepare_export_dir(root: &Path) -> Result<PathBuf, String> {
    let output = root.join(".dctx");
    if output.parent() != Some(root) {
        return Err(format!("refusing output directory {}", output.display()));
    }
    if let Ok(meta) = fs::symlink_metadata(&output) {
        if !meta.is_dir() {
            return Err(format!("refusing output directory {}", output.display()));
        }
    }
    fs::create_dir_all(&output).map_err(|error| format!("Failed to create output directory .dctx/: {error}"))?;
    let meta = fs::symlink_metadata(&output).map_err(|error| error.to_string())?;
    if !meta.is_dir() {
        return Err(format!("refusing output directory {}", output.display()));
    }
    Ok(output)
}

/// One normal file name inside `parent`. `..` and absolute names cannot discard the parent.
pub(crate) fn child_file(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if !single_component(name) {
        return Err(format!("refusing path component {name}"));
    }
    let path = parent.join(name);
    if path.parent() != Some(parent) {
        return Err(format!("refusing path {}", path.display()));
    }
    Ok(path)
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

/// Write `bytes` beside `path` and rename over it only after the temp file is complete.
/// `rename` replaces a destination symlink; it does not open the link target.
fn replace_bytes(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).ok_or_else(|| {
        io::Error::other("destination has no parent directory")
    })?;
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(io::Error::other("destination file name is not a single component"));
    };
    if !single_component(name) {
        return Err(io::Error::other("refusing a destination that leaves its parent"));
    }
    if !fs::symlink_metadata(parent)?.is_dir() {
        return Err(io::Error::other("refusing to write through a link or non-directory"));
    }
    let temp = parent.join(temp_name(name));
    let written = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(())
}

fn temp_name(name: &str) -> PathBuf {
    let mut temp = std::ffi::OsString::from(".");
    temp.push(name);
    temp.push(".");
    temp.push(std::process::id().to_string());
    temp.push(".");
    temp.push(TEMP_SEQ.fetch_add(1, Ordering::Relaxed).to_string());
    temp.push(".tmp");
    PathBuf::from(temp)
}

fn read_state(path: &Path, bodies: LegacyBodies) -> Result<(DirContextTreeNode, Version), String> {
    let limits = LegacyLimits::default();
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(limits.max_total_bytes as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    let document = decode_legacy(&bytes, bodies, limits).map_err(|error| error.to_string())?;
    Ok((document.root, document.header.version))
}

/// History keeps the old reader and folder-recreation heuristic. Bodies are read so a touch-only change can be told from an edit.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
#[allow(clippy::unnecessary_debug_formatting, reason = "Retain quoted and escaped filesystem paths in user diagnostics, including paths containing controls or spaces.")]
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
    match read_state(binary, LegacyBodies::Include) {
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
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn last_version(path: &Path) -> Option<String> {
    let root = fs::canonicalize(path).ok()?;
    let cache = cache_dir()?;
    let id = project_id(&cache, &root).ok()?;
    let binary = state_path(&cache, &id).ok()?;
    read_state(&binary, LegacyBodies::MetadataOnly).ok().map(|(_, version)| version.to_string())
}

/// Encode before touching the previous binary, so codec refusal leaves it intact.
/// # Errors
/// Returns legacy encoding/limit errors or a failed atomic state replacement.
pub fn save_state(path: &Path, tree: &DirContextTreeNode, version: &Version) -> Result<(), String> {
    let bytes = encode_legacy(tree, version, LegacyLimits::default()).map_err(|error| error.to_string())?;
    replace_bytes(path, &bytes).map_err(|error| error.to_string())
}

fn rule(path: &Path) -> Result<Option<String>, String> {
    StdFs.read_policy(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Ignore and omit rules are fixed before any source read. A missing or unreadable omit file adds no rules.
/// # Errors
/// Returns errors reading global or project ignore/omit rule files.
pub fn scan_policy(root: &Path, verbose: bool) -> Result<SelectionPolicy, String> {
    let global_ignore = config_dir().map(|dir| rule(&dir.join("ignore"))).transpose()?.flatten();
    let global_omit = config_dir().map(|dir| rule(&dir.join("omitcontent"))).transpose()?.flatten();
    let local_ignore = rule(&root.join(".dctxignore"))?;
    let local_omit = rule(&root.join(".dctxomitcontent")).ok().flatten();
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        global_ignore: global_ignore.as_deref(),
        global_omit: global_omit.as_deref(),
        local_ignore: local_ignore.as_deref(),
        local_omit: local_omit.as_deref(),
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
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn output_policy(root: &Path) -> SelectionPolicy {
    let global = config_dir().and_then(|dir| rule(&dir.join("omitcontent")).ok().flatten());
    let local = rule(&root.join(".dctxomitcontent")).ok().flatten();
    SelectionPolicy::from_texts(&RuleTexts {
        global_omit: global.as_deref(),
        local_omit: local.as_deref(),
        ..RuleTexts::default()
    })
}

/// Publish one named export by renaming a completed temp file in the same directory.
/// No reset, version bump or corruption path sweeps old exports.
/// # Errors
/// Returns output replacement failures; optional diff cleanup is best effort.
pub fn export(path: &Path, bytes: &[u8], diff: bool) -> Result<(), String> {
    replace_bytes(path, bytes).map_err(|error| format!("Write error for {}: {error}", path.display()))?;
    println!(
        "Legacy {} wrote {} ({} bytes, retention=legacy-project-text).",
        if diff { "diff" } else { "compatibility alias" },
        path.display(),
        bytes.len()
    );
    Ok(())
}

/// Resolve clipboard tools without launching a command-discovery child.
/// # Errors
/// Returns unsupported-platform/tool-not-found errors, spawn/stdin/wait failures, or a nonzero clipboard-tool exit.
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
