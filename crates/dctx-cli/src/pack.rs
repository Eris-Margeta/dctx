// SPDX-License-Identifier: Apache-2.0
//! Explicit pack. Output is stdout, the clipboard, or `--output`.
//! Implements ADR-011 filters and ADR-009 binary omission on the source page.
//! Depends on the facade host, the `dctx-pack` scan, and [`dctx_core::filter::FilterConfig`].
//! Used by `dctx pack`. `--json` stays the facade summary and refuses view filters.
//! Binaries are omitted unless `--binary`. Include and exclude globs are selection rules.
//! View filters ride on [`PackRequest`] and hide non-matching files before a read.

use std::io::Write as _;
use std::path::PathBuf;

use dctx::{DctxError, PackRequest, VecSink};
use dctx_core::budget::PackLimits;
use dctx_core::version::Version;

use crate::exit::{from_facade_scoped, Failure};
use crate::host;

mod filter;
use filter::{parse_extensions, parse_megabytes, parse_pattern, Filters};
mod emit;
use emit::emit;
mod narrow;
use narrow::{file_count, narrow};
mod narrowed;
use narrowed::NarrowedSource;
mod markers;
mod output_file;
mod spool;
pub use markers::legacy_markers;
use markers::{into_legacy_markers, source_document};

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
    /// `--tree-stamps`: `MOD:` timestamps on the tree lines of the source page.
    tree_stamps: dctx_template::TreeStamps,
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
    /// Byte count and retention class: the status line `dctx pack` prints on stderr.
    pub report_line: String,
    /// Files in the page; 0 makes a plain `dctx pack` exit 1.
    pub files: u64,
}

/// Flag names `dctx pack --help` prints after the pack usage lines.
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
  --json                Summary report; with --for, the facade JSON context page
  --tree-stamps=on|off  MOD: timestamp on tree lines: off (default) keeps the page prefix stable across a touch, on = the 1.0.0 text
Task context (agent cards):
  --for <text>          Instead of the source page: map find on the names in <text>, then neighbors and read of the best hits
  --budget <tokens>     With --for: cap in estimated tokens, ceil(2 * UTF-8 bytes / 7), 100..100000 (default 3000); whole lines only
Output: stdout is exactly the page (or one JSON document with --json); the status line `pack bytes=N files=N ...` goes to stderr.
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
    let stdout = std::io::stdout();
    let mut output = std::io::BufWriter::with_capacity(64 * 1024, stdout.lock());
    match write_output(args, &mut output) {
        Err(failure) if failure.code == 0 => Ok(()),
        result => result,
    }
}

/// Writes CLI output incrementally. Capture callers can supply a `Vec<u8>` explicitly.
/// # Errors
/// Returns the same argument, source and no-result failures as [`output`], or a writer failure.
pub fn write_output(args: &[String], stdout: &mut dyn std::io::Write) -> Result<(), Failure> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return output(args)?.write_to(stdout);
    }
    let plan = parse(args).map_err(|message| {
        Failure::usage(
            message,
            "run `dctx pack --help` for the flags and an example",
        )
        .within("pack")
    })?;
    if let Some(message) = summary_refusal(&plan) {
        return Err(Failure::usage(
            message,
            "run `dctx pack --help` for the flags and an example",
        )
        .within("pack"));
    }
    // Summary pages contain only bounded counters and version metadata.
    if plan.report_format.is_some() {
        let outcome = execute_plan(&plan).map_err(|error| error.within("pack"))?;
        if outcome.wrote.is_none() && !outcome.clipboard {
            stdout
                .write_all(&outcome.body)
                .map_err(crate::exit::write_failure)?;
        }
        stdout.flush().map_err(crate::exit::write_failure)?;
        report_status(&outcome.report_line);
        return no_result(&plan, outcome.files).map_or(Ok(()), Err);
    }
    let mut spool = spool::Spool::new(&plan.path).map_err(crate::exit::write_failure)?;
    let (markers, files, document) =
        source_markers(&plan, Some(&mut spool)).map_err(|error| error.within("pack"))?;
    let bodies = spool.finish().map_err(crate::exit::write_failure)?;
    let version = Version::new_v1();
    // Every destination gets the same bytes: the page, ending in exactly one added newline when
    // the template leaves it open.
    let render = |writer: &mut dyn std::io::Write| {
        let mut page = PageWriter::new(writer);
        dctx_template::write_legacy_snapshot_with(
            &mut page,
            &document.template,
            &markers,
            &version,
            false,
            document.omit_svg_content,
            &|node| bodies.load(node),
        )?;
        page.finish()
    };
    let bytes = match &plan.destination {
        Destination::Stdout => render(stdout).map_err(crate::exit::write_failure)?,
        Destination::File(path) | Destination::FileAndClipboard(path) => {
            if path.as_os_str().is_empty() {
                return Err(pack_io_failure("--output path is empty".to_string()).within("pack"));
            }
            let failed = |error: std::io::Error| {
                pack_io_failure(format!("failed to write {}: {error}", path.display()))
                    .within("pack")
            };
            let bytes = output_file::write_with(path, |writer| render(writer)).map_err(failed)?;
            if matches!(plan.destination, Destination::FileAndClipboard(_)) {
                crate::clipboard::copy_with(|writer| {
                    render(writer).map_err(|error| error.to_string())
                })
                .map_err(|error| {
                    clipboard_failure(format!(
                        "{error}; explicit file already written at {}",
                        path.display()
                    ))
                    .within("pack")
                })?;
            }
            bytes
        }
        Destination::Clipboard => {
            crate::clipboard::copy_with(|writer| render(writer).map_err(|error| error.to_string()))
                .map_err(|error| clipboard_failure(error).within("pack"))?
        }
    };
    stdout.flush().map_err(crate::exit::write_failure)?;
    report_status(&emit::status_line(
        bytes,
        files,
        &plan.destination,
        &version.to_string(),
    ));
    no_result(&plan, files).map_or(Ok(()), Err)
}

/// Prints the status line on stderr, so stdout stays exactly the page (or the one JSON document).
/// A closed stderr is not a pack failure.
fn report_status(line: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(stderr, "{line}").and_then(|()| stderr.flush());
}

/// Counts the page bytes and ends the page with a newline when the template leaves it open.
struct PageWriter<'a> {
    inner: &'a mut dyn std::io::Write,
    bytes: u64,
    last: Option<u8>,
}

impl<'a> PageWriter<'a> {
    fn new(inner: &'a mut dyn std::io::Write) -> Self {
        Self {
            inner,
            bytes: 0,
            last: None,
        }
    }

    /// Writes the closing newline if needed, flushes, and returns the page length in bytes.
    fn finish(mut self) -> std::io::Result<u64> {
        if self.last != Some(b'\n') {
            std::io::Write::write_all(&mut self, b"\n")?;
        }
        std::io::Write::flush(&mut self)?;
        Ok(self.bytes)
    }
}

impl std::io::Write for PageWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(bytes)?;
        if count > 0 {
            self.last = Some(bytes[count - 1]);
        }
        self.bytes += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn no_result(plan: &Plan, files: u64) -> Option<Failure> {
    (files == 0 && !plan.filters.affects_view() && !plan.filters.debug_ignored).then(|| {
        Failure::no_result(
            format!(
                "nothing to pack in {}: no file passed the ignore and sensitive-file rules",
                plan.path.display()
            ),
            "add files to the directory, or check its .gitignore and .dctxignore rules",
        )
        .within("pack")
    })
}

/// Captures the same pack output and status as the CLI without printing.
/// # Errors
/// Returns argument, source, scan, render or output failures.
pub fn output(args: &[String]) -> Result<crate::exit::CommandOutput, Failure> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(crate::exit::CommandOutput {
            stdout: format!("{}{}", crate::help::pack_usage(), flag_help()),
            failure: None,
        });
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
    let outcome = execute_plan(&plan).map_err(|error| error.within("pack"))?;
    // stdout is exactly the page, or one JSON document; the status line belongs to stderr.
    let mut stdout = String::new();
    if outcome.wrote.is_none() && !outcome.clipboard {
        stdout.push_str(&String::from_utf8_lossy(&outcome.body));
    }
    let failure = no_result(&plan, outcome.files);
    Ok(crate::exit::CommandOutput { stdout, failure })
}

/// Runs pack and returns the page. Tests use this so stdout stays a projection.
/// # Errors
/// Returns argument parsing, source/session, snapshot/rendering, or requested-format failures.
pub fn execute(args: &[String]) -> Result<PackOutcome, String> {
    execute_plan(&parse(args)?).map_err(|error| error.message)
}

/// Runs one parsed pack: the facade summary for `--json`, else the source page.
fn execute_plan(plan: &Plan) -> Result<PackOutcome, Failure> {
    if plan.report_format.is_some() {
        return facade_summary(plan);
    }
    let (body, files, version) = source_page(plan)?;
    emit(&plan.destination, body, files, &version)
}

fn pack_io_failure(message: String) -> Failure {
    if message.contains("DCTX_MAX_ENTRIES") {
        return Failure::index(
            message,
            "raise DCTX_MAX_ENTRIES (1..100000000) or use `dctx pack <a smaller directory>`",
        );
    }
    Failure::index(message, "check that PATH is an existing, readable directory and that --output names a writable file")
}

/// Clipboard failures name the tools tried (in the message) and offer `--output` instead.
fn clipboard_failure(message: String) -> Failure {
    Failure::index(
        message,
        format!(
            "install a clipboard tool ({}), or write the page to a file with --output FILE",
            crate::clipboard::tool_names().join(", ")
        ),
    )
}

/// Preserve typed capacity refusals until their recovery hint has been selected.
fn pack_facade_failure(error: DctxError) -> Failure {
    from_facade_scoped(
        &error,
        "dctx pack --help",
        "use `dctx pack <a smaller directory>`",
    )
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
fn facade_summary(plan: &Plan) -> Result<PackOutcome, Failure> {
    if let Some(message) = summary_refusal(plan) {
        return Err(pack_io_failure(message.to_string()));
    }
    let format = plan
        .report_format
        .ok_or_else(|| pack_io_failure("pack summary requires --json".to_string()))?;
    let mut session = host::open_dir(&plan.path).map_err(pack_io_failure)?;
    let mut request = PackRequest::new();
    request.output_format = format;
    let mut sink = VecSink::new();
    let report = session
        .pack(&request, &mut sink)
        .map_err(pack_facade_failure)?;
    emit(
        &plan.destination,
        sink.as_bytes().to_vec(),
        report.files,
        &report.version,
    )
}

/// Source page. Selection comes from the repository rules. View filters hide files first.
fn source_page(plan: &Plan) -> Result<(Vec<u8>, u64, String), Failure> {
    let (markers, files, output) = source_markers(plan, None)?;
    let version = Version::new_v1();
    let mut body = Vec::new();
    dctx_template::write_legacy_snapshot(
        &mut body,
        &output.template,
        &markers,
        &version,
        false,
        output.omit_svg_content,
    )
    .map_err(crate::exit::write_failure)?;
    Ok((body, files, version.to_string()))
}

fn source_markers(
    plan: &Plan,
    spool: Option<&mut spool::Spool>,
) -> Result<(dctx_template::MarkerNode, u64, dctx_template::LegacyOutput), Failure> {
    let rules = host::project_rules(&plan.path).map_err(pack_io_failure)?;
    let policy = rules.policy();
    let request = view_request(&plan.filters);
    let inner = dctx::FsSource::open(&plan.path).map_err(pack_facade_failure)?;
    let report_reads = std::env::var("DCTX_READ_REPORT").ok().as_deref() == Some("1");
    let hidden = spool.as_deref().and_then(spool::Spool::hidden).cloned();
    let frozen_parent = spool
        .as_deref()
        .and_then(spool::Spool::frozen_parent)
        .cloned();
    let source = NarrowedSource {
        inner,
        request: &request,
        report_reads,
        hidden: hidden.as_ref(),
        frozen_parent: frozen_parent.as_ref(),
    };
    let cancel = dctx::memory::CancelFlag::new();
    let options = dctx_pack::ScanOptions {
        policy: &policy,
        debug_ignored: plan.filters.debug_ignored,
        limits: facade_scan_limits()?,
    };
    let scanned = if let Some(spool) = spool {
        dctx_pack::scan_spooled(&source, &cancel, options, &mut |path, bytes| {
            spool
                .push(path, bytes)
                .map_err(|error| dctx_core::error::DctxError::io(error.to_string()))
        })
    } else {
        dctx_pack::scan(&source, &cancel, options)
    }
    .map_err(|error| pack_facade_failure(DctxError::from_core(error)))?;
    let tree = narrow(scanned.tree, &plan.filters);
    let files = file_count(&tree);
    let markers = into_legacy_markers(tree, &policy);
    let output = source_document(&plan.path, plan.tree_stamps).map_err(pack_io_failure)?;
    Ok((markers, files, output))
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

/// Safety caps from the facade, including the source and entry environment overrides.
fn facade_scan_limits() -> Result<PackLimits, Failure> {
    Ok(PackLimits {
        max_entries: dctx::entries_ceiling().map_err(pack_facade_failure)?,
        max_depth: 256,
        max_file_bytes: 8 * 1024 * 1024,
        max_source_bytes: dctx::source_ceiling().map_err(pack_facade_failure)?,
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
    let mut tree_stamps = dctx_template::TreeStamps::default();
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
            "--tree-stamps" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--tree-stamps requires on or off".to_string())?;
                tree_stamps = dctx_template::TreeStamps::parse(value)?;
            }
            other if other.starts_with("--tree-stamps=") => {
                tree_stamps = dctx_template::TreeStamps::parse(&other["--tree-stamps=".len()..])?;
            }
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
        tree_stamps,
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
