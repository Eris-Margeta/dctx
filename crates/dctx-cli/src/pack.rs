//! Explicit pack. Output is stdout, the clipboard, or `--output`.
//! Implements ADR-011 filters and ADR-009 binary omission on the source page.
//! Depends on the facade host, the `dctx-pack` scan, and [`FilterConfig`].
//! Used by `dctx pack`. `--json` stays the facade summary and refuses view filters.
//! Binaries are omitted unless `--binary`. Include and exclude globs are selection rules.
//! View filters ride on [`PackRequest`] and hide non-matching files before a read.

use std::path::{Path, PathBuf};

use dctx::{PackRequest, Source, VecSink};
use dctx_core::budget::PackLimits;
use dctx_core::datatypes::{DirContextTreeNode, IgnoreRule, NodeType};
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::file_classifier::{classify_file, FileContentHandling};
use dctx_core::filter::FilterConfig;
use dctx_core::ids::RelativePath;
use dctx_core::selection::{parse_ignore_pattern_line, rule_matches};
use dctx_core::version::Version;

use crate::compat;
use crate::exit::Failure;
use crate::host;

/// Where the encoded pack page goes.
#[derive(Debug, PartialEq, Eq)]
enum Destination {
    /// The process stdout. No file is created.
    Stdout,
    /// One caller-chosen file.
    File(PathBuf),
    /// The clipboard only.
    Clipboard,
    /// The caller-chosen file and the clipboard.
    FileAndClipboard(PathBuf),
}

/// ADR-011 view plus include/exclude globs. Binary omission is the default.
struct Filters {
    config: FilterConfig,
    include: Vec<IgnoreRule>,
    exclude: Vec<IgnoreRule>,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    debug_ignored: bool,
}

impl Filters {
    fn new() -> Self {
        Self {
            config: FilterConfig::default(),
            include: Vec::new(),
            exclude: Vec::new(),
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            debug_ignored: false,
        }
    }

    /// True when the caller asked for a view the JSON summary cannot show.
    fn affects_view(&self) -> bool {
        !self.config.extensions.is_empty()
            || self.config.include_binary
            || self.config.only_empty
            || self.config.only_non_empty
            || self.config.min_size_bytes.is_some()
            || self.config.max_size_bytes.is_some()
            || self.config.only_hidden
            || !self.include.is_empty()
            || !self.exclude.is_empty()
    }
}

struct Plan {
    path: PathBuf,
    destination: Destination,
    /// Summary report encoding. `None` selects the source-bearing page.
    report_format: Option<u32>,
    filters: Filters,
}

/// Result of one explicit pack. `wrote` is set only for `--output`.
#[derive(Debug, PartialEq, Eq)]
pub struct PackOutcome {
    /// Encoded facade page.
    pub body: Vec<u8>,
    /// File written by this call, when the user passed `--output`.
    pub wrote: Option<PathBuf>,
    /// True when the page was copied to the clipboard.
    pub clipboard: bool,
    /// Byte count and retention class.
    pub report_line: String,
    /// Files in the page; 0 makes a plain `dctx pack` exit 1.
    pub files: u64,
}

/// Flag names `dctx pack --help` prints after the shared help page.
fn flag_help() -> &'static str {
    "\
Pack filters (ADR-011, ADR-009):
  --ext <extensions>    Only files with these extensions (comma-separated)
  --binary              Include binary files (omitted by default)
  --empty               Only 0-byte files
  --non-empty           Only files with content
  --large <MB>          Only files larger than this many megabytes
  --small <MB>          Only files smaller than this many megabytes
  --hidden              Only hidden files and directories
  --debug-ignored       List ignored directories without reading them
  --include <pattern>   Only files matching this ignore-rule glob (repeatable)
  --exclude <pattern>   Drop files matching this ignore-rule glob (repeatable)
  --output, -o <path>   Write the page to one file
  --clipboard, -c       Copy the page to the clipboard
  --json                Summary report instead of the source page
Task context (agent cards):
  --for <text>          Instead of the source page: map find on the names in <text>, then neighbors and read of the best hits
  --budget <tokens>     With --for: cap in estimated tokens, ceil(2 * UTF-8 bytes / 7), 100..100000 (default 3000); whole lines only
Example: dctx pack src --ext rs,toml --output context.txt
Example: dctx pack --for \"fix alpha_body in BetaStore\" --budget 3000
Exit codes: 0 ok, 1 nothing to pack (no view filter), 2 usage, 3 source or output error; see docs/CLI-EXIT-CODES.md.
"
}

/// Parses `pack` arguments, runs the facade, and emits only the requested destination.
/// Exit codes follow docs/CLI-EXIT-CODES.md. With no view filter, a place with no packable file prints its page, then fails with exit 1.
/// # Errors
/// Argument refusals (exit 2), source, scan, render or output failures (exit 3), and a pack of no file (exit 1).
pub fn run(args: &[String]) -> Result<(), Failure> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{}", crate::help::text());
        print!("{}", flag_help());
        return Ok(());
    }
    let usage = |message: String| Failure::usage(message, "run `dctx pack --help` for the flags and an example").within("pack");
    let plan = parse(args).map_err(usage)?;
    if let Some(message) = summary_refusal(&plan) {
        return Err(usage(message.to_string()));
    }
    let outcome = execute_plan(&plan).map_err(|message| {
        Failure::index(message, "check that PATH is an existing, readable directory and that --output names a writable file").within("pack")
    })?;
    if outcome.wrote.is_none() && !outcome.clipboard {
        print!("{}", String::from_utf8_lossy(&outcome.body));
    }
    println!("{}", outcome.report_line);
    if outcome.files == 0 && !plan.filters.affects_view() && !plan.filters.debug_ignored {
        let message = format!("nothing to pack in {}: no file passed the ignore and sensitive-file rules", plan.path.display());
        return Err(Failure::no_result(message, "add files to the directory, or check its .gitignore and .dctxignore rules").within("pack"));
    }
    Ok(())
}

/// Runs pack and returns the page. Tests use this so stdout stays a projection.
/// # Errors
/// Returns argument parsing, source/session, snapshot/rendering, or requested-format failures.
pub fn execute(args: &[String]) -> Result<PackOutcome, String> {
    execute_plan(&parse(args)?)
}

/// Runs one parsed pack: the facade summary for `--json`, else the source page.
fn execute_plan(plan: &Plan) -> Result<PackOutcome, String> {
    if plan.report_format.is_some() {
        return facade_summary(plan);
    }
    let (body, files, version) = source_page(plan)?;
    emit(&plan.destination, &body, files, &version)
}

/// Why `--json` refuses this plan: the summary has no file list, so debug and view filters do not apply.
fn summary_refusal(plan: &Plan) -> Option<&'static str> {
    if plan.report_format.is_none() {
        return None;
    }
    if plan.filters.debug_ignored {
        return Some("pack --debug-ignored applies to the source page; drop --json");
    }
    plan.filters.affects_view().then_some("pack view filters apply to the source page; drop --json")
}

/// `--json` summary. The facade report has no file list, so view filters are refused.
fn facade_summary(plan: &Plan) -> Result<PackOutcome, String> {
    if let Some(message) = summary_refusal(plan) {
        return Err(message.to_string());
    }
    let format = plan.report_format.ok_or_else(|| "pack summary requires --json".to_string())?;
    let mut session = host::open_dir(&plan.path)?;
    let mut request = PackRequest::new();
    request.output_format = format;
    let mut sink = VecSink::new();
    let report = session.pack(&request, &mut sink).map_err(|error| error.to_string())?;
    emit(&plan.destination, sink.as_bytes(), report.files, &report.version)
}

/// Source page. Selection comes from the repository rules. View filters hide files first.
fn source_page(plan: &Plan) -> Result<(Vec<u8>, u64, String), String> {
    let rules = host::project_rules(&plan.path)?;
    let policy = rules.policy();
    let request = view_request(&plan.filters);
    let inner = dctx::FsSource::open(&plan.path).map_err(|error| error.to_string())?;
    let report_reads = std::env::var("DCTX_READ_REPORT").ok().as_deref() == Some("1");
    let source = NarrowedSource { inner, request: &request, report_reads };
    let cancel = dctx::memory::CancelFlag::new();
    let scanned = dctx_pack::scan(
        &source,
        &cancel,
        dctx_pack::ScanOptions {
            policy: &policy,
            debug_ignored: plan.filters.debug_ignored,
            limits: facade_scan_limits()?,
        },
    )
    .map_err(|error| error.to_string())?;
    let tree = narrow(&scanned.tree, &plan.filters);
    let files = file_count(&tree);
    let markers = legacy_markers(&tree, &policy);
    let output = source_document(&plan.path)?;
    let version = Version::new_v1();
    let text = dctx_template::render_legacy_snapshot(
        &output.template,
        &markers,
        &version,
        false,
        output.omit_svg_content,
    );
    Ok((text.into_bytes(), files, version.to_string()))
}

fn view_request(filters: &Filters) -> PackRequest {
    let mut request = PackRequest::new();
    request.apply_filters = true;
    request.extensions.clone_from(&filters.config.extensions);
    request.include_patterns.clone_from(&filters.include_patterns);
    request.exclude_patterns.clone_from(&filters.exclude_patterns);
    request.include_binary = filters.config.include_binary;
    request.only_empty = filters.config.only_empty;
    request.only_non_empty = filters.config.only_non_empty;
    request.min_size_bytes = filters.config.min_size_bytes;
    request.max_size_bytes = filters.config.max_size_bytes;
    request.only_hidden = filters.config.only_hidden;
    request
}

/// Drops files the view filters reject before `dctx-pack` can open them.
/// `DCTX_READ_REPORT=1` prints each content read. The facade wrapper stays quiet.
struct NarrowedSource<'a, S> {
    inner: S,
    request: &'a PackRequest,
    report_reads: bool,
}

impl<S: Source> Source for NarrowedSource<'_, S> {
    fn root_kind(&self) -> Result<NodeType, CoreError> {
        self.inner.root_kind()
    }

    fn entries(&self, dir: &RelativePath) -> Result<Vec<SourceEntry>, CoreError> {
        let entries = self.inner.entries(dir)?;
        if !self.request.apply_filters {
            return Ok(entries);
        }
        let mut kept = Vec::with_capacity(entries.len());
        for entry in entries {
            let Ok(path) = dir.child(&entry.name) else {
                kept.push(entry);
                continue;
            };
            if entry.is_dir() {
                if self.request.admits_directory(path.as_str(), &entry.name) {
                    kept.push(entry);
                }
                continue;
            }
            let size = if self.request.filters_need_size() {
                match self.inner.metadata(&path) {
                    Ok(meta) => meta.size,
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(_) => {
                        kept.push(entry);
                        continue;
                    }
                }
            } else {
                0
            };
            if self.request.admits_file(path.as_str(), &entry.name, size) {
                kept.push(entry);
            }
        }
        Ok(kept)
    }

    fn metadata(&self, path: &RelativePath) -> Result<EntryMeta, CoreError> {
        self.inner.metadata(path)
    }

    fn read(&self, grant: &ReadGrant, max_bytes: u64) -> Result<Vec<u8>, CoreError> {
        if self.request.apply_filters {
            let path = grant.path();
            let size = if self.request.filters_need_size() {
                match self.inner.metadata(path) {
                    Ok(meta) => meta.size,
                    Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                    Err(_) => {
                        if self.report_reads {
                            eprintln!("read {}", path.as_str());
                        }
                        return self.inner.read(grant, max_bytes);
                    }
                }
            } else {
                0
            };
            if !self.request.admits_file(path.as_str(), path.name(), size) {
                return Err(CoreError::PolicyDenied);
            }
        }
        if self.report_reads {
            eprintln!("read {}", grant.path().as_str());
        }
        self.inner.read(grant, max_bytes)
    }
}

/// Safety caps from `crates/dctx/src/domain/limits.rs`. `resolve_scan` is crate-private; the source cap follows `DCTX_MAX_SOURCE_BYTES`.
fn facade_scan_limits() -> Result<PackLimits, String> {
    Ok(PackLimits {
        max_entries: 100_000,
        max_depth: 256,
        max_file_bytes: 8 * 1024 * 1024,
        max_source_bytes: dctx::source_ceiling().map_err(|error| error.to_string())?,
    })
}

fn emit(destination: &Destination, body: &[u8], files: u64, version: &str) -> Result<PackOutcome, String> {
    let bytes = u64::try_from(body.len()).unwrap_or(u64::MAX);
    match destination {
        Destination::Stdout => Ok(PackOutcome {
            body: body.to_vec(),
            wrote: None,
            clipboard: false,
            report_line: format!("pack bytes={bytes} files={files} retention=stdout version={version}"),
            files,
        }),
        Destination::File(path) => {
            write_output(path, body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: Some(path.clone()),
                clipboard: false,
                report_line: format!(
                    "pack bytes={bytes} files={files} retention=explicit path={} version={version}",
                    path.display()
                ),
                files,
            })
        }
        Destination::Clipboard => {
            copy(body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: None,
                clipboard: true,
                report_line: format!("pack bytes={bytes} files={files} retention=clipboard version={version}"),
                files,
            })
        }
        Destination::FileAndClipboard(path) => {
            write_output(path, body)?;
            if let Err(error) = copy(body) {
                return Err(format!("{error}; explicit file already written at {}", path.display()));
            }
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: Some(path.clone()),
                clipboard: true,
                report_line: format!(
                    "pack bytes={bytes} files={files} retention=explicit path={} version={version}",
                    path.display()
                ),
                files,
            })
        }
    }
}

fn write_output(path: &Path, body: &[u8]) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("--output path is empty".to_string());
    }
    std::fs::write(path, body).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn copy(body: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(body);
    compat::copy_to_clipboard(text.as_bytes())
}

fn parse(args: &[String]) -> Result<Plan, String> {
    if args.first().map(String::as_str) != Some("pack") {
        return Err("expected pack".to_string());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err("help is printed by run".to_string());
    }
    let mut path = PathBuf::from(".");
    let mut path_set = false;
    let mut output: Option<PathBuf> = None;
    let mut clipboard = false;
    let mut json = false;
    let mut filters = Filters::new();
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--json" => json = true,
            "--clipboard" | "-c" => clipboard = true,
            "--binary" => filters.config = filters.config.with_include_binary(true),
            "--empty" => filters.config = filters.config.with_only_empty(true),
            "--non-empty" => filters.config = filters.config.with_only_non_empty(true),
            "--hidden" => filters.config = filters.config.with_only_hidden(true),
            "--debug-ignored" => filters.debug_ignored = true,
            "--output" | "-o" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| "--output requires a path".to_string())?;
                if value.starts_with('-') {
                    return Err("--output requires a path".to_string());
                }
                if output.is_some() {
                    return Err("pack accepts one --output path".to_string());
                }
                output = Some(PathBuf::from(value));
            }
            "--ext" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| "--ext requires a comma-separated list of extensions".to_string())?;
                if value.starts_with('-') {
                    return Err("--ext requires a comma-separated list of extensions".to_string());
                }
                filters.config = filters.config.with_extensions(parse_extensions(value)?);
            }
            flag @ ("--large" | "--small") => {
                index += 1;
                let value = args.get(index).ok_or_else(|| format!("{flag} requires a size in megabytes"))?;
                if value.starts_with('-') {
                    return Err(format!("{flag} requires a size in megabytes"));
                }
                let size = parse_megabytes(flag, value)?;
                filters.config = if flag == "--large" {
                    filters.config.with_min_size_mb(size)
                } else {
                    filters.config.with_max_size_mb(size)
                };
            }
            flag @ ("--include" | "--exclude") => {
                index += 1;
                let value = args.get(index).ok_or_else(|| format!("{flag} requires a pattern"))?;
                if value.starts_with('-') {
                    return Err(format!("{flag} requires a pattern"));
                }
                let rule = parse_pattern(flag, value)?;
                if flag == "--include" {
                    filters.include.push(rule);
                    filters.include_patterns.push(value.clone());
                } else {
                    filters.exclude.push(rule);
                    filters.exclude_patterns.push(value.clone());
                }
            }
            other if other.starts_with('-') => return Err(format!("unknown pack flag: {other}")),
            other => {
                if path_set {
                    return Err("pack accepts one source path".to_string());
                }
                path = PathBuf::from(other);
                path_set = true;
            }
        }
        index += 1;
    }
    if filters.config.only_empty && filters.config.only_non_empty {
        return Err("Cannot use both --empty and --non-empty flags".to_string());
    }
    let destination = match (output, clipboard) {
        (None, false) => Destination::Stdout,
        (Some(path), false) => Destination::File(path),
        (None, true) => Destination::Clipboard,
        (Some(path), true) => Destination::FileAndClipboard(path),
    };
    Ok(Plan { path, destination, report_format: if json { Some(2) } else { None }, filters })
}

fn parse_extensions(value: &str) -> Result<Vec<String>, String> {
    let extensions: Vec<_> =
        value.split(',').map(str::trim).filter(|part| !part.is_empty()).map(str::to_string).collect();
    if extensions.is_empty() {
        return Err("--ext requires at least one extension".to_string());
    }
    Ok(extensions)
}

fn parse_megabytes(flag: &str, value: &str) -> Result<f64, String> {
    let size = value.parse::<f64>().map_err(|_| format!("Invalid size for {flag}: {value}"))?;
    if !size.is_finite() || size < 0.0 {
        return Err(format!("Invalid size for {flag}: {value}"));
    }
    Ok(size)
}

fn parse_pattern(flag: &str, value: &str) -> Result<IgnoreRule, String> {
    let rule = parse_ignore_pattern_line(value).ok_or_else(|| format!("{flag} requires a pattern"))?;
    if rule.pattern.is_empty() {
        return Err(format!("{flag} requires a pattern"));
    }
    if rule.is_negation {
        return Err(format!("{flag} does not take a negated pattern"));
    }
    Ok(rule)
}

/// Keeps the root. Drops files the ADR filters reject and prunes empty directories.
fn narrow(node: &DirContextTreeNode, filters: &Filters) -> DirContextTreeNode {
    let mut root = shell(node);
    for child in &node.children {
        if let Some(kept) = narrow_child(child, filters) {
            root.children.push(kept);
        }
    }
    root
}

fn narrow_child(node: &DirContextTreeNode, filters: &Filters) -> Option<DirContextTreeNode> {
    let name = base_name(&node.relative_path);
    if node.node_type == NodeType::Directory {
        if !filters.config.should_include_directory(name) || pattern_hit(&filters.exclude, &node.relative_path, name, true)
        {
            return None;
        }
        let mut kept = shell(node);
        for child in &node.children {
            if let Some(next) = narrow_child(child, filters) {
                kept.children.push(next);
            }
        }
        return (!kept.children.is_empty()).then_some(kept);
    }
    let binary = treated_as_binary(node);
    if !filters.config.should_include_file(name, node.content_size, binary, Path::new(&node.relative_path)) {
        return None;
    }
    if !filters.include.is_empty() && !pattern_hit(&filters.include, &node.relative_path, name, false) {
        return None;
    }
    if pattern_hit(&filters.exclude, &node.relative_path, name, false) {
        return None;
    }
    Some(shell(node))
}

fn shell(node: &DirContextTreeNode) -> DirContextTreeNode {
    DirContextTreeNode {
        node_type: node.node_type,
        relative_path: node.relative_path.clone(),
        last_modified_timestamp: node.last_modified_timestamp,
        content_offset_in_data_section: node.content_offset_in_data_section,
        content_size: node.content_size,
        disk_path: node.disk_path.clone(),
        children: Vec::new(),
        generated_id_for_llm: node.generated_id_for_llm.clone(),
        file_content: node.file_content.clone(),
        is_binary_file: node.is_binary_file,
        binary_file_type: node.binary_file_type.clone(),
        is_ignored: node.is_ignored,
        handling_decision: node.handling_decision,
    }
}

/// Extension binaries stay omitted when the scan withheld their bytes.
fn treated_as_binary(node: &DirContextTreeNode) -> bool {
    node.is_binary_file
        || matches!(classify_file(Path::new(&node.relative_path), b""), FileContentHandling::OmitBinary { .. })
}

fn pattern_hit(rules: &[IgnoreRule], path: &str, name: &str, is_dir: bool) -> bool {
    rules.iter().any(|rule| rule_matches(rule, path, name, is_dir))
}

fn base_name(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, name)) => name,
        None => path,
    }
}

fn file_count(node: &DirContextTreeNode) -> u64 {
    let own = u64::from(node.node_type == NodeType::File);
    node.children.iter().fold(own, |sum, child| sum.saturating_add(file_count(child)))
}

/// Source-bearing page. Scan caps in the facade bound which files are packed.
fn source_document(path: &Path) -> Result<dctx_template::LegacyOutput, String> {
    let mut template = crate::template::legacy_template("default")?;
    template.insert_variable("project", &project_name(path));
    Ok(dctx_template::LegacyOutput::new(template, dctx_template::LegacyLayout::Snapshot))
}

fn project_name(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("project")
        .to_string()
}

/// Project the scanned facts into pure renderer input, preserving presentation-only omit rules.
#[allow(clippy::must_use_candidate, reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract.")]
pub fn legacy_markers(
    node: &dctx_core::datatypes::DirContextTreeNode,
    policy: &dctx_core::selection::SelectionPolicy,
) -> dctx_template::MarkerNode {
    let name = base_name(&node.relative_path);
    let is_dir = node.node_type == dctx_core::datatypes::NodeType::Directory;
    let omit_content = if node.is_binary_file || node.binary_file_type.is_some() {
        false
    } else {
        node.handling_decision == dctx_core::FileHandlingDecision::OmitContent
            || dctx_core::selection::rule::last_match(policy.omit_rules(), &node.relative_path, name, is_dir)
    };
    dctx_template::MarkerNode {
        relative_path: node.relative_path.clone(),
        is_dir,
        is_ignored: node.is_ignored,
        omit_content,
        binary_label: if node.is_binary_file {
            Some(node.binary_file_type.clone().unwrap_or_else(|| "Unknown".into()))
        } else {
            node.binary_file_type.clone().filter(|label| label == "SVG file")
        },
        content: if node.is_ignored || omit_content || node.is_binary_file { None } else { node.file_content.clone() },
        content_size: node.content_size,
        modified: node.last_modified_timestamp,
        children: node.children.iter().map(|child| legacy_markers(child, policy)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("dctx-cli-pack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp");
        std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("source");
        path
    }

    #[test]
    fn explicit_pack_writes_no_project_text_file() {
        let path = project("stdout");
        let outcome = execute(&["pack".into(), path.display().to_string()]).expect("pack");
        assert!(outcome.wrote.is_none());
        assert!(!outcome.clipboard);
        assert!(!path.join(".dctx").exists());
        let text = std::str::from_utf8(&outcome.body).expect("utf8");
        assert!(text.contains("PATH=\"kept.rs\""), "{text}");
        assert!(text.contains("fn kept() {}"), "{text}");
        assert!(outcome.report_line.contains("retention=stdout"));
        assert!(outcome.report_line.contains(&format!("bytes={}", outcome.body.len())));
        let listed: Vec<_> = std::fs::read_dir(&path).expect("list").flatten().collect();
        assert_eq!(listed.len(), 1);
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn env_shaped_files_are_never_printed_by_default() {
        let path = project("envfiles");
        for name in [".env", ".env.local", "settings.env", ".envrc"] {
            std::fs::write(path.join(name), b"K=CANARYSECRET\n").expect("env");
        }
        let outcome = execute(&["pack".into(), path.display().to_string()]).expect("pack");
        let text = std::str::from_utf8(&outcome.body).expect("utf8");
        assert!(!text.contains("CANARYSECRET"), "{text}");
        assert!(text.contains("fn kept() {}"), "{text}");
        let json = execute(&["pack".into(), path.display().to_string(), "--json".into()]).expect("json");
        assert!(!String::from_utf8_lossy(&json.body).contains("CANARYSECRET"));
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn output_flag_writes_only_the_requested_file() {
        let path = project("file");
        let output = path.join("explicit.txt");
        let outcome = execute(&[
            "pack".into(),
            path.display().to_string(),
            "--output".into(),
            output.display().to_string(),
            "--json".into(),
        ])
        .expect("pack");
        assert_eq!(outcome.wrote.as_deref(), Some(output.as_path()));
        assert!(!path.join(".dctx").exists());
        let saved = std::fs::read(&output).expect("saved");
        assert_eq!(saved, outcome.body);
        assert!(std::str::from_utf8(&saved).expect("utf8").contains("\"files\""));
        assert!(outcome.report_line.contains("retention=explicit"));
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn unknown_flag_is_refused_before_a_scan() {
        let error = execute(&["pack".into(), "--tree".into()]).expect_err("flag");
        assert!(error.contains("unknown pack flag"));
    }

    #[test]
    fn help_names_every_pack_flag() {
        let help = flag_help();
        for flag in [
            "--ext",
            "--binary",
            "--empty",
            "--non-empty",
            "--large",
            "--small",
            "--hidden",
            "--debug-ignored",
            "--include",
            "--exclude",
            "--output",
            "-o",
            "--clipboard",
            "-c",
            "--json",
            "--for",
            "--budget",
        ] {
            assert!(help.contains(flag), "missing {flag}");
        }
    }

    #[test]
    fn empty_and_non_empty_are_refused() {
        let error = execute(&["pack".into(), "--empty".into(), "--non-empty".into()]).expect_err("both");
        assert!(error.contains("--empty"));
    }

    #[test]
    fn default_omits_binary_and_ext_keeps_rust() {
        let path = std::env::temp_dir().join(format!("dctx-cli-pack-filter-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp");
        std::fs::write(path.join("kept.rs"), b"fn kept() {}\n").expect("rs");
        std::fs::write(path.join("notes.md"), b"notes\n").expect("md");
        std::fs::write(path.join("logo.png"), b"\x89PNG\0SENTINEL").expect("png");
        let packed = execute(&["pack".into(), path.display().to_string()]).expect("pack");
        let text = std::str::from_utf8(&packed.body).expect("utf8");
        assert!(text.contains("PATH=\"kept.rs\""));
        assert!(text.contains("PATH=\"notes.md\""));
        assert!(!text.contains("logo.png"), "{text}");
        assert!(!text.contains("SENTINEL"));
        let only = execute(&["pack".into(), path.display().to_string(), "--ext".into(), "rs".into()]).expect("ext");
        let narrowed = std::str::from_utf8(&only.body).expect("utf8");
        assert!(narrowed.contains("PATH=\"kept.rs\""));
        assert!(!narrowed.contains("notes.md"));
        assert!(only.report_line.contains("files=1"));
        let _ = std::fs::remove_dir_all(&path);
    }
}
