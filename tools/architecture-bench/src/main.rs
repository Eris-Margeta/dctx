// SPDX-License-Identifier: Apache-2.0
//! DX-A1 baseline process for the current dctx map mode.
//! Implements ARCHITECTURE-PROPOSAL §3 and §7 Stage 0.
//! Depends on the architecture_bench library; tools/architecture-bench/run.sh starts it.
#![forbid(unsafe_code)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = architecture_bench::parse_args(&args).and_then(|options| {
        architecture_bench::run(&options)
    }) {
        eprintln!("architecture-bench: {error}");
        std::process::exit(1);
    }
}
