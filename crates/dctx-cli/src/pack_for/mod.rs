// SPDX-License-Identifier: Apache-2.0
//! CLI arguments for engine-owned task-context selection and rendering.
//! Both text and JSON output call `Session::context` against one pinned snapshot.

use std::path::PathBuf;

use crate::exit::{self, Failure};

/// Budget when `--budget` is not given, in estimated tokens.
const DEFAULT_BUDGET: u32 = 3000;
/// Smallest budget: the header and the end line take about 40 tokens.
const MIN_BUDGET: u32 = 100;
/// Largest budget.
const MAX_BUDGET: u32 = 100_000;
/// The usage hint of every argument refusal.
const HELP_HINT: &str = "run `dctx pack --help` for the flags and an example";
/// Printed on stderr, never on the page, when the text names nothing code-like and reads as prose.
const PROSE_WARNING: &str = "warning: no identifiers in --for text; plain words match loosely. Pass function, type or file names, or put names in backticks (see `dctx pack --help`)";

/// True when the words after `pack` ask for task context (`--for` or `--budget`) and not for help.
pub(crate) fn requested(args: &[String]) -> bool {
    !args.iter().any(|arg| arg == "--help" || arg == "-h")
        && args.iter().any(|arg| arg == "--for" || arg == "--budget")
}

/// Runs `pack --for`. `args` starts with `pack`.
/// # Errors
/// Usage refusals (exit 2), a place or index failure (exit 3), and a page on which no term found anything (exit 1).
pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    let (output, prose_only) = transaction(args)?;
    if prose_only {
        eprintln!("{PROSE_WARNING}");
    }
    output.emit()
}

/// Shared CLI/RPC context transaction. RPC gets no warning text: stdout is its channel.
pub(crate) fn output(args: &[String]) -> Result<crate::exit::CommandOutput, Failure> {
    transaction(args).map(|(output, _)| output)
}

/// The page, its status, and whether the text read as prose without identifiers.
fn transaction(args: &[String]) -> Result<(crate::exit::CommandOutput, bool), Failure> {
    let plan = parse(args).map_err(|message| Failure::usage(message, HELP_HINT).within("pack"))?;
    let (mut session, key) = crate::host::open_mapped(&plan.place).map_err(|message| {
        Failure::index(message, "pass an existing project directory as PATH").within("pack")
    })?;
    let handle = session
        .snapshot_reusable(&dctx::SnapshotRequest::strict(), &key)
        .map_err(|error| exit::from_facade(&error, "dctx pack --help"))?;
    let request = dctx::context::ContextRequest {
        text: plan.text,
        budget: plan.budget,
        output_format: if plan.json { 2 } else { 1 },
    };
    let built = session
        .context(&handle, &request)
        .map_err(|error| exit::from_facade(&error, "dctx pack --help"))?;
    let failure = if built.terms == 0 {
        Some(Failure::no_result(
            "pack --for: the text has no search terms (common words and numbers are dropped, and so are words under 5 letters when the text has backticks or code-like names)",
            "name a function, type or file, or put a name in backticks, e.g. --for '`main`'",
        ))
    } else if built.hits == 0 {
        Some(Failure::no_result(
            "pack --for: no name in the text matched the index",
            "name a function, type or file in the text, or check `dctx map find <name>`",
        ))
    } else {
        None
    };
    let prose_only = built.prose_only;
    Ok((
        crate::exit::CommandOutput {
            stdout: built.text,
            failure,
        },
        prose_only,
    ))
}

/// Parsed `pack --for` arguments.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    place: PathBuf,
    text: String,
    budget: u32,
    json: bool,
}

fn parse(args: &[String]) -> Result<Plan, String> {
    let mut place: Option<PathBuf> = None;
    let mut text: Option<String> = None;
    let mut budget: Option<u32> = None;
    let mut json = false;
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
            "--json" => json = true,
            "--for" | "--budget" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                if arg == "--for" {
                    // The text may start with `-` (a markdown list), so any value is taken.
                    if text.replace(value.clone()).is_some() {
                        return Err("pack accepts one --for text".to_string());
                    }
                } else {
                    let tokens: u32 = value
                        .parse()
                        .map_err(|_| format!("invalid --budget: {value}"))?;
                    if !(MIN_BUDGET..=MAX_BUDGET).contains(&tokens) {
                        return Err(format!("--budget must be {MIN_BUDGET}..={MAX_BUDGET} estimated tokens, got {tokens}"));
                    }
                    if budget.replace(tokens).is_some() {
                        return Err("--budget was given twice".to_string());
                    }
                }
            }
            other if other.starts_with('-') => {
                return Err(format!(
                    "pack --for takes PATH, --for, --budget and --json only, not {other}"
                ))
            }
            other => {
                if place.replace(PathBuf::from(other)).is_some() {
                    return Err("pack accepts one source path".to_string());
                }
            }
        }
        index += 1;
    }
    let text = text.ok_or_else(|| "--budget applies to pack --for \"<text>\"".to_string())?;
    if text.trim().is_empty() {
        return Err("--for requires a non-empty task text".to_string());
    }
    Ok(Plan {
        place: place.unwrap_or_else(|| PathBuf::from(".")),
        text,
        budget: budget.unwrap_or(DEFAULT_BUDGET),
        json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn requested_needs_for_or_budget_and_no_help() {
        assert!(requested(&words(&["--for", "x"])));
        assert!(requested(&words(&[".", "--budget", "200"])));
        assert!(!requested(&words(&["--for", "x", "--help"])));
        assert!(!requested(&words(&[".", "--ext", "rs"])));
    }

    #[test]
    fn parse_takes_place_text_and_budget_and_refuses_the_rest() {
        let plan = parse(&words(&[
            "pack",
            "src",
            "--for",
            "- fix alpha_body",
            "--budget",
            "500",
        ]))
        .expect("plan");
        assert_eq!(
            plan,
            Plan {
                place: PathBuf::from("src"),
                text: "- fix alpha_body".into(),
                budget: 500,
                json: false,
            }
        );
        assert_eq!(
            parse(&words(&["pack", "--for", "x"]))
                .expect("default")
                .budget,
            DEFAULT_BUDGET
        );
        for (args, wanted) in [
            (vec!["pack", "--budget", "300"], "applies to pack --for"),
            (vec!["pack", "--for"], "--for requires a value"),
            (vec!["pack", "--for", "x", "--budget", "99"], "100..=100000"),
            (
                vec!["pack", "--for", "x", "--budget", "100001"],
                "100..=100000",
            ),
            (
                vec!["pack", "--for", "x", "--budget", "lots"],
                "invalid --budget",
            ),
            (vec!["pack", "--for", "x", "--for", "y"], "one --for"),
            (vec!["pack", "--for", " "], "non-empty"),
            (vec!["pack", "a", "b", "--for", "x"], "one source path"),
            (
                vec!["pack", "--for", "x", "--ext", "rs"],
                "--for, --budget and --json only, not --ext",
            ),
        ] {
            let error = parse(&words(&args)).expect_err("refused");
            assert!(error.contains(wanted), "{args:?}: {error}");
        }
    }
}
