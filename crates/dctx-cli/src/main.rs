//! Command routing and legacy argument parsing (D3-LEGACY-REMOVAL §5 D3-04).
//! Snapshot and TUI operations use the CLI host library and pure pack/render APIs.
//! Plugin and map commands retain their explicit routes and facade sessions.
#![forbid(unsafe_code)]

mod map;
mod mcp;
mod pack_for;
mod plugin;

use dctx_cli::{cache, exit, help, host, languages, pack, run_headless_mode, template, tui, SnapshotConfig};
use exit::Failure;
use std::{env, ffi::OsString, path::{Path, PathBuf}};

#[derive(Debug)]
enum CliCommand {
    Snapshot(SnapshotConfig),
    Help,
    Version,
}

fn main() {
    let raw = match utf8_arguments(env::args_os()) {
        Ok(raw) => raw,
        Err(failure) => fail(&failure),
    };
    if raw.get(1).is_some_and(|arg| arg == "--internal-plugin-worker") {
        std::process::exit(i32::from(plugin::enter_worker(&raw[2..])));
    }
    let allow_auto_clean = host::auto_clean_allowed(&raw);
    let args = host::without_auto_flag(raw);
    let head = args.get(1).map(String::as_str);
    if head.is_some_and(plugin::is_early) {
        maybe_auto_clean(allow_auto_clean);
        std::process::exit(i32::from(plugin::enter(&args)));
    }
    // map, pack, mcp, languages and unknown commands follow docs/CLI-EXIT-CODES.md; cache and template keep exit 1.
    let result = match head {
        Some("map") => {
            maybe_auto_clean(allow_auto_clean);
            map::run(&args[2..])
        }
        // The MCP server answers on stdout in JSON-RPC lines only, so it never auto-cleans.
        Some("mcp") => mcp::run(&args[2..]).map_err(|failure| failure.within("mcp")),
        Some("pack") => {
            maybe_auto_clean(allow_auto_clean);
            // `--for`/`--budget` select the task-context page (map find, neighbors, read); everything else is the source page.
            if pack_for::requested(&args[2..]) {
                pack_for::run(&args[1..])
            } else {
                pack::run(&args[1..])
            }
        }
        Some("cache") => cache::run(&args[1..]).map_err(|error| legacy(format!("cache: {error}"))),
        // Reads only the facade registry: no source, no cache, so no auto-clean either.
        Some("languages") => languages::run(&args[2..])
            .map_err(|error| Failure::usage(format!("languages: {error}"), "run `dctx languages --help` for the formats and an example")),
        Some("template") => {
            maybe_auto_clean(allow_auto_clean);
            template::run(&args[1..]).map_err(|error| legacy(format!("template: {error}")))
        }
        None => {
            maybe_auto_clean(allow_auto_clean);
            tui::run();
            Ok(())
        }
        Some(word) if is_unknown_command(word) => Err(Failure::usage(
            format!("unknown command `{word}`"),
            "run `dctx --help` for the commands; `dctx PATH` snapshots an existing directory",
        )),
        _ => parse_command(&args)
            .map(|command| match command {
                CliCommand::Help => print!("{}", help::text()),
                CliCommand::Version => {
                    // DCTX_GIT_SHA comes from build.rs: the short commit of the built tree, or `unknown`.
                    println!("dctx {} ({})", env!("CARGO_PKG_VERSION"), env!("DCTX_GIT_SHA"));
                    if args.iter().any(|arg| arg == "--verbose") {
                        print!("{}", help::limits());
                    }
                }
                CliCommand::Snapshot(config) => run_headless_mode(config),
            })
            .map_err(|error| Failure::usage(error, "run `dctx --help` for the snapshot options")),
    };
    if let Err(failure) = result {
        fail(&failure);
    }
}

/// Prints `failure` (message, then hint) on stderr and exits with its code.
fn fail(failure: &Failure) -> ! {
    eprint!("{}", failure.render());
    std::process::exit(failure.code);
}

/// A failure of a command outside the exit-code contract: exit 1 as before, with no hint.
fn legacy(message: String) -> Failure {
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

/// True for a first word that is neither a flag, `help`, `version` nor an existing path: the legacy `dctx PATH` cannot apply.
fn is_unknown_command(word: &str) -> bool {
    !word.starts_with('-') && !matches!(word, "help" | "version") && !Path::new(word).exists()
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
        _ => parse_snapshot_args(&args[1..]),
    }
}

fn parse_snapshot_args(args: &[String]) -> Result<CliCommand, String> {
    let mut config = SnapshotConfig::default();
    let mut path_set = false;
    let mut empty = false;
    let mut non_empty = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-c" | "--clipboard" => config.clipboard_mode = true,
            "-d" | "--diff" => config.diff_mode = true,
            "-t" | "--tree" => config.tree_only = true,
            "-r" | "--reset" => config.force_reset = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            "-v" | "--version" => return Ok(CliCommand::Version),
            "--binary" => config.filter_config = config.filter_config.with_include_binary(true),
            "--empty" => {
                empty = true;
                config.filter_config = config.filter_config.with_only_empty(true);
            }
            "--non-empty" => {
                non_empty = true;
                config.filter_config = config.filter_config.with_only_non_empty(true);
            }
            "--hidden" => config.filter_config = config.filter_config.with_only_hidden(true),
            "--debug-ignored" => {
                config.debug_ignored = true;
                config.filter_config = config.filter_config.with_collect_ignored(true);
            }
            "--debug-verbose" | "-V" => config.debug_verbose = true,
            "--tree-stamps" => {
                index += 1;
                let value = args.get(index).ok_or("--tree-stamps requires on or off")?;
                config.tree_stamps = dctx_template::TreeStamps::parse(value)?;
            }
            flag if flag.starts_with("--tree-stamps=") => {
                config.tree_stamps = dctx_template::TreeStamps::parse(&flag["--tree-stamps=".len()..])?;
            }
            "--ext" => {
                index += 1;
                let value = args.get(index).ok_or("--ext requires a comma-separated list of extensions")?;
                let extensions: Vec<_> =
                    value.split(',').map(str::trim).filter(|part| !part.is_empty()).map(str::to_string).collect();
                if extensions.is_empty() {
                    return Err("--ext requires at least one extension".into());
                }
                config.filter_config = config.filter_config.with_extensions(extensions);
            }
            flag @ ("--large" | "--small") => {
                index += 1;
                let value = args.get(index).ok_or_else(|| format!("{flag} requires a size in megabytes"))?;
                let size = value.parse::<f64>().map_err(|_| format!("Invalid size for {flag}: {value}"))?;
                config.filter_config = if flag == "--large" {
                    config.filter_config.with_min_size_mb(size)
                } else {
                    config.filter_config.with_max_size_mb(size)
                };
            }
            value => {
                if value.starts_with('-') {
                    return Err(format!("Unknown flag: {value}"));
                }
                if path_set {
                    return Err("Multiple target paths provided.".into());
                }
                config.target_path = PathBuf::from(value);
                path_set = true;
            }
        }
        index += 1;
    }
    if empty && non_empty {
        return Err("Cannot use both --empty and --non-empty flags".into());
    }
    Ok(CliCommand::Snapshot(config))
}

#[cfg(test)]
mod compatibility {
    #[test]
    fn dot_alias_stays_a_snapshot() {
        let args = vec!["dctx".to_string(), ".".to_string()];
        assert!(matches!(super::parse_command(&args).expect("parse"), super::CliCommand::Snapshot(_)));
    }

    #[test]
    fn plugin_and_git_are_not_snapshot_paths() {
        assert!(super::plugin::is_early("plugin"));
        assert!(super::plugin::is_early("git"));
        assert!(!super::plugin::is_early("."));
    }

    #[test]
    fn unknown_words_are_commands_and_paths_stay_snapshots() {
        assert!(super::is_unknown_command("frobnicate-dctx-v1-5"));
        assert!(!super::is_unknown_command("."));
        assert!(!super::is_unknown_command("help"));
        assert!(!super::is_unknown_command("--bogus"));
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_argument_is_a_usage_error_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let raw = vec![std::ffi::OsString::from("dctx"), "map".into(), "read".into(), std::ffi::OsString::from_vec(vec![0xff, 0xfe, b'.', b'p', b'y'])];
        let failure = super::utf8_arguments(raw.into_iter()).expect_err("non-UTF-8");
        assert_eq!(failure.code, super::exit::USAGE);
        assert!(failure.message.contains("argument 3 is not valid UTF-8"), "{}", failure.message);
        let program = vec![std::ffi::OsString::from_vec(vec![b'd', 0xff]), "map".into()];
        assert_eq!(super::utf8_arguments(program.into_iter()).expect("lossy name")[1], "map");
    }

    #[test]
    fn help_is_still_a_command() {
        let args = vec!["dctx".to_string(), "--help".to_string()];
        assert!(matches!(super::parse_command(&args).expect("parse"), super::CliCommand::Help));
    }
}
