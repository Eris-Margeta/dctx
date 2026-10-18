//! Desktop snapshot source: filters and a closed `.git` boundary around `FsSource`.
//! Implements DX-E4a privacy: selection is applied before any content read (F01, F03).
//! Depends on the `dctx` `Source` port and [`crate::settings::DesktopConfig`].
//! [`crate::snapshot`] opens this source after the rule files are loaded.
//! `SourceSpy` records calls that reach the confined source so tests can fail closed.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use dctx::Source;
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;

use crate::error::DesktopError;
use crate::settings::DesktopConfig;
use crate::snapshot_rules::excluded_by_builtin;

/// Safety depth copied from the facade. Deeper survey entries are not counted.
const MAX_DEPTH: u32 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchKind {
    List,
    Metadata,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Touch {
    kind: TouchKind,
    path: String,
}

thread_local! {
    static SPY: std::cell::RefCell<Option<Arc<Mutex<Vec<Touch>>>>> = const { std::cell::RefCell::new(None) };
}

/// Records list, metadata and content reads that reach the confined snapshot source.
///
/// The pack walks on the calling thread. Arm the spy on that thread, run
/// [`crate::generate_snapshot`], then inspect [`Self::reads`]. Dropping it stops recording.
#[derive(Debug)]
#[must_use = "recording stops when the spy is dropped"]
pub struct SourceSpy {
    touches: Arc<Mutex<Vec<Touch>>>,
}

impl SourceSpy {
    /// Starts recording on this thread.
    pub fn arm() -> Self {
        let touches = Arc::new(Mutex::new(Vec::new()));
        SPY.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&touches)));
        Self { touches }
    }

    /// Content-read paths, in order.
    pub fn reads(&self) -> Vec<String> {
        self.collect(|touch| touch.kind == TouchKind::Read)
    }

    /// True when any recorded call named `path` exactly.
    pub fn touched(&self, path: &str) -> bool {
        self.snapshot().iter().any(|touch| touch.path == path)
    }

    /// True when a recorded path has a `.git` component.
    pub fn git_touched(&self) -> bool {
        self.snapshot().iter().any(|touch| git_component(&touch.path))
    }

    /// Every recorded call, for assertion messages.
    pub fn paths(&self) -> Vec<String> {
        self.snapshot().iter().map(|touch| format!("{:?} {}", touch.kind, touch.path)).collect()
    }

    fn collect(&self, keep: impl Fn(&Touch) -> bool) -> Vec<String> {
        self.snapshot().into_iter().filter(|touch| keep(touch)).map(|touch| touch.path).collect()
    }

    fn snapshot(&self) -> Vec<Touch> {
        self.touches.lock().unwrap_or_else(|error| error.into_inner()).clone()
    }
}

impl Drop for SourceSpy {
    fn drop(&mut self) {
        SPY.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|spy| Arc::ptr_eq(spy, &self.touches)) {
                *slot = None;
            }
        });
    }
}

fn note(kind: TouchKind, path: &str) {
    let spy = SPY.with(|slot| slot.borrow().clone());
    let Some(spy) = spy else {
        return;
    };
    spy.lock().unwrap_or_else(|error| error.into_inner()).push(Touch { kind, path: path.to_string() });
}

fn git_component(path: &str) -> bool {
    path.split('/').any(|segment| segment == ".git")
}

/// Refuses every path with a `.git` component before the inner source sees it.
struct GitClosedSource<S> {
    inner: S,
}

impl<S> GitClosedSource<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: Source> Source for GitClosedSource<S> {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        if git_component(dir.as_str()) {
            return Err(DctxError::PolicyDenied);
        }
        let mut entries = self.inner.entries(dir)?;
        entries.retain(|entry| entry.name != ".git");
        Ok(entries)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        if git_component(path.as_str()) {
            return Err(DctxError::PolicyDenied);
        }
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        if git_component(grant.path().as_str()) {
            return Err(DctxError::PolicyDenied);
        }
        self.inner.read(grant, max_bytes)
    }
}

/// Drops files the desktop filters reject before a content read.
struct SelectedSource<S> {
    inner: S,
    config: DesktopConfig,
}

impl<S> SelectedSource<S> {
    fn new(inner: S, config: DesktopConfig) -> Self {
        Self { inner, config }
    }
}

impl<S: Source> Source for SelectedSource<S> {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        let entries = self.inner.entries(dir)?;
        if !self.config.has_active_filters() {
            return Ok(entries);
        }
        let mut kept = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry.name == ".git" {
                continue;
            }
            if entry.is_dir() {
                if directory_visible(&entry.name, &self.config) {
                    kept.push(entry);
                }
                continue;
            }
            if self.file_allowed(dir, &entry.name) {
                kept.push(entry);
            }
        }
        Ok(kept)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        let path = grant.path();
        if self.config.has_active_filters() {
            let parent = parent_path(path)?;
            if !self.file_allowed(&parent, path.name()) {
                return Err(DctxError::PolicyDenied);
            }
        }
        self.inner.read(grant, max_bytes)
    }
}

impl<S: Source> SelectedSource<S> {
    fn file_allowed(&self, dir: &RelativePath, name: &str) -> bool {
        if !name_visible(name, &self.config) {
            return false;
        }
        if !needs_size(&self.config) {
            return true;
        }
        let Ok(path) = dir.child(name) else {
            return false;
        };
        let len = self.inner.metadata(&path).map(|meta| meta.size).unwrap_or(0);
        size_visible(len, &self.config)
    }
}

fn parent_path(path: &RelativePath) -> Result<RelativePath, DctxError> {
    match path.as_str().rsplit_once('/') {
        Some((parent, _)) => RelativePath::parse(parent),
        None => Ok(RelativePath::root()),
    }
}

/// Records calls, then forwards to the confined source.
struct LoggingSource<S> {
    inner: S,
}

impl<S> LoggingSource<S> {
    fn new(inner: S) -> Self {
        Self { inner }
    }
}

impl<S: Source> Source for LoggingSource<S> {
    fn root_kind(&self) -> Result<NodeType, DctxError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, DctxError> {
        note(TouchKind::List, dir.as_str());
        self.inner.entries(dir)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, DctxError> {
        note(TouchKind::Metadata, path.as_str());
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, DctxError> {
        note(TouchKind::Read, grant.path().as_str());
        self.inner.read(grant, max_bytes)
    }
}

/// Confined filesystem source with desktop filters and the `.git` boundary outside it.
pub(crate) fn desktop_source(root: &Path, config: &DesktopConfig) -> Result<impl Source + Send + 'static, DesktopError> {
    let fs = dctx::FsSource::open(root).map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    Ok(GitClosedSource::new(SelectedSource::new(LoggingSource::new(fs), config.clone())))
}

/// Files seen by the pre-pack survey, and how many pass the desktop filters.
#[derive(Debug, Default)]
pub(crate) struct Survey {
    /// Files other than builtin-excluded names.
    pub(crate) files: u64,
    /// Files that also pass the active desktop filters.
    pub(crate) included: u64,
}

/// Counts files without reading contents. `.git` is not opened.
pub(crate) fn survey(root: &Path, config: &DesktopConfig) -> Result<Survey, DesktopError> {
    let meta = fs::metadata(root).map_err(|error| DesktopError::Walk { detail: error.to_string() })?;
    let mut survey = Survey::default();
    if meta.is_dir() {
        survey_dir(root, 0, true, config, &mut survey)?;
    } else {
        survey.files = 1;
        let name = root.file_name().and_then(|item| item.to_str()).unwrap_or("");
        if file_visible(name, meta.len(), config) {
            survey.included = 1;
        }
    }
    Ok(survey)
}

fn survey_dir(dir: &Path, depth: u32, admit: bool, config: &DesktopConfig, survey: &mut Survey) -> Result<(), DesktopError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if depth == 0 => return Err(DesktopError::Walk { detail: error.to_string() }),
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if name.is_empty() || name == "." || name == ".." || name == ".git" {
            continue;
        }
        let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
        if excluded_by_builtin(name, is_dir) {
            continue;
        }
        if is_dir {
            let child_admit = admit && directory_visible(name, config);
            survey_dir(&entry.path(), depth.saturating_add(1), child_admit, config, survey)?;
        } else {
            survey.files = survey.files.saturating_add(1);
            let len = entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            if admit && file_visible(name, len, config) {
                survey.included = survey.included.saturating_add(1);
            }
        }
    }
    Ok(())
}

fn directory_visible(name: &str, config: &DesktopConfig) -> bool {
    if config.has_active_filters() && config.only_hidden && !is_hidden(name) {
        return false;
    }
    true
}

fn file_visible(name: &str, len: u64, config: &DesktopConfig) -> bool {
    if !config.has_active_filters() {
        return true;
    }
    name_visible(name, config) && size_visible(len, config)
}

fn name_visible(name: &str, config: &DesktopConfig) -> bool {
    if config.only_hidden && !is_hidden(name) {
        return false;
    }
    if let Some(exts) = config.filter_extensions.as_ref().filter(|exts| !exts.is_empty()) {
        let Some(ext) = extension_of(name) else {
            return false;
        };
        if !exts.iter().any(|wanted| wanted.eq_ignore_ascii_case(ext)) {
            return false;
        }
    }
    true
}

fn size_visible(len: u64, config: &DesktopConfig) -> bool {
    if config.only_empty && len > 0 {
        return false;
    }
    if config.only_non_empty && len == 0 {
        return false;
    }
    if let Some(min) = config.min_size_mb {
        if len < mb_to_bytes(min) {
            return false;
        }
    }
    if let Some(max) = config.max_size_mb {
        if len > mb_to_bytes(max) {
            return false;
        }
    }
    true
}

fn needs_size(config: &DesktopConfig) -> bool {
    config.only_empty || config.only_non_empty || config.min_size_mb.is_some() || config.max_size_mb.is_some()
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

fn extension_of(file_name: &str) -> Option<&str> {
    let pos = file_name.rfind('.')?;
    if pos == 0 && file_name.len() > 1 {
        return Some(&file_name[1..]);
    }
    if pos > 0 && pos + 1 < file_name.len() {
        return Some(&file_name[pos + 1..]);
    }
    None
}

fn mb_to_bytes(mb: f64) -> u64 {
    let bytes = mb * 1_048_576.0;
    if !bytes.is_finite() || bytes <= 0.0 {
        return 0;
    }
    if bytes >= u64::MAX as f64 { u64::MAX } else { bytes as u64 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dctx::memory::MemorySource;
    use dctx::FileHandlingDecision;
    use dctx_core::facts::ReadGrant;

    fn grant(path: &str) -> ReadGrant {
        let path = RelativePath::parse(path).unwrap();
        ReadGrant::authorize(&path, FileHandlingDecision::Include, true).unwrap()
    }

    #[test]
    fn closed_source_never_lists_or_reads_git() {
        let mut inner = MemorySource::new();
        inner.insert_file("keep.txt", b"keep", 1).unwrap();
        inner.insert_file(".git/config", b"PRIVATE_SENTINEL", 1).unwrap();
        inner.insert_file("nested/.git/config", b"PRIVATE_SENTINEL", 1).unwrap();
        let source = GitClosedSource::new(LoggingSource::new(inner));
        let spy = SourceSpy::arm();
        let entries = source.entries(&RelativePath::root()).unwrap();
        assert!(entries.iter().all(|entry| entry.name != ".git"));
        let nested = RelativePath::parse("nested").unwrap();
        assert!(source.entries(&nested).unwrap().iter().all(|entry| entry.name != ".git"));
        assert!(source.entries(&RelativePath::parse(".git").unwrap()).is_err());
        assert!(source.read(&grant("keep.txt"), 100).is_ok());
        assert!(source.read(&grant(".git/config"), 100).is_err());
        assert!(source.read(&grant("nested/.git/config"), 100).is_err());
        assert!(!spy.git_touched(), "{:?}", spy.paths());
        assert!(spy.reads().iter().any(|path| path == "keep.txt"));
    }
}
