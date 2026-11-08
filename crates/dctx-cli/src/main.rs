// SPDX-License-Identifier: Apache-2.0
//! Explicit command routing for pack, map, plugins and host operations.
//! Plugin and map commands retain their explicit routes and facade sessions.
#![forbid(unsafe_code)]

mod context_cmd;
mod map;
mod mcp;
mod pack_for;
mod plugin;
mod rpc;

use dctx_cli::{cache, doctor, exit, help, host, languages, pack, template, tui};
use exit::Failure;
use std::{env, ffi::OsString};

#[derive(Debug)]
enum CliCommand {
    Help,
    Version,
}

fn main() {
    exit_quietly_on_broken_pipe();
    let raw = match utf8_arguments(env::args_os()) {
        Ok(raw) => raw,
        Err(failure) => fail(&failure),
    };
    if raw
        .get(1)
        .is_some_and(|arg| arg == "--internal-plugin-worker")
    {
        std::process::exit(i32::from(plugin::enter_worker(&raw[2..])));
    }
    let allow_auto_clean = host::auto_clean_allowed(&raw);
    let args = host::without_auto_flag(raw);
    let head = args.get(1).map(String::as_str);
    if head.is_some_and(plugin::is_early) {
        maybe_auto_clean(allow_auto_clean);
        std::process::exit(i32::from(plugin::enter(&args)));
    }
    // map, pack, mcp, languages and unknown commands follow docs/CLI-EXIT-CODES.md; cache and template use exit 1.
    let result = match head {
        Some("doctor") => doctor::run(&args[2..]),
        Some("map") => {
            maybe_auto_clean(allow_auto_clean);
            map::run(&args[2..])
        }
        // The MCP server answers on stdout in JSON-RPC lines only, so it never auto-cleans.
        Some("rpc") => rpc::run(&args[2..]).map_err(|failure| failure.within("rpc")),
        Some("mcp") => mcp::run(&args[2..]).map_err(|failure| failure.within("mcp")),
        Some("serve") => {
            let words: Vec<_> = args[2..]
                .iter()
                .filter(|word| word.as_str() != "--mcp")
                .cloned()
                .collect();
            mcp::run(&words).map_err(|failure| failure.within("serve"))
        }
        Some("context") => {
            maybe_auto_clean(allow_auto_clean);
            context_cmd::Runtime::default()
                .output(&args[2..])
                .and_then(exit::CommandOutput::emit)
        }
        Some("read") => {
            maybe_auto_clean(allow_auto_clean);
            map::run(&args[1..])
        }
        Some("pack") => {
            maybe_auto_clean(allow_auto_clean);
            // `--for`/`--budget` select the task-context page (map find, neighbors, read); everything else is the source page.
            if pack_for::requested(&args[2..]) {
                pack_for::run(&args[1..])
            } else {
                pack::run(&args[1..])
            }
        }
        Some("cache") => {
            cache::run(&args[1..]).map_err(|error| command_error(format!("cache: {error}")))
        }
        // Reads only the facade registry: no source, no cache, so no auto-clean either.
        Some("languages") => languages::output(&args[2..]).and_then(exit::CommandOutput::emit),
        Some("template") => {
            maybe_auto_clean(allow_auto_clean);
            template::run(&args[1..]).map_err(|error| command_error(format!("template: {error}")))
        }
        None => {
            maybe_auto_clean(allow_auto_clean);
            tui::run();
            Ok(())
        }
        Some(word) if is_unknown_command(word) => Err(Failure::usage(
            format!("unknown command `{word}`"),
            "run `dctx --help` for commands; use `dctx pack PATH` to pack a directory",
        )),
        _ => parse_command(&args)
            .map(|command| match command {
                CliCommand::Help => print!("{}", help::text()),
                CliCommand::Version => {
                    // DCTX_GIT_SHA comes from build.rs: the short commit of the built tree, or `unknown`.
                    println!(
                        "dctx {} ({})",
                        env!("CARGO_PKG_VERSION"),
                        env!("DCTX_GIT_SHA")
                    );
                    if args.iter().any(|arg| arg == "--verbose") {
                        match doctor::executable_details() {
                            Ok(details) => print!("{details}"),
                            Err(error) => eprintln!("dctx: {error}"),
                        }
                        print!("{}", help::limits());
                    }
                }
            })
            .map_err(|error| Failure::usage(error, "run `dctx --help` for commands")),
    };
    if let Err(failure) = result {
        fail(&failure);
    }
}

/// A reader that closes stdout early (`dctx pack . | head -1`) ends the run with exit 0 and no message.
/// Rust ignores SIGPIPE, so the write returns EPIPE and `print!` panics with "failed printing to stdout";
/// that one panic is turned into a quiet exit, as `cat` and `rg` behave. Every other panic keeps the default report.
fn exit_quietly_on_broken_pipe() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        if message.is_some_and(is_broken_stdout) {
            std::process::exit(0);
        }
        default(info);
    }));
}

/// True for the std panic text of a stdout write that hit a closed pipe (Unix EPIPE, Windows ERROR_NO_DATA).
fn is_broken_stdout(message: &str) -> bool {
    message.starts_with("failed printing to stdout")
        && (message.contains("Broken pipe") || message.contains("os error 232"))
}

/// Prints `failure` (message, then hint) on stderr and exits with its code.
fn fail(failure: &Failure) -> ! {
    eprint!("{}", failure.render());
    std::process::exit(failure.code);
}

/// A failure of a command outside the exit-code contract: exit 1 as before, with no hint.
fn command_error(message: String) -> Failure {
    Failure::new(1, message, "")
}

/// The process arguments as UTF-8. `env::args` panics on a non-UTF-8 argument; this refuses it with exit 2 instead.
/// The program name (argument 0) is converted lossily, as it is never parsed.
fn utf8_arguments(raw: impl Iterator<Item = OsString>) -> Result<Vec<String>, Failure> {
    raw.enumerate()
        .map(|(index, argument)| match argument.into_string() {
            Ok(text) => Ok(text),
            Err(original) if index == 0 => Ok(original.to_string_lossy().into_owned()),
            Err(_) => Err(Failure::usage(
                format!("argument {index} is not valid UTF-8"),
                "dctx takes UTF-8 arguments: rename the file, or pass its stable id from `dctx map find <name>`",
            )),
        })
        .collect()
}

/// True for a first word that is neither a flag, `help` nor `version`.
fn is_unknown_command(word: &str) -> bool {
    !word.starts_with('-') && !matches!(word, "help" | "version")
}

fn maybe_auto_clean(allow: bool) {
    if allow {
        host::piggyback_auto_clean();
    }
}

fn parse_command(args: &[String]) -> Result<CliCommand, String> {
    match args.get(1).map(String::as_str) {
        Some("-h" | "--help" | "help") => Ok(CliCommand::Help),
        Some("-v" | "--version" | "version") => Ok(CliCommand::Version),
        Some(other) => Err(format!("unknown option `{other}`")),
        None => Err("a command is required".into()),
    }
}

#[cfg(test)]
mod arguments {
    #[test]
    fn positional_paths_are_not_commands() {
        assert!(super::is_unknown_command("."));
        assert!(super::is_unknown_command("frobnicate-dctx-v1-5"));
        assert!(!super::is_unknown_command("help"));
        assert!(!super::is_unknown_command("--bogus"));
        assert!(super::plugin::is_early("plugin"));
        assert!(super::plugin::is_early("git"));
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_argument_is_a_usage_error_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let raw = vec![
            std::ffi::OsString::from("dctx"),
            "map".into(),
            "read".into(),
            std::ffi::OsString::from_vec(vec![0xff, 0xfe, b'.', b'p', b'y']),
        ];
        let failure = super::utf8_arguments(raw.into_iter()).expect_err("non-UTF-8");
        assert_eq!(failure.code, super::exit::USAGE);
        assert!(
            failure.message.contains("argument 3 is not valid UTF-8"),
            "{}",
            failure.message
        );
        let program = vec![std::ffi::OsString::from_vec(vec![b'd', 0xff]), "map".into()];
        assert_eq!(
            super::utf8_arguments(program.into_iter()).expect("lossy name")[1],
            "map"
        );
    }

    #[test]
    fn only_a_closed_stdout_pipe_is_quiet() {
        assert!(super::is_broken_stdout(
            "failed printing to stdout: Broken pipe (os error 32)"
        ));
        assert!(!super::is_broken_stdout(
            "failed printing to stdout: No space left on device (os error 28)"
        ));
        assert!(!super::is_broken_stdout(
            "failed printing to stderr: Broken pipe (os error 32)"
        ));
        assert!(!super::is_broken_stdout(
            "called `Option::unwrap()` on a `None` value"
        ));
    }

    #[test]
    fn help_is_still_a_command() {
        let args = vec!["dctx".to_string(), "--help".to_string()];
        assert!(matches!(
            super::parse_command(&args).expect("parse"),
            super::CliCommand::Help
        ));
    }
}

#[cfg(test)]
mod robustness;
