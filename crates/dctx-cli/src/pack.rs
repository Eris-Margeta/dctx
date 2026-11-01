//! Explicit pack. Output is stdout, the clipboard, or `--output`.
//! Implements ADR-011 filters and ADR-009 binary omission on the source page.
//! Depends on the facade host, the `dctx-pack` scan, and [`FilterConfig`].
//! Used by `dctx pack`. `--json` stays the facade summary and refuses view filters.
//! Binaries are omitted unless `--binary`. Include and exclude globs are selection rules.
//! View filters ride on [`PackRequest`] and hide non-matching files before a read.

use std::path::PathBuf;

use dctx::{PackRequest, Source, VecSink};
use dctx_core::budget::PackLimits;
use dctx_core::datatypes::NodeType;
use dctx_core::error::DctxError as CoreError;
use dctx_core::facts::{EntryMeta, ReadGrant, SourceEntry};
use dctx_core::ids::RelativePath;
use dctx_core::version::Version;

use crate::exit::Failure;
use crate::host;

mod filter;
use filter::{Filters, parse_extensions, parse_megabytes, parse_pattern};
mod emit;
use emit::emit;
mod narrow;
use narrow::{file_count, narrow};
mod markers;
pub use markers::legacy_markers;
use markers::source_document;

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
    let usage = |message: String| {
        Failure::usage(
            message,
            "run `dctx pack --help` for the flags and an example",
        )
        .within("pack")
    };
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
        let message = format!(
            "nothing to pack in {}: no file passed the ignore and sensitive-file rules",
            plan.path.display()
        );
        return Err(Failure::no_result(
            message,
            "add files to the directory, or check its .gitignore and .dctxignore rules",
        )
        .within("pack"));
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
    plan.report_format?;
    if plan.filters.debug_ignored {
        return Some("pack --debug-ignored applies to the source page; drop --json");
    }
    plan.filters
        .affects_view()
        .then_some("pack view filters apply to the source page; drop --json")
}

/// `--json` summary. The facade report has no file list, so view filters are refused.
fn facade_summary(plan: &Plan) -> Result<PackOutcome, String> {
    if let Some(message) = summary_refusal(plan) {
        return Err(message.to_string());
    }
    let format = plan
        .report_format
        .ok_or_else(|| "pack summary requires --json".to_string())?;
    let mut session = host::open_dir(&plan.path)?;
    let mut request = PackRequest::new();
    request.output_format = format;
    let mut sink = VecSink::new();
    let report = session
        .pack(&request, &mut sink)
        .map_err(|error| error.to_string())?;
    emit(
        &plan.destination,
        sink.as_bytes(),
        report.files,
        &report.version,
    )
}

/// Source page. Selection comes from the repository rules. View filters hide files first.
fn source_page(plan: &Plan) -> Result<(Vec<u8>, u64, String), String> {
    let rules = host::project_rules(&plan.path)?;
    let policy = rules.policy();
    let request = view_request(&plan.filters);
    let inner = dctx::FsSource::open(&plan.path).map_err(|error| error.to_string())?;
    let report_reads = std::env::var("DCTX_READ_REPORT").ok().as_deref() == Some("1");
    let source = NarrowedSource {
        inner,
        request: &request,
        report_reads,
    };
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
    request
        .include_patterns
        .clone_from(&filters.include_patterns);
    request
        .exclude_patterns
        .clone_from(&filters.exclude_patterns);
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
                let value = args
                    .get(index)
                    .ok_or_else(|| "--output requires a path".to_string())?;
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
                let value = args.get(index).ok_or_else(|| {
                    "--ext requires a comma-separated list of extensions".to_string()
                })?;
                if value.starts_with('-') {
                    return Err("--ext requires a comma-separated list of extensions".to_string());
                }
                filters.config = filters.config.with_extensions(parse_extensions(value)?);
            }
            flag @ ("--large" | "--small") => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{flag} requires a size in megabytes"))?;
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
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{flag} requires a pattern"))?;
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
    Ok(Plan {
        path,
        destination,
        report_format: if json { Some(2) } else { None },
        filters,
    })
}

fn base_name(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, name)) => name,
        None => path,
    }
}

#[cfg(test)]
mod tests;
