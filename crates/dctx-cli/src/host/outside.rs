// SPDX-License-Identifier: Apache-2.0
//! Bounded literal import fallback. No ambient search paths or recursive sibling scan.
use super::*;
use dctx::{Inventory, MapRequest, SnapshotHandle, VecSink, Verb};
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_FILES: usize = 256;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const FILE_BYTES: u64 = 64 * 1024;
const MAX_TIME: Duration = Duration::from_millis(100);

struct Scan {
    started: Instant,
    files: usize,
    bytes: usize,
}
impl Scan {
    fn available(&self) -> bool {
        self.files < MAX_FILES && self.bytes < MAX_BYTES && self.started.elapsed() < MAX_TIME
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

/// Literal imports captured from source bytes the engine already requested.
/// The fallback has no inventory traversal and never reopens an indexed source.
#[derive(Clone, Default)]
pub struct Imports(Arc<Mutex<ImportFiles>>);
#[derive(Default)]
struct ImportFiles {
    files: BTreeMap<String, BTreeSet<(PathBuf, Vec<String>)>>,
    bytes: usize,
}

pub(super) struct ImportSource<S> {
    pub source: S,
    pub imports: Option<Imports>,
    pub root: PathBuf,
    pub prefix: String,
}
impl<S: Source> Source for ImportSource<S> {
    fn inventory(&self) -> Option<Box<dyn Inventory + '_>> {
        self.source.inventory()
    }
    fn invalidate_inventory(&self) {
        self.source.invalidate_inventory();
    }
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.source.root_kind()
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        self.source.entries(dir)
    }
    fn kind(&self, path: &RelativePath) -> Result<NodeType, CoreError> {
        self.source.kind(path)
    }
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.source.metadata(path)
    }
    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
        self.source.stat(path)
    }
    fn read(&self, grant: &ReadGrant, cap: u64) -> Result<Vec<u8>, CoreError> {
        let bytes = self.source.read(grant, cap)?;
        let Some(observed) = &self.imports else {
            return Ok(bytes);
        };
        if bytes.len() <= FILE_BYTES as usize && source_file(Path::new(grant.path().as_str())) {
            if let (Ok(text), Ok(mut captured)) = (std::str::from_utf8(&bytes), observed.0.lock()) {
                let relative = grant.path().as_str();
                let path = format!("{}{relative}", self.prefix);
                if !captured.files.contains_key(&path)
                    && captured.files.len() < MAX_FILES
                    && captured.bytes + bytes.len() <= MAX_BYTES
                {
                    let mut candidates = BTreeSet::new();
                    imports(&self.root, &self.root.join(relative), text, &mut candidates);
                    captured.bytes += bytes.len();
                    captured.files.insert(path, candidates);
                }
            }
        }
        Ok(bytes)
    }
}

impl Imports {
    /// Selects missing imported names using metadata-only index queries.
    #[must_use]
    pub fn unresolved(
        &self,
        session: &mut Session,
        handle: &SnapshotHandle,
        path: Option<&str>,
        scope: Option<&str>,
    ) -> Vec<(PathBuf, Vec<String>)> {
        let mut checked = BTreeMap::new();
        self.candidates(path)
            .into_iter()
            .filter_map(|(path, names)| {
                let names: Vec<_> = names
                    .into_iter()
                    .filter(|name| {
                        let present = if let Some(present) = checked.get(name) {
                            *present
                        } else if checked.len() >= MAX_FILES {
                            true
                        } else {
                            let present = indexed_name(session, handle, name, scope);
                            checked.insert(name.clone(), present);
                            present
                        };
                        !present
                    })
                    .collect();
                (!names.is_empty()).then_some((path, names))
            })
            .collect()
    }

    /// Returns the captured imports of one resolved file, or all files for a missing name.
    #[must_use]
    pub fn candidates(&self, path: Option<&str>) -> Vec<(PathBuf, Vec<String>)> {
        self.0.lock().map_or_else(
            |_| Vec::new(),
            |captured| {
                captured
                    .files
                    .iter()
                    .filter(|(file, _)| path.is_none_or(|path| path == file.as_str()))
                    .flat_map(|(_, candidates)| candidates.iter().cloned())
                    .take(MAX_FILES)
                    .collect()
            },
        )
    }
}

/// Tests only indexed definitions. Excluding text disables the engine's literal-read fallback.
/// A scoped handle can check names in its selected file without expanding the snapshot.
#[must_use]
pub fn indexed_name(
    session: &mut Session,
    handle: &SnapshotHandle,
    name: &str,
    scope: Option<&str>,
) -> bool {
    let mut request = MapRequest::new(Verb::Find);
    request.query = name.to_owned();
    request.within = scope.map(str::to_owned);
    request.kinds = [
        "fn",
        "method",
        "struct",
        "enum",
        "trait",
        "impl",
        "const",
        "type",
        "class",
        "interface",
        "macro",
        "heading",
        "test",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    session
        .query(handle, &request, &mut VecSink::new())
        .map_or(true, |answer| {
            answer.records.iter().any(|record| {
                record
                    .id
                    .rsplit_once('#')
                    .is_some_and(|(_, symbol)| symbol.split('~').next() == Some(name))
            })
        })
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
    candidates: &[(PathBuf, Vec<String>)],
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
    };
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
    for (candidate, names) in candidates {
        if !scan.available() {
            break;
        }
        for candidate in variants(candidate) {
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
            let matched = if name.is_empty() {
                names.iter().any(|name| defines(&text, name))
            } else {
                defines(&text, name)
            };
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

fn imports(root: &Path, path: &Path, text: &str, out: &mut BTreeSet<(PathBuf, Vec<String>)>) {
    let Some(dir) = path.parent() else {
        return;
    };
    for line in text.lines() {
        if out.len() >= MAX_FILES {
            return;
        }
        let line = line.trim();
        if !import_line(line) {
            continue;
        }
        let names_line = if line.starts_with("from ") {
            line.split_once(" import ").map_or("", |(_, names)| names)
        } else if line.starts_with("import ") || line.starts_with("export ") {
            line.split(" from ").next().unwrap_or(line)
        } else {
            line.rsplit("::").next().unwrap_or(line)
        };
        let names = names_line
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| {
                !s.is_empty() && !matches!(*s, "import" | "export" | "from" | "use" | "pub" | "as")
            })
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
        if meta.file_type().is_symlink()
            || (!meta.is_file() && !meta.is_dir())
            || (shown == relative.as_str() && !meta.is_file())
        {
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
