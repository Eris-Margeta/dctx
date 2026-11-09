// SPDX-License-Identifier: Apache-2.0
//! Thin CLI adapter for engine-owned named context and delivery sessions.

use std::cell::RefCell;
use std::path::PathBuf;

use dctx::context::{ContextSessions, NamedContextRequest};
use dctx_cli::exit::{self, CommandOutput, Failure};

const HELP: &str = "Usage: dctx context TARGET [TARGET ...] [--place PATH] [--budget N] [--json] [--session ID --dedupe]\nReturn minimal ranked context for a file, symbol name or stable node id.\nRepeated targets share a delivery session within this invocation. MCP/RPC retain sessions\nuntil the connection closes. --dedupe requires --session; new processes emit full chunks.\n";

/// Connection-local engine delivery state. The adapter only parses and transports pages.
#[derive(Default)]
pub(crate) struct Runtime(RefCell<ContextSessions>);

impl Runtime {
    pub(crate) fn output(&self, args: &[String]) -> Result<CommandOutput, Failure> {
        if args.is_empty() {
            return Err(Failure::usage(
                "context requires TARGET",
                "name a file, symbol or stable id, as `dctx context src/main.rs`; run `dctx context --help` for options",
            ));
        }
        if has_flag(args, "--help") || has_flag(args, "-h") {
            return Ok(CommandOutput {
                stdout: HELP.into(),
                failure: None,
            });
        }
        let (place, requests) = parse(args).map_err(usage)?;
        // Validate every request before opening a source or emitting any page.
        let (mut session, key) = dctx_cli::host::open_mapped(&place)
            .map_err(|message| Failure::index(message, "pass an existing --place PATH"))?;
        let handle = session
            .snapshot_reusable(&dctx::SnapshotRequest::strict(), &key)
            .map_err(facade)?;
        let mut stdout = String::new();
        let mut missing = false;
        let mut history = self.0.borrow().clone();
        for request in &requests {
            let report = history
                .context(&mut session, &handle, request)
                .map_err(facade)?;
            missing |= report.chunks.is_empty();
            stdout.push_str(&report.text);
        }
        *self.0.borrow_mut() = history;
        Ok(CommandOutput {
            stdout,
            failure: missing.then(|| {
                Failure::no_result(
                    "context: target not found",
                    "name a file or symbol from `dctx map find NAME`",
                )
            }),
        })
    }

    pub(crate) fn write_map(
        &self,
        args: &[String],
        out: &mut dyn std::io::Write,
    ) -> Result<(), String> {
        if args.first().is_some_and(|arg| arg == "context") {
            let output = self
                .output(&args[1..])
                .map_err(|failure| failure.render())?;
            out.write_all(output.stdout.as_bytes())
                .map_err(|error| error.to_string())
        } else {
            crate::map::write_page(args, out)
        }
    }
}

/// Finds an option without treating an opaque option value or a literal target as a flag.
pub(crate) fn has_flag(args: &[String], flag: &str) -> bool {
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--" => break,
            value if value == flag => return true,
            "--place" | "--budget" | "--session" | "--for" => {
                words.next();
            }
            _ => {}
        }
    }
    false
}

fn facade(error: dctx::DctxError) -> Failure {
    exit::from_facade(&error, "dctx context --help")
}

fn usage(message: impl Into<String>) -> Failure {
    Failure::usage(message, "run `dctx context --help` for options")
}

fn parse(args: &[String]) -> Result<(PathBuf, Vec<NamedContextRequest>), String> {
    let mut place = PathBuf::from(".");
    let mut base = NamedContextRequest::new("");
    let mut targets = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut positional = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if !positional && arg == "--" {
            positional = true;
            continue;
        }
        if !positional && arg.starts_with('-') {
            if !seen.insert(arg.as_str()) {
                return Err(format!("duplicate option {arg}"));
            }
            match arg.as_str() {
                "--json" => base.output_format = 2,
                "--dedupe" => base.dedupe = true,
                "--place" | "--budget" | "--session" => {
                    let value = args
                        .next()
                        .ok_or_else(|| format!("{arg} requires a value"))?;
                    match arg.as_str() {
                        "--place" => place = PathBuf::from(value),
                        "--session" => base.session_id = Some(value.clone()),
                        _ => {
                            base.budget =
                                value.parse().map_err(|_| "invalid --budget".to_string())?
                        }
                    }
                }
                _ => return Err(format!("unknown context option {arg}")),
            }
        } else {
            targets.push(arg.clone());
        }
    }
    if targets.is_empty() {
        return Err("context requires TARGET".into());
    }
    if targets.len() > 64 {
        return Err("context accepts at most 64 targets".into());
    }
    let requests = targets
        .into_iter()
        .map(|target| {
            let mut request = base.clone();
            request.target = target;
            request.validate().map_err(|error| error.to_string())?;
            Ok(request)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((place, requests))
}
