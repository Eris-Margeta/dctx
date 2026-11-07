// SPDX-License-Identifier: Apache-2.0
//! Read-only binary discovery, ownership scopes and per-engine cache diagnostics.
//! Version probes use only `--version`, with auto-clean disabled and bounded time/output.

mod discover;
mod inspect;
mod render;

use std::{env, path::PathBuf};

use crate::{exit::Failure, host};
use discover::Context;

const HELP: &str = "Usage: dctx doctor [--json]\n\nList dctx installations visible from this directory, their ownership scopes, versions,\nSHA-256 checksums and version-scoped cache sizes. No network or cache changes.\nSearches PATH, ./node_modules/.bin, VIRTUAL_ENV/bin (Scripts on Windows), and DCTX_BIN.\nThe running executable is always included. Unavailable fields are reported with errors.\n";

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
    cache_root: Option<PathBuf>,
    cache_size_bytes: Option<u64>,
    sha256: Option<String>,
    errors: Vec<String>,
}

/// Lists visible binaries without opening a cache session or running auto-clean.
/// # Errors
/// Returns usage errors or a failure to resolve the working/running directory.
pub fn run(arguments: &[String]) -> Result<(), Failure> {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        [flag] if flag == "-h" || flag == "--help" => {
            print!("{HELP}");
            return Ok(());
        }
        _ => {
            return Err(Failure::usage(
                "invalid doctor arguments",
                "run `dctx doctor --help`",
            ))
        }
    };
    let context = Context::from_env().map_err(|error| Failure::new(1, error, ""))?;
    let binaries = inspect::binaries(&context, host::cache_base_dir());
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
