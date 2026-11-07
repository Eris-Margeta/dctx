// SPDX-License-Identifier: Apache-2.0
//! `dctx pack [PATH] --for "<text>" [--budget N]`: task context for an agent card (v1 card 9).
//! Takes the names the text mentions (see `terms`), runs `map find` on each, then `map neighbors` and `map read` on the
//! best hit of the first terms, and keeps whole lines in that priority order while the page stays inside the budget.
//! The budget is in estimated tokens, ceil(2 * UTF-8 bytes / 7), the `dctx map --budget` estimate (see `page`).
//! Depends on the map verbs of this binary, so it inherits their ignore and sensitive-file rules and their cache.
//! Used by tools/cardgen/context.sh. Prints to stdout only; writes nothing inside the place.
//! Exit codes (docs/CLI-EXIT-CODES.md): 0 ok, 1 no term found anything (the page is printed first), 2 usage, 3 index error.

mod page;
mod terms;

use std::path::PathBuf;

use crate::exit::{self, Failure};

/// Budget when `--budget` is not given, in estimated tokens.
const DEFAULT_BUDGET: u32 = 3000;
/// Smallest budget: the header and the end line take about 40 tokens.
const MIN_BUDGET: u32 = 100;
/// Largest budget.
const MAX_BUDGET: u32 = 100_000;
/// Find records asked per term.
const FIND_LIMIT: &str = "3";
/// Neighbor records asked per best hit.
const NEIGHBOR_LIMIT: &str = "12";
/// Best hits whose neighbors are listed.
const NEIGHBOR_HITS: usize = 4;
/// Best hits whose source is read.
const READ_HITS: usize = 3;
/// The usage hint of every argument refusal.
const HELP_HINT: &str = "run `dctx pack --help` for the flags and an example";

/// True when the words after `pack` ask for task context (`--for` or `--budget`) and not for help.
pub(crate) fn requested(args: &[String]) -> bool {
    !args.iter().any(|arg| arg == "--help" || arg == "-h")
        && args.iter().any(|arg| arg == "--for" || arg == "--budget")
}

/// Runs `pack --for`. `args` starts with `pack`.
/// # Errors
/// Usage refusals (exit 2), a place or index failure (exit 3), and a page on which no term found anything (exit 1).
pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    let plan = parse(args).map_err(|message| Failure::usage(message, HELP_HINT).within("pack"))?;
    if !plan.place.is_dir() {
        return Err(Failure::index(
            format!(
                "cannot open the place {}: not a directory",
                plan.place.display()
            ),
            "pass an existing project directory as PATH, or run dctx inside one",
        )
        .within("pack"));
    }
    let built = build(&plan, crate::map::answer).map_err(|failure| failure.within("pack"))?;
    print!("{}", built.text);
    if built.hits == 0 {
        return Err(Failure::no_result(
            "pack --for: no name in the text matched the index",
            "name a function, type or file in the text, or check `dctx map find <name>`",
        ));
    }
    Ok(())
}

/// Parsed `pack --for` arguments.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    place: PathBuf,
    text: String,
    budget: u32,
}

fn parse(args: &[String]) -> Result<Plan, String> {
    let mut place: Option<PathBuf> = None;
    let mut text: Option<String> = None;
    let mut budget: Option<u32> = None;
    let mut index = 1;
    while index < args.len() {
        let arg = args[index].as_str();
        match arg {
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
                    "pack --for takes PATH, --for and --budget only, not {other}"
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
    })
}

/// The finished page and the number of find records behind it.
struct Built {
    text: String,
    hits: usize,
}

/// Builds the page with `query`, which answers one `dctx map` argument list (the words after `map`).
fn build(
    plan: &Plan,
    mut query: impl FnMut(&[String]) -> Result<String, Failure>,
) -> Result<Built, Failure> {
    let terms = terms::extract(&plan.text);
    let place = plan.place.display().to_string();
    let mut page = page::Page::new(
        plan.budget,
        &format!(
            "# dctx pack --for budget={} terms={}",
            plan.budget,
            terms.len()
        ),
    );
    let mut best: Vec<String> = Vec::new();
    let mut hits = 0;
    for term in &terms {
        let Some(found) = ask(
            &mut query,
            &[
                "find",
                term.as_str(),
                "--limit",
                FIND_LIMIT,
                "--place",
                place.as_str(),
            ],
        )?
        else {
            continue;
        };
        let records: Vec<&str> = body(&found)
            .into_iter()
            .filter(|line| line.contains('\t'))
            .collect();
        hits += records.len();
        if let Some(id) = best_hit(&records, term) {
            if !best.contains(&id) {
                best.push(id);
            }
        }
        page.section(&format!("## find {term}"), &records);
    }
    for id in best.iter().take(NEIGHBOR_HITS) {
        if let Some(answer) = ask(
            &mut query,
            &[
                "neighbors",
                id.as_str(),
                "--limit",
                NEIGHBOR_LIMIT,
                "--place",
                place.as_str(),
            ],
        )? {
            page.section(&format!("## neighbors {id}"), &body(&answer));
        }
    }
    for id in best.iter().take(READ_HITS) {
        if let Some(answer) = ask(
            &mut query,
            &["read", id.as_str(), "--place", place.as_str()],
        )? {
            page.section(&format!("## read {id}"), &body(&answer));
        }
    }
    Ok(Built {
        text: page.finish(),
        hits,
    })
}

/// One map query. A usage or no-result refusal skips that term or id; an index failure stops the pack.
fn ask(
    query: &mut impl FnMut(&[String]) -> Result<String, Failure>,
    words: &[&str],
) -> Result<Option<String>, Failure> {
    let words: Vec<String> = words.iter().map(ToString::to_string).collect();
    match query(words.as_slice()) {
        Ok(answer) => Ok(Some(answer)),
        Err(failure) if failure.code == exit::USAGE || failure.code == exit::NO_RESULT => Ok(None),
        Err(failure) => Err(failure),
    }
}

/// The lines of a map text page after its `# <place> <verb>` header.
fn body(answer: &str) -> Vec<&str> {
    let mut lines = answer.lines();
    let first = answer.lines().next();
    if first.is_some_and(|line| line.starts_with("# ")) {
        lines.next();
    }
    lines.collect()
}

/// A file term selects its module. Otherwise prefer the named symbol, then the
/// first symbol id, then the first id.
fn best_hit(records: &[&str], term: &str) -> Option<String> {
    let ids: Vec<&str> = records
        .iter()
        .filter_map(|line| line.split('\t').next())
        .filter(|id| !id.is_empty())
        .collect();
    if term.contains('.') {
        let suffix = format!("/{term}");
        if let Some(path) = ids
            .iter()
            .filter_map(|id| {
                id.strip_prefix("s:")
                    .or_else(|| id.strip_prefix("m:"))
                    .map(|rest| rest.split('#').next().unwrap_or(rest))
            })
            .find(|path| *path == term || path.ends_with(&suffix))
        {
            return Some(format!("m:{path}"));
        }
    }
    let named = |id: &str| {
        let name = id.rsplit('#').next().unwrap_or(id);
        name == term || name.ends_with(&format!("::{term}")) || name.ends_with(&format!(".{term}"))
    };
    let chosen = ids
        .iter()
        .copied()
        .find(|&id| id.contains('#') && named(id))
        .or_else(|| ids.iter().copied().find(|&id| id.starts_with("s:")))
        .or_else(|| ids.first().copied());
    chosen.map(str::to_string)
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
                budget: 500
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
            (vec!["pack", "--for", "x", "--json"], "not --json"),
            (vec!["pack", "a", "b", "--for", "x"], "one source path"),
        ] {
            let error = parse(&words(&args)).expect_err("refused");
            assert!(error.contains(wanted), "{args:?}: {error}");
        }
    }

    /// A fake `map` that answers find, neighbors and read from fixed pages and records every call.
    fn fake(calls: &mut Vec<String>) -> impl FnMut(&[String]) -> Result<String, Failure> + '_ {
        move |args: &[String]| {
            calls.push(args.join(" "));
            match (args[0].as_str(), args[1].as_str()) {
                ("find", "alpha_body") => Ok(
                    "# p find\ns:a.rs#beta\tfn\tL5-7\ns:a.rs#alpha_body\tfn\tL1-3\n".to_string(),
                ),
                ("find", "BetaStore") => {
                    Ok("# p find\ns:b.rs#BetaStore\tstruct\tL1-4\n".to_string())
                }
                ("find", "broken_term") => Err(Failure::usage("bad query", "hint")),
                ("find", _) => Ok("# p find\n".to_string()),
                ("neighbors", id) => Ok(format!("# p neighbors\ncalls:in:{id}\tfn\ta.rs\tL9\n")),
                ("read", id) => Ok(format!(
                    "# p read\n{id}\tfn\ta.rs\n# a source comment\nfn body() {{}}\n"
                )),
                _ => Err(Failure::index("unexpected", "hint")),
            }
        }
    }

    #[test]
    fn build_finds_each_term_then_reads_the_best_hits_in_order() {
        let plan = Plan {
            place: PathBuf::from("/p"),
            text: "fix `alpha_body`, broken_term and BetaStore".into(),
            budget: 3000,
        };
        let mut calls = Vec::new();
        let built = build(&plan, fake(&mut calls)).expect("built");
        assert_eq!(built.hits, 3);
        assert_eq!(
            calls,
            [
                "find alpha_body --limit 3 --place /p",
                "find broken_term --limit 3 --place /p",
                "find BetaStore --limit 3 --place /p",
                "neighbors s:a.rs#alpha_body --limit 12 --place /p",
                "neighbors s:b.rs#BetaStore --limit 12 --place /p",
                "read s:a.rs#alpha_body --place /p",
                "read s:b.rs#BetaStore --place /p",
            ]
        );
        let text = built.text;
        assert!(text.starts_with("# dctx pack --for budget=3000 terms=3\n## find alpha_body\ns:a.rs#beta\tfn\tL5-7\n"), "{text}");
        assert!(
            text.contains(
                "## neighbors s:a.rs#alpha_body\ncalls:in:s:a.rs#alpha_body\tfn\ta.rs\tL9\n"
            ),
            "{text}"
        );
        assert!(text.contains("## read s:b.rs#BetaStore\ns:b.rs#BetaStore\tfn\ta.rs\n# a source comment\nfn body() {}\n"), "{text}");
        assert!(!text.contains("## find broken_term"), "{text}");
        assert!(text.ends_with("dropped=0\n"), "{text}");
    }

    #[test]
    fn an_index_failure_stops_the_pack_and_no_hit_is_zero_hits() {
        let failing =
            |_: &[String]| -> Result<String, Failure> { Err(Failure::index("disk", "hint")) };
        let plan = Plan {
            place: PathBuf::from("."),
            text: "alpha_body".into(),
            budget: 3000,
        };
        let Err(failure) = build(&plan, failing) else {
            panic!("an index failure must stop the pack");
        };
        assert_eq!(failure.code, exit::INDEX);
        let mut calls = Vec::new();
        let none = Plan {
            place: PathBuf::from("."),
            text: "nothing_here".into(),
            budget: 3000,
        };
        assert_eq!(build(&none, fake(&mut calls)).expect("built").hits, 0);
    }

    #[test]
    fn best_hit_prefers_the_named_symbol() {
        assert_eq!(
            best_hit(&["s:src/lib.rs#Alias\ttype"], "src/lib.rs").as_deref(),
            Some("m:src/lib.rs")
        );
        assert_eq!(
            best_hit(&["s:src/lib.rs#Alias\ttype"], "lib.rs").as_deref(),
            Some("m:src/lib.rs")
        );
        assert_eq!(
            best_hit(&["s:a.rs#x\tfn", "s:a.rs#Repo::get\tfn"], "get").as_deref(),
            Some("s:a.rs#Repo::get")
        );
        assert_eq!(
            best_hit(&["m:a.rs\tmodule", "s:a.rs#x\tfn"], "y").as_deref(),
            Some("s:a.rs#x")
        );
        assert_eq!(
            best_hit(&["m:a.rs\tmodule"], "y").as_deref(),
            Some("m:a.rs")
        );
        assert_eq!(best_hit(&[], "y"), None);
    }

    #[test]
    fn body_drops_only_the_page_header() {
        assert_eq!(
            body("# p read\nid\tfn\n# kept comment\n"),
            ["id\tfn", "# kept comment"]
        );
        assert_eq!(body("no header\n"), ["no header"]);
    }
}
