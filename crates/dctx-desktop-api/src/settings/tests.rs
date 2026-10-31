use super::*;

fn scratch(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let path = std::env::temp_dir().join(format!("dctx-desktop-api-settings-{label}-{n}-{nanos}"));
    fs::create_dir_all(&path).expect("temp");
    path
}

#[test]
fn malformed_lines_are_ignored_and_valid_lines_apply() {
    let dir = scratch("malformed");
    let raw = "\
# comment
not a pair
unknown_key = 1
window_width = nope
window_height = 640
theme = \"dark\"
# theme = \"light\"
file_count_limit = 42
";
    fs::write(dir.join(DESKTOP_CONFIG_FILENAME), raw).expect("write");
    let loaded = load_desktop_config(&dir);
    assert_eq!(loaded.window_width, 1200);
    assert_eq!(loaded.window_height, 640);
    assert_eq!(loaded.theme, "dark");
    assert_eq!(loaded.file_count_limit, 42);
    assert!(!loaded.has_active_filters());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Start each configuration regression from the public defaults and override only the settings relevant to its scenario; separate mutation steps also exercise validation transitions.")]
fn save_load_and_reset_round_trip() {
    let dir = scratch("round");
    let mut config = DesktopConfig::default();
    config.last_directory = Some("/work/app".to_string());
    config.window_width = 1024;
    config.window_height = 768;
    config.file_count_limit = 50;
    config.theme = "light".to_string();
    config.auto_copy_to_clipboard = true;
    config.filter_extensions = Some(vec!["rs".to_string(), "toml".to_string()]);
    config.include_binary = true;
    config.only_hidden = true;
    config.min_size_mb = Some(1.5);
    config.max_size_mb = Some(4.0);
    config.only_empty = true;
    config.only_non_empty = false;
    config.tree_only = true;
    save_desktop_config(&dir, &config).expect("save");
    let loaded = load_desktop_config(&dir);
    assert_eq!(loaded, config);
    assert!(loaded.has_active_filters());
    reset_desktop_config(&dir).expect("reset");
    assert_eq!(load_desktop_config(&dir), DesktopConfig::default());
    let _ = fs::remove_dir_all(&dir);
}
