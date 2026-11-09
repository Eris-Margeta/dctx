// SPDX-License-Identifier: Apache-2.0
//! Template build and list through the facade compiler.
//! Implements ARCHITECTURE-PROPOSAL §2 template compilation and §4 explicit outputs.
//! Depends on [`dctx::Session::compile_template`] and [`dctx::Session::load_template`].
//! Build writes a plan only when `--output` is set. List does not scan the home directory.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use dctx_adapter_fs::StdFs;
use dctx_template::to_bytes;

use crate::exit::Failure;

const MAX_TEMPLATE_BYTES: u64 = 1024 * 1024;
const MAX_TEMPLATE_ENTRIES: usize = 100_000;

/// Hint for a wrong template argument.
const HELP_HINT: &str = "run `dctx template --help` for the template commands";

use crate::host;

/// Prints the result of `template build` or `template list`.
/// Failures follow docs/CLI-EXIT-CODES.md: 2 for arguments or a template that does not compile, 3 for file I/O.
/// # Errors
/// Returns template command parsing, compilation, loading, or listing failures with their exit code and hint.
pub fn run(args: &[String]) -> Result<(), Failure> {
    println!("{}", execute(args)?);
    Ok(())
}

/// Compiles or lists and returns the report. `--output` is the only write.
/// # Errors
/// Returns missing/unknown actions, invalid arguments (exit 2), or template filesystem failures (exit 3).
pub fn execute(args: &[String]) -> Result<String, Failure> {
    if args.first().map(String::as_str) != Some("template") {
        return Err(usage("expected template"));
    }
    if args.len() == 1 || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(crate::help::text());
    }
    match args[1].as_str() {
        "build" => build(&args[2..]),
        "list" => list(&args[2..]),
        other => Err(usage(format!(
            "unknown template action: {other}. Use build or list"
        ))),
    }
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::usage(message, HELP_HINT)
}

/// Exit 3 for a template file or directory that cannot be read or written; the message names the path.
fn io_failure(action: &str, path: &Path, error: impl std::fmt::Display) -> Failure {
    Failure::index(
        format!("failed to {action} {}: {error}", path.display()),
        if action == "write" {
            "check that the --output directory exists and is writable"
        } else {
            "check that the path exists and is readable"
        },
    )
}

fn build(args: &[String]) -> Result<String, Failure> {
    let mut source_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--output" | "-o" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage("--output requires a path"))?;
                if output.is_some() {
                    return Err(usage("template build accepts one --output path"));
                }
                output = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown template flag: {other}")))
            }
            other => {
                if source_path.is_some() {
                    return Err(usage("template build accepts one source path"));
                }
                source_path = Some(PathBuf::from(other));
            }
        }
        index += 1;
    }
    let source_path =
        source_path.ok_or_else(|| usage("template build requires a template path"))?;
    let bytes = host::read_explicit_file(&source_path, MAX_TEMPLATE_BYTES)
        .map_err(|error| io_failure("read template", &source_path, error))?;
    let source = String::from_utf8(bytes).map_err(|_| {
        Failure::usage(
            format!("template {} is not UTF-8 text", source_path.display()),
            "save the template as UTF-8",
        )
    })?;
    let session = compiler()?;
    let plan = session
        .compile_template(&source)
        .map_err(|error| {
            Failure::usage(
                format!(
                    "template {} does not compile: {error}",
                    source_path.display()
                ),
                "fix the template source; crates/dctx-template/templates/default.dctx_template in the dctx repository is a working example",
            )
        })?;
    let bytes = to_bytes(&plan).map_err(|error| {
        Failure::index(
            format!("failed to encode the compiled plan: {error}"),
            "report this as a dctx bug",
        )
    })?;
    let digest = hex(&plan.plan_digest);
    let retention = if let Some(path) = &output {
        write_output(path, &bytes)?;
        format!("explicit path={}", path.display())
    } else {
        "stdout".to_string()
    };
    Ok(format!(
        "template source_version={} plan_bytes={} digest={} views={} retention={retention}",
        plan.source_version,
        bytes.len(),
        digest,
        plan.views.len()
    ))
}

/// Writes `--output` without following a symlink at the target and refuses a non-regular target.
/// The plan goes to a new temporary file in the target directory (created exclusively, so never through a link),
/// which is then renamed over the target: a symlink planted at the path is replaced, never written through.
fn write_output(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Failure::index(
                format!(
                    "refusing to write {}: it is a symbolic link",
                    path.display()
                ),
                "pass --output a regular file path, not a link",
            ))
        }
        Ok(meta) if !meta.is_file() => {
            return Err(Failure::index(
                format!(
                    "refusing to write {}: it is not a regular file",
                    path.display()
                ),
                "pass --output a regular file path",
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_failure("write", path, error)),
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".dctx-template-")
        .tempfile_in(directory)
        .map_err(|error| io_failure("write", path, error))?;
    staged
        .write_all(bytes)
        .and_then(|()| staged.as_file().sync_all())
        .map_err(|error| io_failure("write", path, error))?;
    staged
        .persist(path)
        .map_err(|error| io_failure("write", path, error.error))?;
    Ok(())
}

fn list(args: &[String]) -> Result<String, Failure> {
    let mut dir: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| usage("--dir requires a path"))?;
                dir = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown template flag: {other}")))
            }
            other => return Err(usage(format!("unknown template list argument: {other}"))),
        }
        index += 1;
    }
    let Some(dir) = dir else {
        return Ok(
            "template list does not scan the home directory. Pass --dir PATH to load explicit compiled plans through the facade.\n"
                .to_string(),
        );
    };
    let dir = std::fs::canonicalize(&dir).map_err(|error| io_failure("read", &dir, error))?;
    let session = compiler()?;
    let entries = std::fs::read_dir(&dir).map_err(|error| io_failure("read", &dir, error))?;
    let mut lines = Vec::new();
    for (visited, entry) in entries.enumerate() {
        if visited >= MAX_TEMPLATE_ENTRIES {
            return Err(Failure::index(
                format!(
                    "template directory {} exceeds 100000 entries",
                    dir.display()
                ),
                "pass --dir a directory that holds only compiled plans",
            ));
        }
        let entry = entry.map_err(|error| io_failure("read", &dir, error))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let bytes = StdFs
            .read_file_limited(&path, MAX_TEMPLATE_BYTES)
            .map_err(|error| io_failure("read", &path, error))?;
        match session.load_template(&bytes) {
            Ok(plan) => lines.push(format!(
                "ok {} source_version={} digest={}",
                path.display(),
                plan.source_version,
                hex(&plan.plan_digest)
            )),
            Err(error) => lines.push(format!("refused {} {error}", path.display())),
        }
    }
    if lines.is_empty() {
        lines.push(format!("no compiled plans in {}", dir.display()));
    }
    Ok(lines.join("\n"))
}

fn compiler() -> Result<dctx::Session, Failure> {
    host::open_memory().map_err(|message| {
        Failure::index(
            format!("cannot start the template compiler: {message}"),
            "run the command again; if it repeats, run `dctx doctor`",
        )
    })
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)] as char);
        out.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// Facade session used by the TUI template actions. No home path is consulted.
/// # Errors
/// Returns template argument, source reading, compilation, or output persistence failures.
pub fn build_path(path: &Path) -> Result<(), Failure> {
    println!(
        "{}",
        execute(&[
            "template".into(),
            "build".into(),
            path.display().to_string()
        ])?
    );
    Ok(())
}

/// TUI list. It does not scan a home template directory.
/// # Errors
/// Returns template directory discovery or listing failures.
pub fn list_default() -> Result<(), Failure> {
    println!("{}", execute(&["template".into(), "list".into()])?);
    Ok(())
}

/// Interactive template actions use the same explicit facade compiler as the command.
pub(crate) fn run_menu() {
    use crate::tui::{create_template_menu, get_input, show_error, show_message};
    loop {
        let result = match create_template_menu().show() {
            Ok('1') => {
                let _ = show_message("Already using default template.");
                continue;
            }
            Ok('2') => crate::template::list_default(),
            Ok('3') => match get_input("Enter template file path:") {
                Ok(path) if !path.is_empty() => crate::template::build_path(&PathBuf::from(path)),
                Ok(_) => {
                    let _ = show_error("No path provided.");
                    continue;
                }
                Err(error) => {
                    let _ = show_error(&format!("Error: {error}"));
                    continue;
                }
            },
            Ok('0') => return,
            Ok(_) => {
                let _ = show_error("Invalid selection. Try again.");
                continue;
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                return;
            }
        };
        match result {
            Ok(()) => {
                let _ = show_message("Press Enter to continue...");
            }
            Err(failure) => {
                let _ = show_error(&failure.render());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"
[DTS]
version = 2
[SELECT:engineering]
roots = ["src"]
languages = ["rust"]
[INDEX:engineering]
facts = ["symbols"]
parser = "structural"
[BUDGET:agent]
estimated_tokens = 500
records = 40
[VIEW:pack]
selection = "engineering"
budget = "agent"
layout = "SNAPSHOT"
"#;

    fn words(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    #[test]
    fn failures_carry_contract_codes_and_name_the_path() {
        for parts in [
            &["template", "frobnicate"][..],
            &["template", "build"],
            &["template", "build", "a", "b"],
            &["template", "build", "a", "--bogus"],
            &["template", "list", "--dir"],
        ] {
            let failure = execute(&words(parts)).expect_err("usage");
            assert_eq!(failure.code, crate::exit::USAGE, "{parts:?}");
            assert!(!failure.hint.is_empty());
        }
        let missing = std::env::temp_dir().join(format!("dctx-no-template-{}", std::process::id()));
        let shown = missing.display().to_string();
        let failure = execute(&words(&["template", "build", &shown])).expect_err("missing");
        assert_eq!(failure.code, crate::exit::INDEX);
        assert!(failure.message.contains(&shown), "{}", failure.message);
        assert!(!failure.hint.is_empty());
        let failure =
            execute(&words(&["template", "list", "--dir", &shown])).expect_err("missing dir");
        assert_eq!(failure.code, crate::exit::INDEX);
        assert!(failure.message.contains(&shown), "{}", failure.message);
    }

    #[test]
    fn a_template_that_does_not_compile_is_a_usage_error() {
        let dir = tempfile::tempdir().expect("temp");
        let source = dir.path().join("broken.dctx_template");
        std::fs::write(&source, "[DTS]\nversion = 99\n").expect("source");
        let failure = execute(&words(&[
            "template",
            "build",
            &source.display().to_string(),
        ]))
        .expect_err("broken");
        assert_eq!(failure.code, crate::exit::USAGE, "{failure:?}");
        assert!(failure.message.contains("does not compile"));
    }

    #[cfg(unix)]
    #[test]
    fn output_never_writes_through_a_symlink_or_into_a_non_regular_target() {
        let dir = tempfile::tempdir().expect("temp");
        let source = dir.path().join("policy.dctx_template");
        std::fs::write(&source, SOURCE).expect("source");
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, "keep me").expect("victim");
        let link = dir.path().join("plan.bin");
        std::os::unix::fs::symlink(&victim, &link).expect("link");
        let build = |output: &Path| {
            execute(&words(&[
                "template",
                "build",
                &source.display().to_string(),
                "--output",
                &output.display().to_string(),
            ]))
        };
        let failure = build(&link).expect_err("symlink");
        assert_eq!(failure.code, crate::exit::INDEX);
        assert!(
            failure.message.contains("symbolic link"),
            "{}",
            failure.message
        );
        assert_eq!(std::fs::read_to_string(&victim).expect("victim"), "keep me");
        let failure = build(dir.path()).expect_err("directory");
        assert_eq!(failure.code, crate::exit::INDEX);
        assert!(
            failure.message.contains("not a regular file"),
            "{}",
            failure.message
        );
        let plain = dir.path().join("plain.bin");
        std::fs::write(&plain, "old").expect("old");
        build(&plain).expect("overwrite a regular file");
        assert_ne!(std::fs::read(&plain).expect("plan"), b"old");
        let leftovers = std::fs::read_dir(dir.path())
            .expect("list")
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".dctx-template-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn build_without_output_creates_no_file() {
        let dir = std::env::temp_dir().join(format!("dctx-cli-template-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp");
        let source = dir.join("policy.dctx_template");
        std::fs::write(&source, SOURCE).expect("source");
        let report = execute(&[
            "template".into(),
            "build".into(),
            source.display().to_string(),
        ])
        .expect("build");
        assert!(report.contains("source_version=2"));
        assert!(report.contains("retention=stdout"));
        let listed: Vec<_> = std::fs::read_dir(&dir).expect("list").flatten().collect();
        assert_eq!(listed.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_round_trips_through_the_facade_loader() {
        let dir =
            std::env::temp_dir().join(format!("dctx-cli-template-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp");
        let source = dir.join("policy.dctx_template");
        let output = dir.join("policy.bin");
        std::fs::write(&source, SOURCE).expect("source");
        let report = execute(&[
            "template".into(),
            "build".into(),
            source.display().to_string(),
            "--output".into(),
            output.display().to_string(),
        ])
        .expect("build");
        assert!(report.contains("retention=explicit"));
        let listed = execute(&[
            "template".into(),
            "list".into(),
            "--dir".into(),
            dir.display().to_string(),
        ])
        .expect("list");
        assert!(listed.contains("ok "));
        assert!(listed.contains("source_version=2"));
        let bare = execute(&["template".into(), "list".into()]).expect("bare");
        assert!(bare.contains("does not scan the home directory"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
