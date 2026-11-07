// SPDX-License-Identifier: Apache-2.0
//! Project ignore and omit rule loading for the facade selection policy.
//! The session entry points that use these helpers stay in the host module.

use super::*;

/// One rule file. A larger file is refused, matching the desktop loader.
const MAX_RULE_FILE_BYTES: u64 = 1_048_576;
/// Nested `.gitignore` walk stops after this many directory levels.
const MAX_RULE_DEPTH: u32 = 256;

/// Ignore and omit texts for `root`. `DCTX_INDEX_IGNORED=1` loads omit texts only.
///
/// Local ignore order is the root `.gitignore`, then nested `.gitignore` files rewritten
/// to root-relative lines, then `.dctxignore`. An excluded directory is not listed.
pub(crate) fn project_rules(root: &Path) -> Result<RuleSources, String> {
    project_rules_scoped(root, None)
}

/// Loads only ancestor rule files and the selected subtree's nested rules.
pub(crate) fn project_rules_scoped(
    root: &Path,
    scope: Option<&str>,
) -> Result<RuleSources, String> {
    let root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let global_omit = optional_omit(config_file("omitcontent")?);
    let local_omit = None;
    if index_ignored() {
        return Ok(RuleSources {
            global_ignore: None,
            global_omit,
            local_ignore: None,
            local_omit,
        });
    }
    let global_ignore = required_ignore(config_file("ignore")?)?;
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
    let mut local = String::new();
    if let Some(text) = read_rule_file(&root.join(".gitignore"))? {
        append_text(&mut local, &text);
    }
    collect_nested(root, "", &mut local, global_ignore, 0)?;
    if let Some(text) = read_rule_file(&root.join(".dctxignore"))? {
        append_text(&mut local, &text);
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
    let scope = dctx_core::ids::RelativePath::parse(scope).map_err(|error| error.to_string())?;
    let mut local = String::new();
    if let Some(text) = read_rule_file(&root.join(".gitignore"))? {
        append_text(&mut local, &text);
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
            Err(error) => return Err(format!("{}: {error}", path.display())),
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
        if let Some(text) = read_rule_file(&path.join(".gitignore"))? {
            append_nested(&mut local, &relative, &text);
        }
        if relative == scope.as_str() {
            collect_nested(
                root,
                &relative,
                &mut local,
                global_ignore,
                u32::try_from(depth + 1).unwrap_or(u32::MAX),
            )?;
        }
    }
    if let Some(text) = read_rule_file(&root.join(".dctxignore"))? {
        append_text(&mut local, &text);
    }
    Ok((!local.is_empty()).then_some(local))
}

fn index_ignored() -> bool {
    std::env::var("DCTX_INDEX_IGNORED").ok().as_deref() == Some("1")
}

fn config_file(name: &str) -> Result<Option<PathBuf>, String> {
    config_dir().map(|dir| dir.map(|dir| dir.join(name)))
}

fn config_dir() -> Result<Option<PathBuf>, String> {
    config_dir_from_env(cfg!(windows), |key| std::env::var_os(key))
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
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("{}: is a symlink", path.display()));
    }
    if !meta.is_file() {
        return Err(format!("{}: is not a file", path.display()));
    }
    if meta.len() > MAX_RULE_FILE_BYTES {
        return Err(format!(
            "{}: exceeds {MAX_RULE_FILE_BYTES} bytes",
            path.display()
        ));
    }
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut buf = Vec::new();
    file.take(MAX_RULE_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if buf.len() as u64 > MAX_RULE_FILE_BYTES {
        return Err(format!(
            "{}: exceeds {MAX_RULE_FILE_BYTES} bytes",
            path.display()
        ));
    }
    let text =
        std::str::from_utf8(&buf).map_err(|_| format!("{}: invalid UTF-8", path.display()))?;
    Ok(Some(text.to_string()))
}

fn collect_nested(
    root: &Path,
    prefix: &str,
    rules: &mut String,
    global_ignore: Option<&str>,
    depth: u32,
) -> Result<(), String> {
    if depth > MAX_RULE_DEPTH {
        return Ok(());
    }
    let dir = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let mut names = Vec::new();
    let entries =
        fs::read_dir(&dir).map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("rule walk {}: {error}", dir.display()))?;
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        names.push(name);
    }
    names.sort();
    for name in names {
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if directory_excluded(global_ignore, rules, &relative, &name) {
            continue;
        }
        if let Some(text) = read_rule_file(&root.join(&relative).join(".gitignore"))? {
            append_nested(rules, &relative, &text);
        }
        collect_nested(root, &relative, rules, global_ignore, depth + 1)?;
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

pub(super) fn append_text(rules: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !rules.is_empty() && !rules.ends_with('\n') {
        rules.push('\n');
    }
    rules.push_str(text);
    if !rules.ends_with('\n') {
        rules.push('\n');
    }
}

pub(super) fn append_nested(rules: &mut String, prefix: &str, text: &str) {
    for line in text.lines() {
        if !rules.is_empty() && !rules.ends_with('\n') {
            rules.push('\n');
        }
        rules.push_str(&rewrite_nested_line(prefix, line));
        rules.push('\n');
    }
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
        let temp = tempfile::tempdir().expect("fixture");
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
        let temp = tempfile::tempdir().expect("fixture");
        let root = temp.path().join("place");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&root).expect("place");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(outside.join(".gitignore"), [0xff]).expect("invalid outside rule");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("link");
        assert!(scoped_ignore_text(&root, "link/file.py", None).is_ok());
    }
}
