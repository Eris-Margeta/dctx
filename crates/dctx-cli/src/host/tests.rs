// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn session_cache_status_is_a_facade_report() {
    let report = session_cache(SessionCacheCall::Status).expect("status");
    assert_eq!(report.reclaimed_bytes, 0);
    assert_eq!(report.generations, 0);
    let gc = session_cache(SessionCacheCall::Gc { apply: false }).expect("gc");
    assert_eq!(gc.reclaimed_bytes, 0);
    let purge = session_cache(SessionCacheCall::Purge { apply: false }).expect("purge");
    assert_eq!(purge.reclaimed_bytes, 0);
}

#[test]
fn empty_path_is_refused_before_a_scan() {
    assert!(open_dir(Path::new("")).is_err());
}

#[test]
fn relative_cache_roots_and_the_auto_clean_switch_are_refused() {
    assert!(ensure_cache_dir(Path::new("cache")).is_err());
    let unsafe_root = std::env::temp_dir().join("../dctx-cache");
    assert!(unsafe_root.is_absolute());
    assert!(ensure_cache_dir(&unsafe_root).is_err());
    assert!(!auto_clean_enabled(&["--no-auto-clean".to_string()], None));
    assert!(!auto_clean_enabled(&[], Some("0")));
    assert!(auto_clean_enabled(&[], Some("1")));
    let kept = without_auto_flag(vec![
        "dctx".to_string(),
        "--no-auto-clean".to_string(),
        ".".to_string(),
    ]);
    assert_eq!(kept, vec!["dctx".to_string(), ".".to_string()]);
}

#[test]
fn disabled_auto_clean_keeps_staged_bytes_and_an_enabled_slice_reclaims_them() {
    let root = scratch("auto");
    let mut life = Lifecycle::product_default();
    life.load_staged(1_000, 0).expect("staged");
    life.load_staged(1_000, 0).expect("staged");
    store_ledger(&root, &life).expect("store");
    let before = fs::read(root.join("ledger.v1")).expect("ledger");
    configure(&root, Some(false), None, None).expect("off");
    let skipped = piggyback_at(&root, true).expect("disabled");
    assert_eq!(skipped, 0);
    assert_eq!(fs::read(root.join("ledger.v1")).expect("ledger"), before);
    assert!(load_ledger(&root)
        .expect("load")
        .last_auto_clean()
        .is_none());
    let blocked = piggyback_at(&root, false).expect("flag");
    assert_eq!(blocked, 0);
    configure(&root, Some(true), None, None).expect("on");
    let reclaimed = piggyback_at(&root, true).expect("clean");
    assert_eq!(reclaimed, 2_000);
    let restored = load_ledger(&root).expect("load");
    assert_eq!(restored.admitted(), 0);
    assert!(restored.last_auto_clean().is_some());
    let report = auto_report(&root).expect("auto");
    assert_eq!(report.reclaimed_bytes, 2_000);
    assert!(report.enabled);
    configure(&root, None, Some(1_000_000), None).expect("first cap");
    configure(&root, None, Some(500_000), None).expect("second cap");
    assert_eq!(load_ledger(&root).expect("load").status().hard_cap, 500_000);
    let page = crate::cache::run_in(&args(&["cache", "auto"]), Some(&root)).expect("page");
    assert!(page.contains("reclaimed_bytes: 2000"));
    assert!(page.contains("last_auto_clean_ms: "));
    assert!(!page.contains("last_auto_clean_ms: none"));
}

#[test]
fn nested_gitignore_lines_become_root_relative() {
    assert_eq!(
        rewrite_nested_line("src", "generated.rs"),
        "src/**/generated.rs"
    );
    assert_eq!(rewrite_nested_line("src", "/only.rs"), "src/only.rs");
    assert_eq!(rewrite_nested_line("src", "build/"), "src/**/build/");
    assert_eq!(rewrite_nested_line("src", "!keep.rs"), "!src/**/keep.rs");
    assert_eq!(rewrite_nested_line("", "*.log"), "*.log");
    assert_eq!(rewrite_nested_line("src", "# note"), "# note");
    assert_eq!(rewrite_nested_line("src", "foo/bar"), "src/foo/bar");
}

#[test]
fn nested_rules_anchor_at_their_directory_and_a_leading_slash_is_root_only() {
    let mut local = String::new();
    append_text(&mut local, "/corpus/\n*.secret\n!keep.secret\n").unwrap();
    append_nested(&mut local, "src", "generated.rs\n").unwrap();
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        local_ignore: Some(&local),
        ..RuleTexts::default()
    });
    let decision = |path: &str, dir: bool| {
        let name = path.rsplit('/').next().unwrap_or(path);
        policy.decide(&ItemContext {
            relative_path: path,
            name,
            is_directory: dir,
            debug_mode: false,
        })
    };
    let excluded = |path: &str, dir: bool| decision(path, dir) == FileHandlingDecision::Exclude;
    let described: Vec<String> = policy
        .ignore_rules()
        .iter()
        .map(|rule| {
            format!(
                "{:?} {} dir={} neg={}",
                rule.pattern_type, rule.pattern, rule.is_dir_only, rule.is_negation
            )
        })
        .collect();
    assert!(
        excluded("corpus", true),
        "decision={:?} local={local:?} rules={described:?}",
        decision("corpus", true)
    );
    assert!(!excluded("src/corpus", true));
    assert!(!excluded("corpus/ready.json", false));
    assert!(excluded("drop.secret", false));
    assert!(!excluded("keep.secret", false));
    assert!(excluded("src/generated.rs", false));
    assert!(excluded("src/a/generated.rs", false));
    assert!(!excluded("generated.rs", false));
    assert!(!excluded("src/lib.rs", false));
}

#[cfg(unix)]
#[test]
fn an_ignored_unreadable_directory_is_not_listed() {
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let Ok(meta) = fs::metadata(&self.0) else {
                return;
            };
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&self.0, perms);
        }
    }
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("ignored-corpus");
    fs::write(root.join(".gitignore"), "/corpus/\n").expect("ignore");
    let corpus = root.join("corpus");
    fs::create_dir(&corpus).expect("corpus");
    fs::write(corpus.join("ready.json"), b"{}\n").expect("ready");
    let mut perms = fs::metadata(&corpus).expect("meta").permissions();
    perms.set_mode(0o0);
    fs::set_permissions(&corpus, perms).expect("chmod");
    {
        let _restore = Restore(corpus);
        let text = local_ignore_text(&root, None)
            .expect("rules")
            .expect("text");
        assert!(text.contains("/corpus/"), "{text}");
    }
    let _ = fs::remove_dir_all(&root);
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dctx-cli-host-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("dir");
    fs::canonicalize(path).expect("canonical fixture for low-level rule reads")
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

#[test]
fn included_file_preflight_honours_ancestor_rules_without_reading_unrelated_ones() {
    let place = tempfile::tempdir().expect("place");
    let root = place.path();
    std::fs::create_dir_all(root.join("src/deep")).expect("src");
    std::fs::create_dir_all(root.join("unrelated")).expect("unrelated");
    std::fs::write(root.join("src/deep/kept.rs"), b"pub fn kept() {}\n").expect("kept");
    std::fs::write(root.join("src/deep/secret.rs"), b"pub fn secret() {}\n").expect("secret");
    std::fs::write(root.join("src/.gitignore"), b"deep/secret.rs\n").expect("ancestor rule");
    // A rule file outside the target's ancestry cannot change its decision; it must not be needed.
    std::fs::write(root.join("unrelated/.gitignore"), b"/src/deep/kept.rs\n")
        .expect("unrelated rule");
    assert_eq!(
        read_included_file(root, "src/deep/kept.rs")
            .expect("kept")
            .as_deref(),
        Some(&b"pub fn kept() {}\n"[..])
    );
    assert_eq!(
        read_included_file(root, "src/deep/secret.rs").expect("secret"),
        None
    );
}

#[cfg(unix)]
#[test]
fn explicit_file_boundary_resolves_ancestors_and_keeps_leaf_and_size_guards() {
    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("input"), b"body").unwrap();
    let linked = temp.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    assert_eq!(
        read_explicit_file(&linked.join("input"), 4).unwrap(),
        b"body"
    );
    assert_eq!(
        read_explicit_file(&linked.join("input"), 3)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::FileTooLarge
    );
    std::os::unix::fs::symlink(real.join("input"), real.join("leaf")).unwrap();
    assert!(read_explicit_file(&linked.join("leaf"), 4).is_err());
}
