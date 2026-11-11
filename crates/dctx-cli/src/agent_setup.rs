// SPDX-License-Identifier: Apache-2.0
//! Static, pasteable agent rules; no repository, cache or environment reads.

use dctx_cli::exit::Failure;

const HELP: &str = "Usage: dctx agent-setup [--format claude|agents|plain]\nPrint agent rules to stdout (default: agents); paste them into your instruction file.\nExample: dctx agent-setup --format claude\n";

const RULES: &str = "\
- Use dctx for large repos, multi-hop traces, impact review, many agents sharing one index, or files over ~100 KB.
- Under ~200 files or for a single literal lookup, use grep.
- Choose dctx or grep for each question; never use both for the same question.
- Find symbols: `dctx map find run` (reuse the returned stable IDs below).
- Trace callers for impact review: `dctx map neighbors 's:src/lib.rs#run' --dir in --edge calls --depth 2`.
- Read a definition: `dctx map read 's:src/lib.rs#run'`.
- Outline a large file: `dctx map skeleton src/lib.rs`.
- Get task context: `dctx pack --for 'fix run' --budget 3000` (estimated tokens).
- MCP: use `dctx mcp --profile lean` for persistent tool access; CLI: run the commands above from a shell.
- Pick MCP or CLI for a question; both use the same local index and query engine.
";

pub(super) fn run(args: &[String]) -> Result<(), Failure> {
    let format = match args {
        [] => "agents",
        [help] if matches!(help.as_str(), "--help" | "-h") => {
            print!("{HELP}");
            return Ok(());
        }
        [flag, value] if flag == "--format" => value.as_str(),
        _ => {
            return Err(Failure::usage(
                "agent-setup: expected [--format claude|agents|plain]",
                "run `dctx agent-setup --help` for usage",
            ));
        }
    };
    let heading = match format {
        "claude" => "# dctx rules for CLAUDE.md",
        "agents" => "# dctx rules for AGENTS.md",
        "plain" => "dctx rules",
        _ => {
            return Err(Failure::usage(
                format!("agent-setup: unknown format `{format}`"),
                "use --format claude, agents or plain",
            ));
        }
    };
    let rules = if format == "plain" {
        RULES.replace("- ", "").replace('`', "")
    } else {
        RULES.to_owned()
    };
    print!("{heading}\n\n{rules}");
    Ok(())
}
