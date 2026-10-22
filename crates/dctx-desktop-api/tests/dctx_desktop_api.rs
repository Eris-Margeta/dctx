//! Public API coverage for the desktop engine seam.
//! Implements ARCHITECTURE-PROPOSAL §7 row DX-E4a: oversized refusal, settings parse, recent list, round trip.
//! Depends on `dctx_desktop_api`. Temp directories stand in for the injected config and storage dirs.
//! Each refusal sits next to a successful call on the same fixture.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_desktop_api::{
    generate_snapshot, load_desktop_config, reset_desktop_config, save_desktop_config, DesktopConfig, RecentStore,
    DESKTOP_CONFIG_FILENAME, MAX_RECENT_PROJECTS,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!("dctx-desktop-api-{label}-{n}-{nanos}"));
        fs::create_dir_all(&path).expect("temp");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
fn oversized_directory_is_refused_and_a_smaller_limit_succeeds() {
    let root = TempDir::new("oversize");
    fs::write(root.path().join("one.txt"), b"one").expect("one");
    fs::write(root.path().join("two.txt"), b"two").expect("two");
    fs::write(root.path().join("three.txt"), b"three").expect("three");

    let mut tight = DesktopConfig::default();
    tight.file_count_limit = 2;
    let refused = generate_snapshot(root.path(), &tight);
    assert!(!refused.success);
    assert!(!refused.is_formatted);
    assert_eq!(refused.output, "");
    assert_eq!(
        refused.error.as_deref(),
        Some("Directory too large: 3 files (limit: 2)\n\nPlease use filters or select a smaller directory.")
    );

    let mut open = DesktopConfig::default();
    open.file_count_limit = 3;
    let allowed = generate_snapshot(root.path(), &open);
    assert!(allowed.success, "{:?}", allowed.error);
    assert!(allowed.is_formatted);
    assert!(allowed.error.is_none());
    assert!(allowed.output.starts_with("# pack "));
}

#[test]
fn malformed_settings_lines_are_ignored() {
    let dir = TempDir::new("settings");
    let raw = "\
# comment
this is not a setting
unknown_key = 9
window_width = nope
window_height = 700
theme = \"dark\"
# theme = \"light\"
file_count_limit = 15
auto_copy_to_clipboard = maybe
";
    fs::write(dir.path().join(DESKTOP_CONFIG_FILENAME), raw).expect("write");
    let loaded = load_desktop_config(dir.path());
    assert_eq!(loaded.window_width, DesktopConfig::default().window_width);
    assert_eq!(loaded.window_height, 700);
    assert_eq!(loaded.theme, "dark");
    assert_eq!(loaded.file_count_limit, 15);
    assert!(!loaded.auto_copy_to_clipboard);
}

#[test]
fn recent_projects_dedup_and_cap() {
    let root = TempDir::new("recent");
    let store = RecentStore::new(root.path().join("state"));
    assert_eq!(store.get().expect("empty"), [] as [dctx_desktop_api::RecentProject; 0]);

    let first = root.path().join("proj");
    fs::create_dir(&first).expect("proj");
    let first_path = first.to_str().expect("utf8");
    store.add(first_path, "2026-02-01T00:00:00Z").expect("add");
    store.add(first_path, "2026-02-03T00:00:00Z").expect("refresh");
    let deduped = store.get().expect("deduped");
    assert_eq!(deduped.len(), 1);
    assert_eq!(deduped[0].access_count, 2);
    assert_eq!(deduped[0].last_accessed, "2026-02-03T00:00:00Z");
    assert_eq!(deduped[0].name, "proj");

    let mut projects = Vec::new();
    for index in 0..MAX_RECENT_PROJECTS {
        let dir = root.path().join(format!("p{index:02}"));
        fs::create_dir(&dir).expect("dir");
        let path = dir.to_str().expect("utf8").to_string();
        let stamp = format!("2026-03-{:02}T00:00:00Z", index + 1);
        store.add(&path, &stamp).expect("add many");
        projects.push((path, stamp));
    }

    let listed = store.get().expect("list");
    assert_eq!(listed.len(), MAX_RECENT_PROJECTS);
    assert!(listed.iter().all(|project| project.path != first_path));
    assert_eq!(listed[0].path, projects[MAX_RECENT_PROJECTS - 1].0);
    assert_eq!(listed[0].last_accessed, projects[MAX_RECENT_PROJECTS - 1].1);
    assert_eq!(listed[0].name, format!("p{:02}", MAX_RECENT_PROJECTS - 1));
    assert_eq!(listed[0].access_count, 1);
    let oldest_kept = &projects[0];
    assert_eq!(listed.last().expect("last").path, oldest_kept.0);

    store.add(first_path, "2026-04-01T00:00:00Z").expect("readd");
    let refreshed = store.get().expect("refreshed");
    assert_eq!(refreshed.len(), MAX_RECENT_PROJECTS);
    assert_eq!(refreshed[0].path, first_path);
    assert_eq!(refreshed[0].access_count, 1);
    assert_eq!(refreshed[0].name, "proj");
}

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
fn settings_save_load_round_trip() {
    let dir = TempDir::new("round");
    let cache = TempDir::new("cache");
    dctx_desktop_api::ensure_config_dirs(dir.path(), cache.path()).expect("dirs");

    let mut config = DesktopConfig::default();
    config.last_directory = Some(dir.path().display().to_string());
    config.window_width = 1440;
    config.window_height = 900;
    config.file_count_limit = 80;
    config.theme = "dark".to_string();
    config.auto_copy_to_clipboard = true;
    config.filter_extensions = Some(vec!["rs".to_string(), "md".to_string()]);
    config.include_binary = false;
    config.only_hidden = false;
    config.min_size_mb = Some(1.5);
    config.max_size_mb = Some(8.0);
    config.only_empty = false;
    config.only_non_empty = true;
    config.tree_only = true;

    save_desktop_config(dir.path(), &config).expect("save");
    assert_eq!(load_desktop_config(dir.path()), config);

    reset_desktop_config(dir.path()).expect("reset");
    assert_eq!(load_desktop_config(dir.path()), DesktopConfig::default());
}
