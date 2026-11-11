// SPDX-License-Identifier: Apache-2.0
//! Bounded literal import fallback. No ambient search paths or recursive sibling scan.
use super::*;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const FILE_BYTES: u64 = 64 * 1024;
const MAX_TIME: Duration = Duration::from_millis(100);

struct Scan {
    started: Instant,
    files: usize,
    bytes: usize,
    entries: usize,
}
impl Scan {
    fn available(&self) -> bool {
        self.files < MAX_FILES
            && self.bytes < MAX_BYTES
            && self.entries < 2048
            && self.started.elapsed() < MAX_TIME
    }
    fn read(&mut self, root: &Path, relative: &str) -> Option<String> {
        if !self.available() {
            return None;
        }
        self.files += 1;
        // Policy discovery is scoped to this file, never to the sibling tree.
        let bytes = read_included_file_bounded(
            root,
            relative,
            FILE_BYTES.min((MAX_BYTES - self.bytes) as u64),
        )
        .ok()??;
        self.bytes += bytes.len();
        String::from_utf8(bytes).ok()
    }
}

/// Appends outside definitions and actionable hints, or adds `defined_outside_index` in JSON.
/// Only literal local imports below the place's parent are eligible; denied paths never appear.
#[must_use]
pub fn outside_notice(
    place: &Path,
    also: &[PathBuf],
    target: &str,
    page: String,
    json: bool,
    references: bool,
) -> String {
    let Ok(place) = fs::canonicalize(place) else {
        return page;
    };
    let Some(boundary) = place.parent() else {
        return page;
    };
    let mut scan = Scan {
        started: Instant::now(),
        files: 0,
        bytes: 0,
        entries: 0,
    };
    let mut candidates = BTreeSet::new();
    imports(&place, &place, &mut scan, &mut candidates, 0);
    let indexed: Vec<_> = also
        .iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .chain(std::iter::once(place.clone()))
        .collect();
    let name = target
        .rsplit_once('#')
        .map_or(target, |v| v.1)
        .split('~')
        .next()
        .unwrap_or(target);
    let Ok(rules) = rules::project_rules_scoped(&place, Some("__outside_probe__")) else {
        return page;
    };
    let policy = rules.policy();
    let mut notices = BTreeSet::new();
    for (candidate, imported) in candidates {
        if !scan.available() {
            break;
        }
        for candidate in variants(&candidate) {
            if !scan.available() {
                break;
            }
            let Some(candidate) = lexical(&candidate) else {
                continue;
            };
            if indexed.iter().any(|root| candidate.starts_with(root)) {
                continue;
            }
            let Ok(relative) = candidate.strip_prefix(boundary) else {
                continue;
            };
            let Some(relative) = relative.to_str() else {
                continue;
            };
            if !safe_components(boundary, relative, &policy) {
                continue;
            }
            let Ok(parent_rules) = rules::project_rules_scoped(boundary, Some(relative)) else {
                continue;
            };
            if !safe_components(boundary, relative, &parent_rules.policy()) {
                continue;
            }
            if !sibling_policy(boundary, relative, &mut scan) {
                continue;
            }
            let Some(text) = scan.read(boundary, relative) else {
                continue;
            };
            let matched = defines(&text, name)
                || (references && imported.iter().any(|name| defines(&text, name)));
            if !matched {
                continue;
            }
            let shown = format!("../{}", relative.replace('\\', "/"));
            let Some(parent) = candidate.parent() else {
                continue;
            };
            if let Some(directory) = invocation_relative(parent) {
                notices.insert((shown, shell_directory(&directory)));
            }
        }
    }
    if notices.is_empty() {
        return page;
    }
    if json {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&page) else {
            return page;
        };
        value["defined_outside_index"] = serde_json::Value::Array(notices.into_iter().map(|(path, directory)| {
            serde_json::json!({"path": path, "hint": format!("add it with --also {directory}")})
        }).collect());
        return serde_json::to_string(&value).map_or(page, |text| format!("{text}\n"));
    }
    let mut page = page;
    for (path, directory) in notices {
        page.push_str(&format!(
            "defined outside index: {path}\nadd it with --also {directory}\n"
        ));
    }
    page
}

// Read sibling policy through the ancestor's confined source, never by reopening
// an untrusted sibling as a canonical root (which could follow a swapped link).
fn sibling_policy(boundary: &Path, relative: &str, scan: &mut Scan) -> bool {
    let Some((label, rest)) = relative.split_once('/') else {
        return true;
    };
    let rule = format!("{label}/.dctxignore");
    match fs::symlink_metadata(boundary.join(&rule)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
        _ => return false,
    }
    let Some(text) = scan.read(boundary, &rule) else {
        return false;
    };
    let policy = SelectionPolicy::from_texts(&RuleTexts {
        local_ignore: Some(&text),
        ..RuleTexts::default()
    });
    safe_components(&boundary.join(label), rest, &policy)
}

fn imports(
    root: &Path,
    dir: &Path,
    scan: &mut Scan,
    out: &mut BTreeSet<(PathBuf, Vec<String>)>,
    depth: usize,
) {
    if depth > 16 || !scan.available() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let remaining = 2048 - scan.entries;
    let mut entries = entries
        .take(remaining + 1)
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    if entries.len() > remaining {
        return;
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let relative_dir = dir
        .strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .unwrap_or("");
    let scope = format!("{relative_dir}/__outside_probe__");
    let scope = scope.trim_start_matches('/');
    let Ok(rules) = rules::project_rules_scoped(root, Some(scope)) else {
        return;
    };
    let policy = rules.policy();
    for entry in entries {
        scan.entries += 1;
        if !scan.available() {
            break;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Some(relative) = relative.to_str() else {
            continue;
        };
        if !safe_components(root, relative, &policy) {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            imports(root, &path, scan, out, depth + 1);
            continue;
        }
        if !kind.is_file() || !source_file(&path) {
            continue;
        }
        let Some(text) = scan.read(root, relative) else {
            continue;
        };
        for line in text.lines() {
            if out.len() >= MAX_FILES || !scan.available() {
                return;
            }
            let line = line.trim();
            if !import_line(line) {
                continue;
            }
            let names = line
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .filter(|s| !s.is_empty())
                .take(128)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            for quote in ['\'', '"'] {
                let mut parts = line.split(quote).skip(1);
                while let Some(literal) = parts.next() {
                    if out.len() >= MAX_FILES {
                        return;
                    }
                    if literal.len() <= 4096
                        && (literal.starts_with('.') || Path::new(literal).is_absolute())
                        && !literal.contains(['\0', '\n', '\r'])
                    {
                        out.insert((dir.join(literal), names.clone()));
                    }
                    parts.next();
                }
            }
            if let Some(module) = line
                .strip_prefix("from ")
                .and_then(|rest| rest.split_whitespace().next())
            {
                let module = module.trim_start_matches('.').replace('.', "/");
                if !module.is_empty() {
                    out.insert((root.parent().unwrap_or(root).join(module), names.clone()));
                }
            }
            if let Some(module) = line
                .strip_prefix("use ")
                .and_then(|rest| rest.split("::").next())
            {
                if !matches!(module, "crate" | "self" | "super" | "std" | "core") {
                    out.insert((root.parent().unwrap_or(root).join(module), names));
                }
            }
        }
    }
}

fn import_line(line: &str) -> bool {
    line.starts_with("import ")
        || line.starts_with("from ")
        || line.starts_with("use ")
        || line.starts_with("pub use ")
        || line.starts_with("#include")
        || line.starts_with("#[path")
        || line.contains("path =")
        || line.contains("require(")
        || (line.starts_with("export ") && line.contains(" from "))
}
fn source_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e,
            "rs" | "py"
                | "js"
                | "jsx"
                | "ts"
                | "tsx"
                | "mjs"
                | "cjs"
                | "c"
                | "h"
                | "cpp"
                | "hpp"
                | "go"
                | "rb"
                | "toml"
        )
    })
}
fn variants(path: &Path) -> Vec<PathBuf> {
    let mut paths = vec![path.to_path_buf()];
    if path.extension().is_none() {
        for extension in ["py", "rs", "ts", "js", "h"] {
            paths.push(path.with_extension(extension));
        }
        for leaf in [
            "__init__.py",
            "index.ts",
            "index.js",
            "mod.rs",
            "src/lib.rs",
        ] {
            paths.push(path.join(leaf));
        }
    }
    paths
}
fn lexical(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}
fn safe_components(root: &Path, relative: &str, policy: &SelectionPolicy) -> bool {
    if dctx_core::filter::has_git_segment(relative)
        || dctx_core::filter::is_secret_shaped_name(relative.rsplit('/').next().unwrap_or(relative))
    {
        return false;
    }
    let Ok(relative) = dctx_core::ids::RelativePath::parse(relative) else {
        return false;
    };
    let mut path = root.to_path_buf();
    let mut shown = String::new();
    for name in relative.as_str().split('/') {
        path.push(name);
        if !shown.is_empty() {
            shown.push('/');
        }
        shown.push_str(name);
        // Decide before stat, then refuse every symlink component.
        if policy.decide(&ItemContext {
            relative_path: &shown,
            name,
            is_directory: shown != relative.as_str(),
            debug_mode: false,
        }) != FileHandlingDecision::Include
        {
            return false;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            return false;
        };
        if meta.file_type().is_symlink() || (!meta.is_file() && !meta.is_dir()) {
            return false;
        }
        if meta.is_dir()
            && policy.decide(&ItemContext {
                relative_path: &shown,
                name,
                is_directory: true,
                debug_mode: false,
            }) != FileHandlingDecision::Include
        {
            return false;
        }
    }
    true
}
fn defines(text: &str, name: &str) -> bool {
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return false;
    }
    text.lines().any(|line| {
        let words = line
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        words.windows(2).any(|pair| {
            pair[1] == name
                && matches!(
                    pair[0],
                    "fn" | "func"
                        | "def"
                        | "class"
                        | "struct"
                        | "enum"
                        | "trait"
                        | "type"
                        | "const"
                        | "let"
                        | "function"
                        | "interface"
                        | "mod"
                        | "int"
                        | "void"
                        | "bool"
                        | "char"
                        | "double"
                        | "float"
                )
        })
    })
}

fn invocation_relative(path: &Path) -> Option<String> {
    let current = std::env::current_dir().ok()?;
    let current = current.components().collect::<Vec<_>>();
    let target = path.components().collect::<Vec<_>>();
    let common = current
        .iter()
        .zip(&target)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in common..current.len() {
        relative.push("..");
    }
    for component in &target[common..] {
        relative.push(component.as_os_str());
    }
    if relative.as_os_str().is_empty() {
        relative.push(".");
    }
    Some(relative.to_str()?.replace('\\', "/"))
}

fn shell_directory(directory: &str) -> String {
    if directory
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
    {
        directory.to_string()
    } else {
        format!("'{}'", directory.replace('\'', "'\\''"))
    }
}
