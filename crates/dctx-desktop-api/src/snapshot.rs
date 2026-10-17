//! Snapshot response built through the `dctx` facade.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a and the `generate_snapshot` limits.
//! Depends on the `dctx` session, [`crate::settings::DesktopConfig`] and [`crate::error::DesktopError`].
//! The Tauri layer serializes [`SnapshotResponse`]. This module never spawns git or reads `.git`.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    BoundedSink, FsSource, HostCeiling, HostGrants, PackRequest, RuleTexts, SelectionPolicy, Session, SnapshotRequest,
    StoragePolicy, Timestamp, VecSink,
};
use serde::{Deserialize, Serialize};

use crate::error::DesktopError;
use crate::settings::DesktopConfig;

/// Bytes the ephemeral facade ledger will admit. This is an accounting ceiling, not an allocation.
const SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Matches the facade safety depth. Deeper entries are not counted.
const MAX_DEPTH: u32 = 256;

/// JSON body `main.ts` reads from `generate_snapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SnapshotResponse {
    /// `true` when `output` holds a facade page.
    pub success: bool,
    /// Facade pack text, or a metadata snapshot line when `tree_only` is set.
    pub output: String,
    /// Command text from [`DesktopError::command_string`] when `success` is false.
    pub error: Option<String>,
    /// `true` when `output` is the formatted page.
    pub is_formatted: bool,
}

#[derive(Debug, Default)]
struct Survey {
    files: u64,
    included: u64,
}

/// Counts files, applies desktop filters, then builds a facade snapshot.
///
/// The count runs before any file contents are read. Builtin ignore names, including `.git`,
/// are not opened. A negation in `.dctxignore` cannot put `.git` back into the walk.
pub fn generate_snapshot(root: &Path, config: &DesktopConfig) -> SnapshotResponse {
    if !root.exists() {
        return refuse(DesktopError::PathMissing);
    }
    let local_ignore = match read_local_ignore(root) {
        Ok(text) => text,
        Err(error) => return refuse(error),
    };
    let survey = match survey(root, config) {
        Ok(survey) => survey,
        Err(error) => return refuse(error),
    };
    let limit = u64::try_from(config.file_count_limit).unwrap_or(u64::MAX);
    if survey.files > limit {
        return refuse(DesktopError::DirectoryTooLarge { files: survey.files, limit });
    }
    if config.has_active_filters() && survey.files > 0 && survey.included == 0 {
        return refuse(DesktopError::AllFiltered);
    }
    match render(root, config, local_ignore.as_deref()) {
        Ok(output) => SnapshotResponse { success: true, output, error: None, is_formatted: true },
        Err(error) => refuse(error),
    }
}

fn refuse(error: DesktopError) -> SnapshotResponse {
    SnapshotResponse { success: false, output: String::new(), error: Some(error.command_string()), is_formatted: false }
}

fn read_local_ignore(root: &Path) -> Result<Option<String>, DesktopError> {
    let path = root.join(".dctxignore");
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .map_err(|error| DesktopError::IgnoreRules { detail: error.to_string() })?;
    Ok(Some(without_builtin_negations(&text)))
}

/// Drops negations that would re-include a builtin-excluded name. `.git` stays closed (decision 078).
fn without_builtin_negations(text: &str) -> String {
    let mut kept = String::new();
    for line in text.lines() {
        if negation_lifts_builtin(line) {
            continue;
        }
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

fn negation_lifts_builtin(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix('!') else {
        return false;
    };
    let token = rest.split('#').next().unwrap_or(rest).trim().trim_matches('/');
    let name = token.split('/').next().unwrap_or(token);
    excluded_by_builtin(name, true) || excluded_by_builtin(name, false)
}

fn survey(root: &Path, config: &DesktopConfig) -> Result<Survey, DesktopError> {
    let meta = fs::metadata(root).map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    let mut survey = Survey::default();
    if meta.is_dir() {
        survey_dir(root, 0, true, config, &mut survey)?;
    } else {
        survey.files = 1;
        let name = root.file_name().and_then(|item| item.to_str()).unwrap_or("");
        if file_visible(name, meta.len(), config) {
            survey.included = 1;
        }
    }
    Ok(survey)
}

fn survey_dir(dir: &Path, depth: u32, admit: bool, config: &DesktopConfig, survey: &mut Survey) -> Result<(), DesktopError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if depth == 0 => return Err(DesktopError::Walk { detail: error.to_string() }),
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.is_empty() || name == "." || name == ".." || name == ".git" {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if excluded_by_builtin(name, is_dir) {
            continue;
        }
        if is_dir {
            let child_admit = admit && directory_visible(name, config);
            survey_dir(&entry.path(), depth.saturating_add(1), child_admit, config, survey)?;
        } else {
            survey.files = survey.files.saturating_add(1);
            let len = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            if admit && file_visible(name, len, config) {
                survey.included = survey.included.saturating_add(1);
            }
        }
    }
    Ok(())
}

fn excluded_by_builtin(name: &str, is_dir: bool) -> bool {
    if is_dir {
        return matches!(
            name,
            ".git" | ".svn" | ".hg" | "node_modules" | "bower_components" | "dist" | "build" | "target" | "bin"
                | "obj" | ".venv" | "venv" | "__pycache__" | ".dctx" | ".cursor" | ".aider" | ".eris-code"
                | ".continue"
        );
    }
    matches!(name, ".DS_Store" | "Thumbs.db" | ".dctxignore") || has_extension(name, "pyc") || has_extension(name, "dctx")
        || has_extension(name, "dircontxt")
}

fn has_extension(name: &str, extension: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn directory_visible(name: &str, config: &DesktopConfig) -> bool {
    if config.has_active_filters() && config.only_hidden && !is_hidden(name) {
        return false;
    }
    true
}

fn file_visible(name: &str, len: u64, config: &DesktopConfig) -> bool {
    if !config.has_active_filters() {
        return true;
    }
    if config.only_hidden && !is_hidden(name) {
        return false;
    }
    if let Some(exts) = config.filter_extensions.as_ref().filter(|exts| !exts.is_empty()) {
        let Some(ext) = extension_of(name) else {
            return false;
        };
        if !exts.iter().any(|wanted| wanted.eq_ignore_ascii_case(ext)) {
            return false;
        }
    }
    if config.only_empty && len > 0 {
        return false;
    }
    if config.only_non_empty && len == 0 {
        return false;
    }
    if let Some(min) = config.min_size_mb {
        if len < mb_to_bytes(min) {
            return false;
        }
    }
    if let Some(max) = config.max_size_mb {
        if len > mb_to_bytes(max) {
            return false;
        }
    }
    true
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

fn extension_of(file_name: &str) -> Option<&str> {
    let pos = file_name.rfind('.')?;
    if pos == 0 && file_name.len() > 1 {
        return Some(&file_name[1..]);
    }
    if pos > 0 && pos + 1 < file_name.len() {
        return Some(&file_name[pos + 1..]);
    }
    None
}

fn mb_to_bytes(mb: f64) -> u64 {
    let bytes = mb * 1_048_576.0;
    if !bytes.is_finite() || bytes <= 0.0 {
        return 0;
    }
    if bytes >= u64::MAX as f64 {
        u64::MAX
    } else {
        bytes as u64
    }
}

fn render(root: &Path, config: &DesktopConfig, local_ignore: Option<&str>) -> Result<String, DesktopError> {
    let mut session = open_session(root, local_ignore)?;
    if config.tree_only {
        render_metadata(&mut session)
    } else {
        render_pack(&mut session)
    }
}

fn open_session(root: &Path, local_ignore: Option<&str>) -> Result<Session, DesktopError> {
    let source = FsSource::open(root).map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    let texts = RuleTexts { local_ignore, ..RuleTexts::default() };
    Session::builder(source)
        .clock(FixedClock(Timestamp(0)))
        .cancellation(Arc::new(CancelFlag::new()))
        .selection(SelectionPolicy::from_texts(&texts))
        .ceiling(HostCeiling::base())
        .grants(HostGrants::PermitAll)
        .storage(StoragePolicy::Ephemeral { max_bytes: SESSION_BYTES })
        .place(place_name(root))
        .view("default")
        .build()
        .map_err(|error| DesktopError::Walk { detail: error.to_string() })
}

fn place_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("project")
        .to_string()
}

fn render_metadata(session: &mut Session) -> Result<String, DesktopError> {
    let handle = session
        .snapshot(&SnapshotRequest::metadata())
        .map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    Ok(format!("# snapshot generation={} digest={}\n", handle.generation(), handle.digest()))
}

fn render_pack(session: &mut Session) -> Result<String, DesktopError> {
    let mut request = PackRequest::new();
    request.output_format = 1;
    // decision 078: a git revision is never sent. The facade would refuse it before the scan.
    request.git_revision = None;
    let mut sink = VecSink::new();
    let page: &mut dyn BoundedSink = &mut sink;
    session.pack(&request, page).map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    String::from_utf8(sink.as_bytes().to_vec()).map_err(|error| DesktopError::Format { detail: error.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("dctx-desktop-api-snap-{label}-{n}-{nanos}"));
        fs::create_dir_all(&path).expect("temp");
        path
    }

    #[test]
    fn missing_path_is_refused_before_a_walk() {
        let response = generate_snapshot(Path::new("/dctx-desktop-api-missing-path"), &DesktopConfig::default());
        assert!(!response.success);
        assert_eq!(response.error.as_deref(), Some("Path does not exist"));
        assert!(!response.is_formatted);
        assert!(response.output.is_empty());
    }

    #[test]
    fn oversized_directory_is_refused_and_a_small_one_packs() {
        let root = scratch("size");
        fs::write(root.join("a.txt"), b"a").expect("a");
        fs::write(root.join("b.txt"), b"b").expect("b");
        fs::write(root.join("c.txt"), b"c").expect("c");
        let mut tight = DesktopConfig::default();
        tight.file_count_limit = 2;
        let refused = generate_snapshot(&root, &tight);
        assert_eq!(
            refused.error.as_deref(),
            Some("Directory too large: 3 files (limit: 2)\n\nPlease use filters or select a smaller directory.")
        );
        assert!(!refused.success);
        let allowed = generate_snapshot(&root, &DesktopConfig::default());
        assert!(allowed.success, "{:?}", allowed.error);
        assert!(allowed.is_formatted);
        assert!(allowed.output.starts_with("# pack "));
        let value = serde_json::to_value(&allowed).expect("json");
        assert_eq!(value["success"], true);
        assert_eq!(value["is_formatted"], true);
        assert!(value["output"].is_string());
        assert!(value["error"].is_null());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_directory_is_not_opened_or_counted() {
        let root = scratch("git");
        fs::write(root.join("keep.txt"), b"keep").expect("keep");
        let git = root.join(".git");
        fs::create_dir(&git).expect("git");
        fs::write(git.join("config"), b"secret").expect("config");
        fs::write(root.join(".dctxignore"), "!.git/\n").expect("ignore");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&git, fs::Permissions::from_mode(0)).expect("chmod");
        }
        let mut config = DesktopConfig::default();
        config.file_count_limit = 1;
        let response = generate_snapshot(&root, &config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&git, fs::Permissions::from_mode(0o755));
        }
        assert!(response.success, "{:?}", response.error);
        assert!(!response.output.contains("secret"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn extension_filter_refusal_has_a_matching_success() {
        let root = scratch("filter");
        fs::write(root.join("note.txt"), b"n").expect("note");
        let mut only_rs = DesktopConfig::default();
        only_rs.filter_extensions = Some(vec!["rs".to_string()]);
        let refused = generate_snapshot(&root, &only_rs);
        assert_eq!(refused.error.as_deref(), Some("All files filtered out"));
        fs::write(root.join("main.rs"), b"fn main() {}\n").expect("rs");
        let allowed = generate_snapshot(&root, &only_rs);
        assert!(allowed.success, "{:?}", allowed.error);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tree_only_uses_a_metadata_snapshot() {
        let root = scratch("tree");
        fs::write(root.join("keep.txt"), b"keep").expect("keep");
        let mut config = DesktopConfig::default();
        config.tree_only = true;
        let response = generate_snapshot(&root, &config);
        assert!(response.success, "{:?}", response.error);
        assert!(response.output.starts_with("# snapshot generation="));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builtin_negation_lines_are_removed() {
        let cleaned = without_builtin_negations("!.git/\n!keep.txt\n# note\n");
        assert!(!cleaned.contains("!.git"));
        assert!(cleaned.contains("!keep.txt"));
    }
}
