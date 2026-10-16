//! Explicit pack. Output is stdout, the clipboard, or `--output`.
//! Implements ARCHITECTURE-PROPOSAL §4 normal outputs: no unsolicited project text file.
//! Depends on the `dctx` facade [`crate::host::open_dir`]. The legacy `dctx .` path does not use this module.
//! A clipboard failure returns an error and does not write a fallback file.

use std::path::PathBuf;

use dctx::{PackReport, PackRequest, VecSink};

use crate::host;
use crate::platform;

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
    output_format: u32,
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
}

/// Parses `pack` arguments, runs the facade, and emits only the requested destination.
pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{}", crate::help::text());
        return Ok(());
    }
    let outcome = execute(args)?;
    if outcome.wrote.is_none() && !outcome.clipboard {
        print!("{}", String::from_utf8_lossy(&outcome.body));
    }
    println!("{}", outcome.report_line);
    Ok(())
}

/// Runs pack and returns the page. Tests use this so stdout stays a projection.
pub fn execute(args: &[String]) -> Result<PackOutcome, String> {
    let plan = parse(args)?;
    let mut session = host::open_dir(&plan.path)?;
    let mut request = PackRequest::new();
    request.output_format = plan.output_format;
    let mut sink = VecSink::new();
    let report = session.pack(&request, &mut sink).map_err(|error| error.to_string())?;
    let body = sink.as_bytes().to_vec();
    emit(&plan.destination, &body, &report)
}

fn emit(destination: &Destination, body: &[u8], report: &PackReport) -> Result<PackOutcome, String> {
    let bytes = u64::try_from(body.len()).unwrap_or(u64::MAX);
    match destination {
        Destination::Stdout => Ok(PackOutcome {
            body: body.to_vec(),
            wrote: None,
            clipboard: false,
            report_line: format!(
                "pack bytes={bytes} files={} retention=stdout version={}",
                report.files, report.version
            ),
        }),
        Destination::File(path) => {
            write_output(path, body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: Some(path.clone()),
                clipboard: false,
                report_line: format!(
                    "pack bytes={bytes} files={} retention=explicit path={} version={}",
                    report.files,
                    path.display(),
                    report.version
                ),
            })
        }
        Destination::Clipboard => {
            copy(body)?;
            Ok(PackOutcome {
                body: body.to_vec(),
                wrote: None,
                clipboard: true,
                report_line: format!(
                    "pack bytes={bytes} files={} retention=clipboard version={}",
                    report.files, report.version
                ),
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
                    "pack bytes={bytes} files={} retention=explicit path={} version={}",
                    report.files,
                    path.display(),
                    report.version
                ),
            })
        }
    }
}

fn write_output(path: &std::path::Path, body: &[u8]) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("--output path is empty".to_string());
    }
    std::fs::write(path, body).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn copy(body: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(body);
    platform::copy_to_clipboard(&text)
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
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--json" => json = true,
            "--clipboard" | "-c" => clipboard = true,
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
    let destination = match (output, clipboard) {
        (None, false) => Destination::Stdout,
        (Some(path), false) => Destination::File(path),
        (None, true) => Destination::Clipboard,
        (Some(path), true) => Destination::FileAndClipboard(path),
    };
    Ok(Plan { path, destination, output_format: if json { 2 } else { 1 } })
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
        assert!(std::str::from_utf8(&outcome.body).expect("utf8").contains("files="));
        assert!(outcome.report_line.contains("retention=stdout"));
        assert!(outcome.report_line.contains(&format!("bytes={}", outcome.body.len())));
        let listed: Vec<_> = std::fs::read_dir(&path).expect("list").flatten().collect();
        assert_eq!(listed.len(), 1);
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
}
