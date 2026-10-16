//! Local `plugin-index` command.
//! Implements PLUGINS.md §7.2 PL-X. Reads only the paths the operator names.
//! Depends on the `plugin-index` library. It validates and checks test-key fixtures.
//! There is no publish, fetch, or sign subcommand.

#![forbid(unsafe_code)]

use plugin_index::{IndexError, execute};
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match execute(&args, read_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("plugin-index: {error}");
            ExitCode::from(1u8)
        }
    }
}

fn read_path(path: &str) -> Result<Vec<u8>, IndexError> {
    fs::read(path).map_err(|_| IndexError::Io {
        path: path.to_owned(),
    })
}
