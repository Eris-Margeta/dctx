// SPDX-License-Identifier: Apache-2.0
//! Read-only binary discovery, ownership scopes and per-engine cache diagnostics.
//! Version probes use only `--version`, with auto-clean disabled and bounded time/output.

mod discover;
mod inspect;
mod render;

use std::{env, path::PathBuf};

use crate::{exit::Failure, host};
use discover::Context;

const HELP: &str = "Usage: dctx doctor [--json] [--probe-project]\n\nList dctx installations visible from this directory, their ownership scopes, versions,\nbuild commits, SHA-256 checksums and version-scoped cache sizes. No network or cache changes.\nSearches PATH, ./node_modules/.bin, VIRTUAL_ENV/bin (Scripts on Windows), and DCTX_BIN.\nThe running executable is always included. Unavailable fields are reported with errors.\n\nOnly the running executable, DCTX_BIN and files in absolute PATH entries are asked for\n`--version`. Candidates in ./node_modules/.bin, VIRTUAL_ENV or relative PATH entries may come\nfrom an untrusted checkout, so they are checksummed but never executed unless\n--probe-project is given.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    System,
    Project,
    Explicit,
}

impl Scope {
    fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Project => "project",
            Self::Explicit => "explicit",
        }
    }
}

struct Binary {
    path: PathBuf,
    real_path: Option<PathBuf>,
    scope: Scope,
    running: bool,
    version: Option<String>,
    commit: Option<String>,
    cache_root: Option<PathBuf>,
    cache_size_bytes: Option<u64>,
    sha256: Option<String>,
    errors: Vec<String>,
}

/// Lists visible binaries without opening a cache session or running auto-clean.
/// # Errors
/// Returns usage errors or a failure to resolve the working/running directory.
pub fn run(arguments: &[String]) -> Result<(), Failure> {
    let (mut json, mut probe_project) = (false, false);
    for argument in arguments {
        match argument.as_str() {
            "--json" if !json => json = true,
            "--probe-project" if !probe_project => probe_project = true,
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "--json" | "--probe-project" => {
                return Err(Failure::usage(
                    format!("doctor: duplicate argument `{}`", render::display(argument)),
                    "run `dctx doctor --help`",
                ))
            }
            _ => {
                return Err(Failure::usage(
                    format!("doctor: invalid argument `{}`", render::display(argument)),
                    "run `dctx doctor --help`",
                ))
            }
        }
    }
    let context = Context::from_env().map_err(|error| {
        Failure::new(1, error, "change to an existing directory and run `dctx doctor` again; if the executable is missing, reinstall dctx")
    })?;
    let binaries = inspect::binaries(&context, host::cache_base_dir(), probe_project);
    print!("{}", render::report(&binaries, json));
    Ok(())
}

/// Executable path and ownership scope for `--version --verbose`; performs no version probe.
/// # Errors
/// Returns failure to discover the current directory or running executable.
pub fn executable_details() -> Result<String, String> {
    let context = Context::from_env()?;
    Ok(format!(
        "executable: {}\nscope: {}\n",
        render::display(
            &env::current_exe()
                .map_err(|error| error.to_string())?
                .to_string_lossy()
        ),
        context.scope(&context.invocation_path()).as_str(),
    ))
}

#[cfg(test)]
mod tests;
