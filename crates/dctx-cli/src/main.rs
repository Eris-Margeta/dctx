//! Command routing and legacy argument parsing (D3-LEGACY-REMOVAL §5 D3-04).
//! Snapshot and TUI operations use the CLI host library and pure pack/render APIs.
//! Plugin and map commands retain their explicit routes and facade sessions.
#![forbid(unsafe_code)]

mod map;
mod plugin;

use dctx_cli::{cache, help, host, pack, run_headless_mode, template, tui, SnapshotConfig};
use std::{env, path::PathBuf};

#[derive(Debug)]
enum CliCommand {
    Snapshot(SnapshotConfig),
    Help,
    Version,
}

fn main() {
    let raw: Vec<String> = env::args().collect();
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
    let result = match head {
        Some("map") => {
            maybe_auto_clean(allow_auto_clean);
            map::run(&args[2..]).map_err(|error| format!("map: {error}"))
        }
        Some("pack") => {
            maybe_auto_clean(allow_auto_clean);
            pack::run(&args[1..]).map_err(|error| format!("pack: {error}"))
        }
        Some("cache") => cache::run(&args[1..]).map_err(|error| format!("cache: {error}")),
        Some("template") => {
            maybe_auto_clean(allow_auto_clean);
            template::run(&args[1..]).map_err(|error| format!("template: {error}"))
        }
        None => {
            maybe_auto_clean(allow_auto_clean);
            tui::run();
            Ok(())
        }
        _ => parse_command(&args).map(|command| match command {
            CliCommand::Help => print!("{}", help::text()),
            CliCommand::Version => {
                println!("dctx-cli v{}", env!("CARGO_PKG_VERSION"));
                if args.iter().any(|arg| arg == "--verbose") {
                    print!("{}", help::limits());
                }
            }
            CliCommand::Snapshot(config) => run_headless_mode(config),
        }),
    };
    if let Err(error) = result {
        eprintln!("❌ Error: {error}");
        std::process::exit(1);
    }
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
    fn help_is_still_a_command() {
        let args = vec!["dctx".to_string(), "--help".to_string()];
        assert!(matches!(super::parse_command(&args).expect("parse"), super::CliCommand::Help));
    }
}
