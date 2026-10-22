//! Template build and list through the facade compiler.
//! Implements ARCHITECTURE-PROPOSAL §2 template compilation and §4 explicit outputs.
//! Depends on [`dctx::Session::compile_template`] and [`dctx::Session::load_template`].
//! Build writes a plan only when `--output` is set. List does not scan the home directory.

use std::path::{Path, PathBuf};

use dctx_template::to_bytes;

use crate::host;

/// Prints the result of `template build` or `template list`.
/// # Errors
/// Returns template command parsing, compilation, loading, or listing failures.
pub fn run(args: &[String]) -> Result<(), String> {
    println!("{}", execute(args)?);
    Ok(())
}

/// Compiles or lists and returns the report. `--output` is the only write.
/// # Errors
/// Returns missing/unknown actions, invalid arguments, or template filesystem/compilation failures.
pub fn execute(args: &[String]) -> Result<String, String> {
    if args.first().map(String::as_str) != Some("template") {
        return Err("expected template".to_string());
    }
    if args.len() == 1 || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(crate::help::text());
    }
    match args[1].as_str() {
        "build" => build(&args[2..]),
        "list" => list(&args[2..]),
        other => Err(format!("unknown template action: {other}. Use build or list")),
    }
}

fn build(args: &[String]) -> Result<String, String> {
    let mut source_path: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--output" | "-o" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| "--output requires a path".to_string())?;
                if output.is_some() {
                    return Err("template build accepts one --output path".to_string());
                }
                output = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => return Err(format!("unknown template flag: {other}")),
            other => {
                if source_path.is_some() {
                    return Err("template build accepts one source path".to_string());
                }
                source_path = Some(PathBuf::from(other));
            }
        }
        index += 1;
    }
    let source_path = source_path.ok_or_else(|| "template build requires a template path".to_string())?;
    let source = std::fs::read_to_string(&source_path).map_err(|error| format!("failed to read template: {error}"))?;
    let session = compiler()?;
    let plan = session.compile_template(&source).map_err(|error| error.to_string())?;
    let bytes = to_bytes(&plan).map_err(|error| error.to_string())?;
    let digest = hex(&plan.plan_digest);
    let retention = if let Some(path) = &output {
        std::fs::write(path, &bytes).map_err(|error| format!("failed to write {}: {error}", path.display()))?;
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

fn list(args: &[String]) -> Result<String, String> {
    let mut dir: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--dir" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| "--dir requires a path".to_string())?;
                dir = Some(PathBuf::from(value));
            }
            other if other.starts_with('-') => return Err(format!("unknown template flag: {other}")),
            other => return Err(format!("unknown template list argument: {other}")),
        }
        index += 1;
    }
    let Some(dir) = dir else {
        return Ok(
            "template list does not scan the home directory. Pass --dir PATH to load explicit compiled plans through the facade.\n"
                .to_string(),
        );
    };
    let session = compiler()?;
    let entries = std::fs::read_dir(&dir).map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
    let mut lines = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
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

fn compiler() -> Result<dctx::Session, String> {
    host::open_memory()
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
pub fn build_path(path: &Path) -> Result<(), String> {
    println!("{}", execute(&["template".into(), "build".into(), path.display().to_string()])?);
    Ok(())
}

/// TUI list. It does not scan a home template directory.
/// # Errors
/// Returns template directory discovery or listing failures.
pub fn list_default() -> Result<(), String> {
    println!("{}", execute(&["template".into(), "list".into()])?);
    Ok(())
}

/// Load the legacy default asset or a named compiled-v1 template from the legacy cache.
/// # Errors
/// Returns cache-path discovery, compiled template reading, or template decoding failures.
pub fn legacy_template(name: &str) -> Result<dctx_template::Template, String> {
    if name == "default" {
        return Ok(dctx_template::Template::parse(include_str!("../../dctx-template/templates/default.dctx_template")));
    }
    let cache = crate::compat::cache_dir().ok_or("Could not determine compiled template path")?;
    let path = cache.join("templates").join(format!("{name}.bin"));
    let bytes =
        std::fs::read(&path).map_err(|error| format!("Compiled template '{name}' at {}: {error}", path.display()))?;
    dctx_template::CompiledTemplate::deserialize(&mut bytes.as_slice())
        .map(|compiled| compiled.to_template())
        .map_err(|error| error.to_string())
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
        if let Err(error) = result {
            eprintln!("❌ {error}");
            std::process::exit(1);
        }
        let _ = show_message("Press Enter to continue...");
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

    #[test]
    fn build_without_output_creates_no_file() {
        let dir = std::env::temp_dir().join(format!("dctx-cli-template-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp");
        let source = dir.join("policy.dctx_template");
        std::fs::write(&source, SOURCE).expect("source");
        let report = execute(&["template".into(), "build".into(), source.display().to_string()]).expect("build");
        assert!(report.contains("source_version=2"));
        assert!(report.contains("retention=stdout"));
        let listed: Vec<_> = std::fs::read_dir(&dir).expect("list").flatten().collect();
        assert_eq!(listed.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_round_trips_through_the_facade_loader() {
        let dir = std::env::temp_dir().join(format!("dctx-cli-template-out-{}", std::process::id()));
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
        let listed =
            execute(&["template".into(), "list".into(), "--dir".into(), dir.display().to_string()]).expect("list");
        assert!(listed.contains("ok "));
        assert!(listed.contains("source_version=2"));
        let bare = execute(&["template".into(), "list".into()]).expect("bare");
        assert!(bare.contains("does not scan the home directory"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
