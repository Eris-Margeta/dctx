//! DX-A1 baseline for the current dctx map mode.
//! Implements ARCHITECTURE-PROPOSAL §3 and §7 Stage 0.
//! Depends on the map query library; `run.sh` and the binary call `run`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;
mod fixture;
mod host;
mod manifest;
mod measure;
mod pin;
mod queries;
mod report;
mod stats;
mod verbs;

use crate::error::BenchError;
use crate::manifest::Manifest;
use crate::measure::corpus_cache;
use crate::report::{BaselineReport, CorpusReport, markdown, to_json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Failure returned to `run.sh`.
pub use crate::error::BenchError as Error;

/// Paths and the optional single-corpus child mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Repository root that contains the dctx crates.
    pub repo: PathBuf,
    /// Scratch directory inside this packet. The fixture is written here.
    pub scratch: PathBuf,
    /// Directory for `baseline.json` and `baseline.md`.
    pub reports: PathBuf,
    /// Cache root outside every measured place.
    pub cache: PathBuf,
    /// Checked-in corpus recipe.
    pub manifest: PathBuf,
    /// `dctx` or `fixture` when this process measures one corpus.
    pub only: Option<String>,
    /// Child report path. Required with `only`.
    pub out: Option<PathBuf>,
}

/// Parses the runner arguments. Unknown flags are refused.
pub fn parse_args(args: &[String]) -> Result<Options, BenchError> {
    let mut repo = None;
    let mut scratch = None;
    let mut reports = None;
    let mut cache = None;
    let mut manifest = None;
    let mut only = None;
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        let flag = &args[index];
        if flag == "--help" || flag == "-h" {
            return Err(BenchError::Message(usage()));
        }
        let value = args.get(index + 1).ok_or_else(|| {
            BenchError::Message(format!("missing value for {flag}"))
        })?;
        match flag.as_str() {
            "--repo" => repo = Some(PathBuf::from(value)),
            "--scratch" => scratch = Some(PathBuf::from(value)),
            "--reports" => reports = Some(PathBuf::from(value)),
            "--cache" => cache = Some(PathBuf::from(value)),
            "--manifest" => manifest = Some(PathBuf::from(value)),
            "--only" => only = Some(value.clone()),
            "--out" => out = Some(PathBuf::from(value)),
            other => {
                return Err(BenchError::Message(format!(
                    "unknown argument {other}; {usage}",
                    usage = usage()
                )))
            }
        }
        index += 2;
    }
    let options = Options {
        repo: required(repo, "--repo")?,
        scratch: required(scratch, "--scratch")?,
        reports: required(reports, "--reports")?,
        cache: required(cache, "--cache")?,
        manifest: required(manifest, "--manifest")?,
        only,
        out,
    };
    match (&options.only, &options.out) {
        (Some(id), None) => Err(BenchError::Message(format!(
            "--only {id} requires --out"
        ))),
        (None, Some(_)) => Err(BenchError::Message("--out is only valid with --only".into())),
        (Some(id), Some(_)) if id != "dctx" && id != "fixture" => Err(BenchError::Message(format!(
            "--only {id} must be dctx or fixture"
        ))),
        _ => Ok(options),
    }
}

fn required(value: Option<PathBuf>, flag: &str) -> Result<PathBuf, BenchError> {
    value.ok_or_else(|| BenchError::Message(format!("missing {flag}")))
}

fn usage() -> String {
    "usage: architecture-bench --repo PATH --scratch PATH --reports PATH --cache PATH --manifest PATH [--only dctx|fixture --out PATH]".into()
}

/// Runs one corpus or merges both into the reports directory.
pub fn run(options: &Options) -> Result<(), BenchError> {
    let manifest = Manifest::parse(&fs::read_to_string(&options.manifest)?)?;
    match options.only.as_deref() {
        Some("dctx") => {
            let report = measure::measure_dctx(
                &options.repo,
                &corpus_cache(&options.cache, &manifest.dctx.id),
                &manifest,
            )?;
            write_bytes(options.out.as_deref().ok_or_else(|| {
                BenchError::Message("--out is required".into())
            })?, &to_json(&report)?)
        }
        Some("fixture") => {
            let report = measure::measure_fixture(
                &options.scratch,
                &corpus_cache(&options.cache, &manifest.fixture.id),
                &manifest,
            )?;
            write_bytes(options.out.as_deref().ok_or_else(|| {
                BenchError::Message("--out is required".into())
            })?, &to_json(&report)?)
        }
        Some(other) => Err(BenchError::Message(format!("unknown corpus {other}"))),
        None => merge(options),
    }
}

fn merge(options: &Options) -> Result<(), BenchError> {
    if options.scratch.exists() {
        fs::remove_dir_all(&options.scratch)?;
    }
    fs::create_dir_all(&options.reports)?;
    for name in ["baseline.json", "baseline.md"] {
        let path = options.reports.join(name);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    let dctx_out = options.cache.join("dctx-report.json");
    let fixture_out = options.cache.join("fixture-report.json");
    spawn(options, "dctx", &dctx_out)?;
    spawn(options, "fixture", &fixture_out)?;
    let dctx = read_report(&dctx_out)?;
    let fixture = read_report(&fixture_out)?;
    let baseline = BaselineReport::new(vec![dctx, fixture]);
    write_bytes(&options.reports.join("baseline.json"), &to_json(&baseline)?)?;
    write_bytes(
        &options.reports.join("baseline.md"),
        markdown(&baseline).as_bytes(),
    )?;
    Ok(())
}

fn spawn(options: &Options, only: &str, out: &Path) -> Result<(), BenchError> {
    let exe = std::env::current_exe()?;
    let status = Command::new(exe)
        .arg("--repo")
        .arg(&options.repo)
        .arg("--scratch")
        .arg(&options.scratch)
        .arg("--reports")
        .arg(&options.reports)
        .arg("--cache")
        .arg(&options.cache)
        .arg("--manifest")
        .arg(&options.manifest)
        .arg("--only")
        .arg(only)
        .arg("--out")
        .arg(out)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(BenchError::Message(format!(
            "corpus {only} exited {status}"
        )))
    }
}

fn read_report(path: &Path) -> Result<CorpusReport, BenchError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), BenchError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Vec<String> {
        vec![
            "--repo",
            "/repo",
            "--scratch",
            "/repo/tools/architecture-bench/scratch",
            "--reports",
            "/repo/tools/architecture-bench/reports",
            "--cache",
            "/tmp/dx-a1-cache",
            "--manifest",
            "/repo/tools/architecture-bench/corpus/manifest.json",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn arguments_accept_the_parent_and_refuse_unknown_or_partial_children() {
        let options = parse_args(&base()).unwrap();
        assert!(options.only.is_none());
        assert_eq!(options.repo, PathBuf::from("/repo"));
        let mut unknown = base();
        unknown.push("--speed".into());
        unknown.push("fast".into());
        assert!(parse_args(&unknown).is_err());
        let mut child = base();
        child.push("--only".into());
        child.push("dctx".into());
        assert!(parse_args(&child).is_err());
        child.push("--out".into());
        child.push("/tmp/dctx.json".into());
        assert!(parse_args(&child).is_ok());
        let mut bad = base();
        bad.push("--only".into());
        bad.push("sqlite".into());
        bad.push("--out".into());
        bad.push("/tmp/x.json".into());
        assert!(parse_args(&bad).is_err());
        assert!(parse_args(&[]).is_err());
    }
}
