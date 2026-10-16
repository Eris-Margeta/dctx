//! Standalone dctx/git worker entry: speaks the plugin protocol on inherited stdin/stdout only.
//! Implements PLUGINS.md §2/§6.4 "src/main.rs — standalone worker entry only"; the launch identity comes
//! from the host's verified install record as arguments. Diagnostics go to stderr, never to stdout.
//! Depends on the dctx_plugin_git library; started by a dctx plugin host (never manually granted).

#![forbid(unsafe_code)]

use std::io::{stdin, stdout};
use std::process::ExitCode;

use dctx_plugin_git::adapters::worker::{LaunchIdentity, serve};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let launch = match LaunchIdentity::from_args(&args) {
        Ok(launch) => launch,
        Err(usage) => {
            eprintln!("dctx-plugin-git: {usage}");
            return ExitCode::from(2);
        }
    };
    match serve(stdin().lock(), stdout().lock(), &launch) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dctx-plugin-git: {error}");
            ExitCode::from(1)
        }
    }
}
