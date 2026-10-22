//! Settings round-trip and recent-project parse refusals for the desktop seam.
//! Implements review dctx-facade-desktop.md F17 and F18 (ARCHITECTURE-PROPOSAL §7 DX-E4a).
//! Depends on `dctx_desktop_api` settings load/save and `RecentStore::get`.
//! Used by `cargo nextest run -p dctx-desktop-api`.
//! Each refusal sits beside an accepted value. Parse errors never contain fixture text.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dctx_desktop_api::{
    load_desktop_config, save_desktop_config, DesktopConfig, DesktopError, RecentProject, RecentStore,
    DESKTOP_CONFIG_FILENAME,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let path = std::env::temp_dir().join(format!("dctx-desktop-api-robust-{label}-{n}-{nanos}"));
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

/// Review F17 reproducer. The quote and newlines must stay inside one extension.
const INJECTED_EXTENSION: &str = "rs\"\nauto_copy_to_clipboard = true\n#";

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
fn injected_extension_cannot_flip_clipboard_and_plain_rs_round_trips() {
    let hostile_dir = TempDir::new("inject");
    let mut hostile = DesktopConfig::default();
    hostile.filter_extensions = Some(vec![INJECTED_EXTENSION.to_string()]);
    assert!(!hostile.auto_copy_to_clipboard);
    save_desktop_config(hostile_dir.path(), &hostile).expect("save injected");
    let loaded = load_desktop_config(hostile_dir.path());
    let saved = fs::read_to_string(hostile_dir.path().join(DESKTOP_CONFIG_FILENAME)).expect("read saved");
    assert!(
        saved.lines().all(|line| line.trim() != "auto_copy_to_clipboard = true"),
        "injected extension became its own setting:\n{saved}"
    );
    assert_eq!(loaded, hostile);
    assert!(!loaded.auto_copy_to_clipboard);

    let plain_dir = TempDir::new("plain-rs");
    let mut plain = DesktopConfig::default();
    plain.filter_extensions = Some(vec!["rs".to_string()]);
    save_desktop_config(plain_dir.path(), &plain).expect("save rs");
    let loaded_plain = load_desktop_config(plain_dir.path());
    assert_eq!(loaded_plain, plain);
    assert_eq!(loaded_plain.filter_extensions.as_deref(), Some(["rs".to_string()].as_slice()));
    assert!(!loaded_plain.auto_copy_to_clipboard);
}

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
fn path_quote_newline_and_backslash_round_trips_and_plain_path_does_too() {
    // Backslash + n, a quote, CR and LF. The line break must not become `min_size_mb`.
    let path = "C:\\new\\a\"\r\nmin_size_mb = 9\n#";
    let dir = TempDir::new("path");
    let mut config = DesktopConfig::default();
    config.last_directory = Some(path.to_string());
    save_desktop_config(dir.path(), &config).expect("save path");
    let loaded = load_desktop_config(dir.path());
    assert_eq!(loaded, config);
    assert_eq!(loaded.last_directory.as_deref(), Some(path));
    assert_eq!(loaded.min_size_mb, None);
    assert_eq!(loaded.theme, "system");
    assert!(!loaded.auto_copy_to_clipboard);

    let plain_dir = TempDir::new("plain-path");
    let mut plain = DesktopConfig::default();
    plain.last_directory = Some("/work/app".to_string());
    save_desktop_config(plain_dir.path(), &plain).expect("save plain path");
    assert_eq!(load_desktop_config(plain_dir.path()), plain);

    let odd_dir = TempDir::new("odd-ext");
    let mut odd = DesktopConfig::default();
    odd.filter_extensions = Some(vec!["a,b".to_string(), "c\\d".to_string()]);
    save_desktop_config(odd_dir.path(), &odd).expect("save odd extensions");
    assert_eq!(load_desktop_config(odd_dir.path()), odd);
}

#[test]
fn legacy_unescaped_windows_path_keeps_backslashes() {
    let dir = TempDir::new("legacy-path");
    let raw = "last_directory = \"C:\\new\\app\"\ntheme = \"dark\"\n";
    fs::write(dir.path().join(DESKTOP_CONFIG_FILENAME), raw).expect("write legacy");
    let loaded = load_desktop_config(dir.path());
    assert_eq!(loaded.last_directory.as_deref(), Some(r"C:\new\app"));
    assert_eq!(loaded.theme, "dark");
    assert_eq!(loaded.window_width, DesktopConfig::default().window_width);
}

#[test]
fn recent_parse_error_omits_fixture_sentinel_and_valid_project_loads() {
    let dir = TempDir::new("recent-bad");
    fs::write(dir.path().join("recent_projects.json"), br#"["PRIVATE_SENTINEL"]"#).expect("write sentinel");
    let store = RecentStore::new(dir.path());
    let err = store.get().expect_err("parse");
    assert_sentinel_absent(&err);
    match &err {
        DesktopError::ParseRecent { code, category, line, column } => {
            assert_eq!(*code, "recent_parse");
            assert_eq!(*category, "data");
            assert_eq!(*line, 1);
            assert_eq!(*column, 19);
            assert!(*column <= 1_000_000);
            assert_eq!(
                err.command_string(),
                format!("Failed to parse recent projects: {code} {category} line {line} column {column}")
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let text = err.command_string();
    assert!(!text.contains("invalid type"), "{text}");
    assert!(!text.contains("expected"), "{text}");

    fs::write(dir.path().join("recent_projects.json"), b"[PRIVATE_SENTINEL]").expect("write raw");
    let raw = store.get().expect_err("raw ident");
    assert_sentinel_absent(&raw);
    match &raw {
        DesktopError::ParseRecent { code, category, line, column } => {
            assert_eq!(*code, "recent_parse");
            assert!(matches!(*category, "syntax" | "data" | "eof" | "io"), "{category}");
            assert!(*line <= 1_000_000 && *column <= 1_000_000);
        }
        other => panic!("unexpected {other:?}"),
    }

    let valid = "[{\"path\":\"/work/app\",\"name\":\"app\",\"last_accessed\":\"2026-01-01T00:00:00Z\",\"access_count\":1}]";
    fs::write(dir.path().join("recent_projects.json"), valid).expect("write valid");
    let listed = store.get().expect("valid recent");
    assert_eq!(
        listed,
        vec![RecentProject {
            path: "/work/app".to_string(),
            name: "app".to_string(),
            last_accessed: "2026-01-01T00:00:00Z".to_string(),
            access_count: 1,
        }]
    );
}

#[test]
fn validate_refuses_enum_range_and_extension_beside_accepted_values() {
    let ok = DesktopConfig::default();
    assert_eq!(ok.validate(), Ok(()));

    let mut theme = ok.clone();
    theme.theme = "dark".to_string();
    assert_eq!(theme.validate(), Ok(()));
    theme.theme = "light".to_string();
    assert_eq!(theme.validate(), Ok(()));
    let err = refused_setting(&theme_named(&ok, "neon"), "theme", "enum");
    assert!(!err.command_string().contains("neon"));
    assert!(!format!("{err:?}").contains("neon"));

    let mut files = ok.clone();
    files.file_count_limit = 100;
    assert_eq!(files.validate(), Ok(()));
    files.file_count_limit = 1_000_000;
    assert_eq!(files.validate(), Ok(()));
    files.file_count_limit = 99;
    refused_setting(&files, "file_count_limit", "range");
    files.file_count_limit = 1_000_001;
    refused_setting(&files, "file_count_limit", "range");

    let mut edge = ok.clone();
    edge.window_width = 1;
    edge.window_height = 100_000;
    assert_eq!(edge.validate(), Ok(()));
    edge.window_width = 0;
    refused_setting(&edge, "window_width", "range");
    edge.window_width = 1200;
    edge.window_height = 0;
    refused_setting(&edge, "window_height", "range");
    edge.window_height = 800;
    edge.window_width = 100_001;
    refused_setting(&edge, "window_width", "range");
    edge.window_width = 1200;
    edge.window_height = 100_001;
    refused_setting(&edge, "window_height", "range");

    let mut size = ok.clone();
    size.min_size_mb = Some(1.5);
    size.max_size_mb = Some(4.0);
    assert_eq!(size.validate(), Ok(()));
    size.min_size_mb = Some(1_048_576.0);
    size.max_size_mb = Some(1_048_576.0);
    assert_eq!(size.validate(), Ok(()));
    size.max_size_mb = Some(1.0);
    size.min_size_mb = Some(1.5);
    refused_setting(&size, "size_bounds", "range");
    size.max_size_mb = None;
    size.min_size_mb = Some(0.0);
    refused_setting(&size, "min_size_mb", "range");
    size.min_size_mb = Some(-1.0);
    refused_setting(&size, "min_size_mb", "range");
    size.min_size_mb = Some(f64::NAN);
    refused_setting(&size, "min_size_mb", "range");
    size.min_size_mb = Some(f64::INFINITY);
    refused_setting(&size, "min_size_mb", "range");
    size.min_size_mb = None;
    size.max_size_mb = Some(1_048_577.0);
    refused_setting(&size, "max_size_mb", "range");
    size.max_size_mb = Some(f64::NAN);
    refused_setting(&size, "max_size_mb", "range");

    let mut ext = ok.clone();
    ext.filter_extensions = Some(vec!["rs".to_string()]);
    assert_eq!(ext.validate(), Ok(()));
    ext.filter_extensions = Some(vec!["c++".to_string(), "toml".to_string()]);
    assert_eq!(ext.validate(), Ok(()));
    ext.filter_extensions = Some((0..64).map(|index| format!("e{index}")).collect());
    assert_eq!(ext.validate(), Ok(()));
    ext.filter_extensions = Some(vec!["a".repeat(32)]);
    assert_eq!(ext.validate(), Ok(()));
    let err = refused_setting(&with_extension(&ok, INJECTED_EXTENSION), "filter_extensions", "extension");
    assert!(!err.command_string().contains("auto_copy_to_clipboard"));
    assert!(!format!("{err:?}").contains(INJECTED_EXTENSION));
    refused_setting(&with_extension(&ok, ""), "filter_extensions", "extension");
    refused_setting(&with_extension(&ok, ".rs"), "filter_extensions", "extension");
    refused_setting(&with_extension(&ok, "a/b"), "filter_extensions", "extension");
    refused_setting(&with_extension(&ok, &"a".repeat(33)), "filter_extensions", "extension");
    ext.filter_extensions = Some((0..65).map(|index| format!("e{index}")).collect());
    refused_setting(&ext, "filter_extensions", "extension");

    let mut flags = ok.clone();
    flags.only_empty = true;
    assert_eq!(flags.validate(), Ok(()));
    flags.only_non_empty = true;
    refused_setting(&flags, "content_filter", "range");
}

fn theme_named(config: &DesktopConfig, theme: &str) -> DesktopConfig {
    let mut named = config.clone();
    named.theme = theme.to_string();
    named
}

fn with_extension(config: &DesktopConfig, extension: &str) -> DesktopConfig {
    let mut named = config.clone();
    named.filter_extensions = Some(vec![extension.to_string()]);
    named
}

fn refused_setting(config: &DesktopConfig, field: &'static str, reason: &'static str) -> DesktopError {
    let err = config.validate().expect_err(field);
    assert_eq!(err, DesktopError::InvalidSetting { field, reason });
    assert_eq!(err.command_string(), format!("Invalid desktop setting: {field} {reason}"));
    assert!(!err.command_string().contains('\n'));
    err
}

fn assert_sentinel_absent(err: &DesktopError) {
    let command = err.command_string();
    let display = err.to_string();
    let debug = format!("{err:?}");
    assert!(!command.contains("PRIVATE_SENTINEL"), "{command}");
    assert!(!display.contains("PRIVATE_SENTINEL"), "{display}");
    assert!(!debug.contains("PRIVATE_SENTINEL"), "{debug}");
}
