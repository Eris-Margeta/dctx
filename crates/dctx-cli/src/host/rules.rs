// SPDX-License-Identifier: Apache-2.0
//! Project ignore and omit rule loading for the facade selection policy.
//! The session entry points that use these helpers stay in the host module.

use super::inventory::{PreparedSource, RuleWalk};
use super::*;

/// Nested `.gitignore` walk stops after this many directory levels.
const MAX_RULE_DEPTH: u32 = 256;
const MAX_RULE_BYTES: usize = 8 * 1024 * 1024;
const MAX_RULE_ENTRIES: u64 = 100_000;

/// Ignore and omit texts for `root`. `DCTX_INDEX_IGNORED=1` loads omit texts only.
///
/// Local ignore order is the root `.gitignore`, then nested `.gitignore` files rewritten
/// to root-relative lines, then `.dctxignore`. An excluded directory is not listed.
pub(crate) fn project_rules(root: &Path) -> Result<RuleSources, String> {
    project_rules_scoped(root, None)
}

/// Prepares the single listing/stat traversal consumed by the first snapshot.
pub(super) fn project_rules_inventory(
    root: &Path,
    source: FsSource,
    scope: Option<&str>,
) -> Result<(PreparedSource, RuleSources), String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let config = canonical_config_dir()?;
    let global_omit = optional_omit(config.as_ref().map(|dir| dir.join("omitcontent")));
    let global_ignore = if index_ignored() {
        None
    } else {
        required_ignore(config.map(|dir| dir.join("ignore")))?
    };
    let mut walk = RuleWalk::new(&source);
    let local_ignore = if index_ignored() {
        None
    } else {
        match scope {
            Some(scope) => scoped_ignore_walk(&root, scope, global_ignore.as_deref(), &mut walk)?,
            None => local_ignore_walk(&root, global_ignore.as_deref(), &mut walk)?,
        }
    };
    let directories = std::mem::take(&mut walk.directories);
    drop(walk);
    Ok((
        PreparedSource::new(source, directories),
        RuleSources {
            global_ignore,
            global_omit,
            local_ignore,
            local_omit: None,
        },
    ))
}

/// Loads only ancestor rule files and the selected subtree's nested rules.
pub(crate) fn project_rules_scoped(
    root: &Path,
    scope: Option<&str>,
) -> Result<RuleSources, String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let config = canonical_config_dir()?;
    let global_omit = optional_omit(config.as_ref().map(|dir| dir.join("omitcontent")));
    let local_omit = None;
    if index_ignored() {
        return Ok(RuleSources {
            global_ignore: None,
            global_omit,
            local_ignore: None,
            local_omit,
        });
    }
    let global_ignore = required_ignore(config.map(|dir| dir.join("ignore")))?;
    let local_ignore = match scope {
        Some(scope) => scoped_ignore_text(&root, scope, global_ignore.as_deref())?,
        None => local_ignore_text(&root, global_ignore.as_deref())?,
    };
    Ok(RuleSources {
        global_ignore,
        global_omit,
        local_ignore,
        local_omit,
    })
}

/// Local ignore text for `root`, or `None` when every rule file is missing or empty.
/// `global_ignore` is consulted only so an already-excluded directory is not listed.
pub(crate) fn local_ignore_text(
    root: &Path,
    global_ignore: Option<&str>,
) -> Result<Option<String>, String> {
    let source = FsSource::open(root).map_err(|error| error.to_string())?;
    let mut walk = RuleWalk::new(&source);
    local_ignore_walk(root, global_ignore, &mut walk)
}

fn local_ignore_walk(
    root: &Path,
    global_ignore: Option<&str>,
    walk: &mut RuleWalk<'_>,
) -> Result<Option<String>, String> {
    let mut local = String::new();
    if let Some(text) = read_local_rule(root, ".gitignore")? {
        append_text(&mut local, &text)?;
    }
    collect_nested(root, "", &mut local, global_ignore, 0, &mut 0, walk)?;
    if let Some(text) = read_local_rule(root, ".dctxignore")? {
        append_text(&mut local, &text)?;
    }
    if local.is_empty() {
        Ok(None)
    } else {
        Ok(Some(local))
    }
}

fn scoped_ignore_text(
    root: &Path,
    scope: &str,
    global_ignore: Option<&str>,
) -> Result<Option<String>, String> {
    let source = FsSource::open(root).map_err(|error| error.to_string())?;
    let mut walk = RuleWalk::new(&source);
    scoped_ignore_walk(root, scope, global_ignore, &mut walk)
}

fn scoped_ignore_walk(
    root: &Path,
    scope: &str,
    global_ignore: Option<&str>,
    walk: &mut RuleWalk<'_>,
) -> Result<Option<String>, String> {
    let scope = dctx_core::ids::RelativePath::parse(scope).map_err(|error| error.to_string())?;
    let mut local = String::new();
    if let Some(text) = read_local_rule(root, ".gitignore")? {
        append_text(&mut local, &text)?;
    }
    let mut relative = String::new();
    for (depth, name) in scope.as_str().split('/').enumerate() {
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(name);
        let path = root.join(&relative);
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(format!("{relative}: {error}")),
        };
        // The source adapter also refuses links. Never read rule files through them.
        if meta.file_type().is_symlink() || !meta.is_dir() {
            break;
        }
        if depth > MAX_RULE_DEPTH as usize
            || directory_excluded(global_ignore, &local, &relative, name)
        {
            break;
        }
        if relative == scope.as_str() {
            collect_nested(
                root,
                &relative,
                &mut local,
                global_ignore,
                u32::try_from(depth + 1).unwrap_or(u32::MAX),
                &mut 0,
                walk,
            )?;
        } else if let Some(text) = read_local_rule(root, &format!("{relative}/.gitignore"))? {
            append_nested(&mut local, &relative, &text)?;
        }
    }
    if let Some(text) = read_local_rule(root, ".dctxignore")? {
        append_text(&mut local, &text)?;
    }
    Ok((!local.is_empty()).then_some(local))
}

fn index_ignored() -> bool {
    std::env::var("DCTX_INDEX_IGNORED").ok().as_deref() == Some("1")
}

fn canonical_config_dir() -> Result<Option<PathBuf>, String> {
    let Some(dir) = config_dir_from_env(cfg!(windows), |key| std::env::var_os(key))? else {
        return Ok(None);
    };
    // Resolve the global configuration boundary, never the rule file itself.
    match fs::canonicalize(&dir) {
        Ok(dir) => Ok(Some(dir)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", dir.display())),
    }
}

/// Pure resolver so Windows mappings can be checked on every test platform.
fn config_dir_from_env(
    windows: bool,
    env: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Result<Option<PathBuf>, String> {
    let nonempty = |key| {
        env(key)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let base = if windows {
        Some(nonempty("APPDATA").ok_or_else(|| "APPDATA is not set or is empty".to_string())?)
    } else {
        nonempty("XDG_CONFIG_HOME").or_else(|| nonempty("HOME").map(|home| home.join(".config")))
    };
    Ok(base.map(|path| path.join(APP_NAME)))
}

fn required_ignore(path: Option<PathBuf>) -> Result<Option<String>, String> {
    match path {
        Some(path) => read_rule_file(&path),
        None => Ok(None),
    }
}

fn optional_omit(path: Option<PathBuf>) -> Option<String> {
    path.and_then(|path| read_rule_file(&path).ok().flatten())
}

fn read_rule_file(path: &Path) -> Result<Option<String>, String> {
    use dctx_adapter_fs::{SourceFs, StdFs};
    StdFs
        .read_policy(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Reads one rule file inside the place, named by its place-relative path.
/// A link, directory, FIFO or other non-regular entry is skipped with a warning, as if absent:
/// rule files are never read through links. Errors name the relative path, never the root.
fn read_local_rule(root: &Path, relative: &str) -> Result<Option<String>, String> {
    use dctx_adapter_fs::{SourceFs, StdFs};
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            warn_skipped_rule(relative);
            return Ok(None);
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{relative}: {error}")),
    }
    StdFs
        .read_policy(&path)
        .map_err(|error| format!("{relative}: {error}"))
}

/// One stderr warning per skipped rule path and process (a server loads rules many times).
fn warn_skipped_rule(relative: &str) {
    static WARNED: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    let first = WARNED
        .lock()
        .map_or(true, |mut warned| warned.insert(relative.to_string()));
    if first {
        eprintln!("{}", skipped_rule_warning(relative));
    }
}

fn skipped_rule_warning(relative: &str) -> String {
    format!("warning: skipped rule file {relative}: it is a link or not a regular file, so its rules do not apply")
}

fn collect_nested(
    root: &Path,
    prefix: &str,
    rules: &mut String,
    global_ignore: Option<&str>,
    depth: u32,
    visited: &mut u64,
    walk: &mut RuleWalk<'_>,
) -> Result<(), String> {
    if depth > MAX_RULE_DEPTH {
        // The last directory's rule was historically loaded before stopping
        // descent. Preserve that boundary without listing below the rule cap.
        if let Some(text) = read_local_rule(root, &format!("{prefix}/.gitignore"))? {
            append_nested(rules, prefix, &text)?;
        }
        return Ok(());
    }
    // Errors name the place-relative directory, never the absolute root.
    let dir = if prefix.is_empty() { "." } else { prefix };
    let mut names = Vec::new();
    let relative =
        dctx_core::ids::RelativePath::parse(prefix).map_err(|error| error.to_string())?;
    let entries = walk
        .entries(&relative)
        .map_err(|error| {
            let entries = matches!(&error, dctx_core::error::DctxError::ResourceLimit { resource: dctx_core::error::Resource::Entries, .. });
            let error = DctxError::from_core(error);
            if entries {
                format!("rule walk {dir}: {error}; raise DCTX_MAX_ENTRIES (1..100000000) or choose a smaller place")
            } else { format!("rule walk {dir}: {error}") }
        })?;
    // The inventory also discovers rule leaves. Missing rules need no separate
    // root-to-leaf open, and links remain visible here and refused by the reader.
    if !prefix.is_empty() && entries.iter().any(|entry| entry.name == ".gitignore") {
        if let Some(text) = read_local_rule(root, &format!("{prefix}/.gitignore"))? {
            append_nested(rules, prefix, &text)?;
        }
    }
    for entry in entries {
        *visited += 1;
        if *visited > MAX_RULE_ENTRIES {
            return Err("rule walk exceeds 100000 entries".into());
        }
        if entry.is_dir() && relative.child(&entry.name).is_ok() {
            names.push(entry.name);
        }
    }
    names.sort();
    for name in names {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if walk.excluded(global_ignore, rules, &relative, &name) {
            continue;
        }
        collect_nested(
            root,
            &relative,
            rules,
            global_ignore,
            depth + 1,
            visited,
            walk,
        )?;
    }
    Ok(())
}

fn directory_excluded(
    global_ignore: Option<&str>,
    local_so_far: &str,
    relative: &str,
    name: &str,
) -> bool {
    let local_ignore = if local_so_far.is_empty() {
        None
    } else {
        Some(local_so_far)
    };
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        global_ignore,
        local_ignore,
        ..RuleTexts::default()
    });
    let context = ItemContext {
        relative_path: relative,
        name,
        is_directory: true,
        debug_mode: false,
    };
    policy.decide(&context) == FileHandlingDecision::Exclude
}

pub(super) fn append_text(rules: &mut String, text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    let separator = usize::from(!rules.is_empty() && !rules.ends_with('\n'));
    let newline = usize::from(!text.ends_with('\n'));
    if rules
        .len()
        .saturating_add(text.len())
        .saturating_add(separator + newline)
        > MAX_RULE_BYTES
    {
        return Err("selection rules exceed 8388608 bytes".into());
    }
    if separator != 0 {
        rules.push('\n');
    }
    rules.push_str(text);
    if newline != 0 {
        rules.push('\n');
    }
    Ok(())
}

pub(super) fn append_nested(rules: &mut String, prefix: &str, text: &str) -> Result<(), String> {
    for line in text.lines() {
        append_text(rules, &rewrite_nested_line(prefix, line))?;
    }
    Ok(())
}

/// Rewrites one line of a nested `.gitignore` so it matches from the repository root.
/// A pattern with no slash applies anywhere under `prefix`. An anchored pattern stays under it.
pub(super) fn rewrite_nested_line(prefix: &str, line: &str) -> String {
    if prefix.is_empty() {
        return line.to_string();
    }
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return line.to_string();
    }
    let (body, comment) = match trimmed.find('#') {
        Some(pos) => (trimmed[..pos].trim_end(), Some(trimmed[pos..].trim())),
        None => (trimmed, None),
    };
    if body.is_empty() {
        return line.to_string();
    }
    let (negation, rest) = match body.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, body),
    };
    let (dir_only, rest) = match rest.strip_suffix('/') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let anchored = rest.starts_with('/');
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    if rest.is_empty() {
        return line.to_string();
    }
    let rewritten = if anchored || rest.contains('/') {
        format!("{prefix}/{rest}")
    } else {
        format!("{prefix}/**/{rest}")
    };
    let mut out = String::new();
    if negation {
        out.push('!');
    }
    out.push_str(&rewritten);
    if dir_only {
        out.push('/');
    }
    if let Some(comment) = comment {
        out.push(' ');
        out.push_str(comment);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, ffi::OsString};

    fn resolve(windows: bool, values: &[(&str, &str)]) -> Result<Option<PathBuf>, String> {
        let env: BTreeMap<_, _> = values
            .iter()
            .map(|(key, value)| (*key, OsString::from(value)))
            .collect();
        config_dir_from_env(windows, |key| env.get(key).cloned())
    }

    #[test]
    fn shared_inventory_preserves_rule_bytes_and_sequential_pruning() {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        for dir in ["a/blocked", "b", "excluded"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for (path, text) in [
            (".gitignore", "excluded/\n*.tmp\n"),
            ("a/.gitignore", "!keep.tmp\nblocked/\n"),
            ("b/.gitignore", "/local.rs\n"),
            (".dctxignore", "b/final.rs\n"),
        ] {
            fs::write(root.join(path), text).unwrap();
        }
        for path in ["excluded/.gitignore", "a/blocked/.gitignore"] {
            fs::write(root.join(path), [0xff]).unwrap();
        }
        let source = FsSource::open(&root).unwrap();
        let mut walk = RuleWalk::new(&source);
        let text = local_ignore_walk(&root, None, &mut walk).unwrap().unwrap();
        assert_eq!(
            text,
            "excluded/\n*.tmp\n!a/**/keep.tmp\na/**/blocked/\nb/local.rs\nb/final.rs\n"
        );
        assert!(walk.directories.contains_key("a"));
        assert!(walk.directories.contains_key("b"));
        assert!(!walk.directories.contains_key("excluded"));
        assert!(!walk.directories.contains_key("a/blocked"));
    }

    #[test]
    fn nested_rule_expansion_is_bounded_before_appending() {
        let mut rules = "x".repeat(MAX_RULE_BYTES - 8);
        let previous = rules.clone();
        assert!(append_nested(&mut rules, &"p".repeat(255), "x").is_err());
        assert_eq!(rules, previous);
        assert!(append_text(&mut rules, "1234567").is_err());
        assert_eq!(rules, previous);
    }

    #[test]
    fn windows_config_uses_appdata_instead_of_xdg_or_localappdata() {
        let values = [
            ("APPDATA", "config"),
            ("XDG_CONFIG_HOME", "other-config"),
            ("LOCALAPPDATA", "local-app-data"),
            ("HOME", "home"),
        ];
        assert_eq!(
            resolve(true, &values),
            Ok(Some(PathBuf::from("config").join(APP_NAME)))
        );
    }

    #[test]
    fn windows_config_refuses_missing_or_empty_appdata_without_falling_back() {
        for values in [
            vec![],
            vec![
                ("XDG_CONFIG_HOME", "config"),
                ("LOCALAPPDATA", "local-app-data"),
                ("HOME", "home"),
            ],
            vec![
                ("APPDATA", ""),
                ("XDG_CONFIG_HOME", "config"),
                ("HOME", "home"),
            ],
        ] {
            assert_eq!(
                resolve(true, &values),
                Err("APPDATA is not set or is empty".to_string())
            );
        }
    }

    #[test]
    fn unix_config_uses_xdg_then_home_and_ignores_appdata() {
        let values = [
            ("XDG_CONFIG_HOME", "config"),
            ("HOME", "home"),
            ("APPDATA", "other-config"),
        ];
        assert_eq!(
            resolve(false, &values),
            Ok(Some(PathBuf::from("config").join(APP_NAME)))
        );
        for values in [
            vec![("HOME", "home")],
            vec![("XDG_CONFIG_HOME", ""), ("HOME", "home")],
        ] {
            assert_eq!(
                resolve(false, &values),
                Ok(Some(PathBuf::from("home").join(".config").join(APP_NAME)))
            );
        }
        assert_eq!(resolve(false, &[("APPDATA", "config")]), Ok(None));
    }
    #[test]
    fn scoped_rules_inherit_ancestors_and_match_full_policy() {
        let temp =
            tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).expect("fixture");
        let root = temp.path();
        fs::create_dir_all(root.join("src/nested/deeper")).expect("dirs");
        fs::create_dir_all(root.join("sibling")).expect("sibling");
        for (path, text) in [
            (".gitignore", "*.private\nsrc/generated/\n"),
            ("src/.gitignore", "*.py\n!kept.py\n"),
            ("src/nested/.gitignore", "/blocked.rs\n!allowed.py\n"),
            ("src/nested/deeper/.gitignore", "*.rs\n"),
            (".dctxignore", "src/nested/final.rs\n"),
        ] {
            fs::write(root.join(path), text).expect("rules");
        }
        let full = local_ignore_text(root, None).expect("full");
        for scope in ["src/nested", "src/nested/kept.py", "src/nested/deeper/a.rs"] {
            let scoped = scoped_ignore_text(root, scope, None).expect("scoped");
            let policy = |text: Option<&str>| {
                SelectionPolicy::from_texts(&RuleTexts {
                    local_ignore: text,
                    ..RuleTexts::default()
                })
            };
            let full = policy(full.as_deref());
            let scoped = policy(scoped.as_deref());
            for name in [
                "kept.py",
                "blocked.rs",
                "allowed.py",
                "final.rs",
                "secret.private",
                "deeper/a.rs",
            ] {
                let relative = format!("src/nested/{name}");
                if !relative.starts_with(scope) && scope != "src/nested" {
                    continue;
                }
                let context = ItemContext {
                    relative_path: &relative,
                    name,
                    is_directory: false,
                    debug_mode: false,
                };
                assert_eq!(
                    scoped.decide(&context),
                    full.decide(&context),
                    "{scope}: {relative}"
                );
            }
        }
        // An unrelated invalid rule file proves that scoped loading doesn't visit it.
        fs::write(root.join("sibling/.gitignore"), [0xff]).expect("invalid sibling");
        assert!(local_ignore_text(root, None).is_err());
        assert!(scoped_ignore_text(root, "src/nested/kept.py", None).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn scoped_rules_never_follow_symlinked_ancestors() {
        let temp =
            tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).expect("fixture");
        let root = temp.path().join("place");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).expect("place");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(outside.join(".gitignore"), [0xff]).expect("invalid outside rule");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("link");
        assert!(scoped_ignore_text(&root, "link/file.py", None).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn linked_and_non_regular_rule_files_are_skipped_without_the_root_path() {
        let temp =
            tempfile::tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap()).expect("fixture");
        let root = temp.path().join("place");
        let outside = temp.path().join("outside");
        fs::create_dir_all(root.join("sub/.gitignore")).expect("directory rule");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(outside.join("rules"), "secret-outside-rule\n").expect("outside rule");
        std::os::unix::fs::symlink(outside.join("rules"), root.join(".gitignore")).expect("link");
        fs::write(root.join(".dctxignore"), "kept-rule\n").expect("dctxignore");
        let text = local_ignore_text(&root, None)
            .expect("links and directories are skipped")
            .expect("regular rules still load");
        assert!(text.contains("kept-rule"), "{text}");
        assert!(!text.contains("secret-outside-rule"), "{text}");
        let scoped = scoped_ignore_text(&root, "sub/file.py", None).expect("scoped skip");
        assert!(!scoped.unwrap_or_default().contains("secret-outside-rule"));
        let warning = skipped_rule_warning("sub/.gitignore");
        assert!(warning.starts_with("warning: skipped rule file sub/.gitignore"));
        // An unreadable rule still fails, and the error names only the relative path.
        fs::remove_file(root.join(".gitignore")).expect("unlink");
        fs::write(root.join(".gitignore"), [0xff]).expect("invalid rule");
        let error = local_ignore_text(&root, None).unwrap_err();
        assert!(error.starts_with(".gitignore: "), "{error}");
        assert!(!error.contains(&root.display().to_string()), "{error}");
    }
}
