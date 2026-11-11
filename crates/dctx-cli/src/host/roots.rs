// SPDX-License-Identifier: Apache-2.0
//! Read-only union of explicitly selected roots, each with its own selection policy.
use super::*;
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, EntryStamp, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

/// Removes repeatable root options before a command's own argument validation.
/// # Errors
/// Refuses missing values and more than sixteen additional roots.
pub fn also_arguments(args: &[String]) -> Result<(Vec<String>, Vec<PathBuf>), String> {
    let mut words = Vec::new();
    let mut roots = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            words.push(arg.clone());
            words.extend(args.cloned());
            break;
        }
        if arg == "--also" {
            let value = args
                .next()
                .filter(|v| !v.is_empty())
                .ok_or("--also requires a directory")?;
            roots.push(PathBuf::from(value));
            if roots.len() > 16 {
                return Err("--also accepts at most sixteen roots".into());
            }
        } else {
            words.push(arg.clone());
            // Opaque values must not be interpreted as root flags.
            if matches!(
                arg.as_str(),
                "--place"
                    | "--for"
                    | "--budget"
                    | "--session"
                    | "--cursor"
                    | "--in"
                    | "--output"
                    | "-o"
                    | "--include"
                    | "--exclude"
                    | "--kind"
                    | "--edge"
                    | "--dir"
                    | "--lines"
                    | "--rev"
                    | "--limit"
                    | "--offset"
                    | "--depth"
                    | "--format"
                    | "--ext"
                    | "--large"
                    | "--small"
                    | "--tree-stamps"
            ) {
                if let Some(value) = args.next() {
                    words.push(value.clone());
                }
            }
        }
    }
    Ok((words, roots))
}

struct Root {
    label: String,
    source: inventory::PreparedSource,
    policy: SelectionPolicy,
}

impl Root {
    fn allowed(&self, path: &RelativePath, directory: bool) -> bool {
        let mut relative = String::new();
        for name in path.as_str().split('/').filter(|s| !s.is_empty()) {
            if !relative.is_empty() {
                relative.push('/');
            }
            relative.push_str(name);
            if self.policy.decide(&ItemContext {
                relative_path: &relative,
                name,
                is_directory: relative != path.as_str() || directory,
                debug_mode: false,
            }) != FileHandlingDecision::Include
            {
                return false;
            }
        }
        true
    }
}

pub(crate) struct Roots(Vec<Root>);
impl Roots {
    fn split(&self, path: &RelativePath) -> Result<(&Root, RelativePath), CoreError> {
        let (label, rest) = path.as_str().split_once('/').unwrap_or((path.as_str(), ""));
        let root = self
            .0
            .iter()
            .find(|root| root.label == label)
            .ok_or(CoreError::SourceUnavailable)?;
        Ok((root, RelativePath::parse(rest)?))
    }
}
impl Source for Roots {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        Ok(NodeType::Directory)
    }
    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        if dir.is_root() {
            return Ok(self
                .0
                .iter()
                .map(|r| SourceEntry::directory(&r.label))
                .collect());
        }
        let (root, path) = self.split(dir)?;
        if !root.allowed(&path, true) {
            return Err(CoreError::PolicyDenied);
        }
        Ok(root
            .source
            .entries(&path)?
            .into_iter()
            .filter(|entry| {
                path.child(&entry.name)
                    .is_ok_and(|child| root.allowed(&child, entry.is_dir()))
            })
            .collect())
    }
    fn kind(&self, path: &RelativePath) -> Result<NodeType, CoreError> {
        if path.is_root() {
            return self.root_kind();
        }
        let (root, path) = self.split(path)?;
        let kind = root.source.kind(&path)?;
        if !root.allowed(&path, kind == NodeType::Directory) {
            return Err(CoreError::PolicyDenied);
        }
        Ok(kind)
    }
    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.stat(path).map(|v| v.0)
    }
    fn stat(&self, path: &RelativePath) -> Result<(EntryMeta, Option<EntryStamp>), CoreError> {
        let (root, path) = self.split(path)?;
        if !root.allowed(&path, false) {
            return Err(CoreError::PolicyDenied);
        }
        root.source.stat(&path)
    }
    fn read(&self, grant: &ReadGrant, cap: u64) -> Result<Vec<u8>, CoreError> {
        let (root, path) = self.split(grant.path())?;
        let grant = ReadGrant::authorize(
            &path,
            FileHandlingDecision::Include,
            root.allowed(&path, false),
        )
        .ok_or(CoreError::PolicyDenied)?;
        root.source.read(&grant, cap)
    }
}

/// Opens a map/pack union. Relative extra roots are relative to the invocation directory.
/// Paths and stable ids in a union carry a directory label, including the primary root.
/// # Errors
/// Refuses unavailable roots, duplicate labels, unsafe labels and rule/cache failures.
pub fn open_mapped_roots(path: &Path, also: &[PathBuf]) -> Result<(Session, String), String> {
    if also.is_empty() {
        return open_mapped(path);
    }
    let (source, key) = union_source(path, also)?;
    let rules = RuleSources::default();
    if let Ok(cache) = cache_root_dir() {
        if ensure_cache_dir(&cache).is_ok() {
            let id = place_root_id(Path::new(&key))?;
            return build_cached(source, "roots", &cache, id, rules).map(|session| (session, key));
        }
    }
    build(source, "roots", "default", rules).map(|session| (session, key))
}

pub(crate) fn union_source(path: &Path, also: &[PathBuf]) -> Result<(Roots, String), String> {
    let mut paths = vec![fs::canonicalize(path).map_err(|e| e.to_string())?];
    for extra in also {
        paths.push(fs::canonicalize(extra).map_err(|e| e.to_string())?);
    }
    paths.sort();
    paths.dedup();
    let mut roots = Vec::new();
    let mut key = String::from("roots-v1:");
    for path in paths {
        let (source, label, canonical) = open_source(&path)?;
        RelativePath::root()
            .child(&label)
            .map_err(|e| e.to_string())?;
        if roots.iter().any(|root: &Root| root.label == label) {
            return Err(format!(
                "duplicate root label {label}; choose directories with distinct names"
            ));
        }
        let (source, rules) = rules::project_rules_inventory(&canonical, source, None)?;
        use std::fmt::Write as _;
        let bytes = canonical.as_os_str().as_encoded_bytes();
        let _ = write!(key, "{}:", bytes.len());
        for byte in bytes {
            let _ = write!(key, "{byte:02x}");
        }
        key.push(';');
        roots.push(Root {
            label,
            source,
            policy: rules.policy(),
        });
    }
    Ok((Roots(roots), key))
}
