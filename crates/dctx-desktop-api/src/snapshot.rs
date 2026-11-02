//! Snapshot response built through the `dctx` facade.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a and the `generate_snapshot` limits.
//! Depends on the `dctx` session, the desktop rule loader, the filtered source and [`crate::settings::DesktopConfig`].
//! The Tauri layer serializes [`SnapshotResponse`]. This module never spawns git or reads `.git`.

#[cfg(test)]
use std::fs;
use std::path::Path;
use std::sync::Arc;

use dctx::memory::{CancelFlag, FixedClock};
use dctx::{
    BoundedSink, HostCeiling, HostGrants, PackRequest, SelectionPolicy, Session, SnapshotRequest, StoragePolicy,
    Timestamp, VecSink,
};
use serde::{Deserialize, Serialize};

use crate::error::DesktopError;
use crate::settings::DesktopConfig;
use crate::snapshot_rules::{load_project_rules, ProjectRules};
use crate::snapshot_source::{desktop_source, survey};

/// Drops negations that name a builtin-excluded literal. Wildcard and nested negations stay.
#[cfg(test)]
pub(crate) use crate::snapshot_rules::without_builtin_negations;

/// Bytes the ephemeral facade ledger will admit. This is an accounting ceiling, not an allocation.
const SESSION_BYTES: u64 = 2 * 1024 * 1024 * 1024;

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

/// Counts files, applies desktop filters, then builds a facade snapshot.
///
/// Rule files load first. Filters and the `.git` boundary sit on the source, so a
/// content read happens only for a file the desktop selection still includes.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn generate_snapshot(root: &Path, config: &DesktopConfig) -> SnapshotResponse {
    if !root.exists() {
        return refuse(&DesktopError::PathMissing);
    }
    let rules = match load_project_rules(root) {
        Ok(rules) => rules,
        Err(error) => return refuse(&error),
    };
    let surveyed = match survey(root, config) {
        Ok(surveyed) => surveyed,
        Err(error) => return refuse(&error),
    };
    let limit = u64::try_from(config.file_count_limit).unwrap_or(u64::MAX);
    if surveyed.files > limit {
        return refuse(&DesktopError::DirectoryTooLarge { files: surveyed.files, limit });
    }
    if config.has_active_filters() && surveyed.files > 0 && surveyed.included == 0 {
        return refuse(&DesktopError::AllFiltered);
    }
    match render(root, config, &rules) {
        Ok(output) => SnapshotResponse { success: true, output, error: None, is_formatted: true },
        Err(error) => refuse(&error),
    }
}

fn refuse(error: &DesktopError) -> SnapshotResponse {
    SnapshotResponse { success: false, output: String::new(), error: Some(error.command_string()), is_formatted: false }
}

fn render(root: &Path, config: &DesktopConfig, rules: &ProjectRules) -> Result<String, DesktopError> {
    let mut session = open_session(root, config, rules)?;
    if config.tree_only { render_metadata(&mut session) } else { render_pack(&mut session) }
}

fn open_session(root: &Path, config: &DesktopConfig, rules: &ProjectRules) -> Result<Session, DesktopError> {
    let source = desktop_source(root, config)?;
    let texts = rules.texts();
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
    root.file_name().and_then(|name| name.to_str()).filter(|name| !name.is_empty()).unwrap_or("project").to_string()
}

fn render_metadata(session: &mut Session) -> Result<String, DesktopError> {
    let handle = session
        .snapshot(&SnapshotRequest::metadata())
        .map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    // The digest hashes modification times, so it changes on a bare `touch`: it goes last, behind a lead line that never changes.
    Ok(format!("# snapshot metadata\n# snapshot generation={} digest={}\n", handle.generation(), handle.digest()))
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
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
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
        assert_eq!(response.output, "");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
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
    #[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
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
            fs::set_permissions(&git, fs::Permissions::from_mode(0o0)).expect("chmod");
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
    #[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
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
    #[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
    fn tree_only_uses_a_metadata_snapshot() {
        let root = scratch("tree");
        fs::write(root.join("keep.txt"), b"keep").expect("keep");
        let mut config = DesktopConfig::default();
        config.tree_only = true;
        let response = generate_snapshot(&root, &config);
        assert!(response.success, "{:?}", response.error);
        assert!(response.output.starts_with("# snapshot metadata\n"));
        let (lead, trailer) = response.output.split_once('\n').expect("two lines");
        assert_eq!(lead, "# snapshot metadata");
        assert!(trailer.starts_with("# snapshot generation=") && trailer.contains(" digest=") && trailer.ends_with('\n'));
        assert_eq!(trailer.matches('\n').count(), 1, "the trailer is the last line");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builtin_negation_lines_are_removed() {
        let cleaned = without_builtin_negations("!.git/\n!keep.txt\n# note\n");
        assert!(!cleaned.contains("!.git"));
        assert!(cleaned.contains("!keep.txt"));
    }
}
