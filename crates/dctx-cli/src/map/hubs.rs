//! `dctx map hubs`: the most-called symbols and per-directory counts of the place.
//! Parses its own few flags, so the five verbs and their bounds in `args` stay as they are.
//! Takes no target (or `.`), counts incoming calls and imports edges, and fits the 2,000-token cap.

use dctx::{MapRequest, Session, SnapshotHandle, VecSink, Verb};
use dctx_map_query::{AnswerBudget, Format, Request, Verb as QueryVerb};
use std::path::PathBuf;

use super::args::Options;

/// Hub lines when `--limit` is absent.
const DEFAULT_LIMIT: usize = 20;

/// Parses `hubs [.] [--json] [--place PATH] [--limit N] [--budget TOKENS] [--meta]`.
pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let mut request = Request::new(QueryVerb::Map, "");
    request.limit = DEFAULT_LIMIT;
    request.budget = AnswerBudget::new(2000).map_err(|e| e.to_string())?;
    let (mut place, mut format, mut meta, mut budget_set) = (PathBuf::from("."), Format::Text, false, false);
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        match flag.as_str() {
            "--json" => format = Format::Json,
            "--meta" => meta = true,
            "." => {}
            "--place" | "--limit" | "--budget" => {
                let value = args.get(index).ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                match flag.as_str() {
                    "--place" => place = PathBuf::from(value),
                    "--limit" => request.limit = value.parse().map_err(|_| format!("invalid number: {value}"))?,
                    _ => {
                        let tokens = value.parse().map_err(|_| format!("invalid number: {value}"))?;
                        request.budget = AnswerBudget::new(tokens).map_err(|e| e.to_string())?;
                        budget_set = true;
                    }
                }
            }
            _ => return Err(format!("hubs takes no target and only --json, --place, --limit, --budget and --meta; got {flag}")),
        }
    }
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options { request, format, place, continuation: None, budget_set, profile: false, meta, hubs: true, grep: false, ignore_case: false })
}

/// Runs the hubs verb on `handle` and returns the page.
pub(super) fn answer(session: &mut Session, handle: &SnapshotHandle, options: &Options) -> Result<String, String> {
    let mut request = MapRequest::new(Verb::Neighbors);
    request.limits.records = Some(u64::try_from(options.request.limit).unwrap_or(u64::MAX));
    request.limits.estimated_tokens = Some(u64::from(options.request.budget.tokens()));
    request.limits.encoded_bytes = Some(u64::try_from(options.request.budget.bytes()).unwrap_or(u64::MAX));
    request.output_format = if matches!(options.format, Format::Json) { 2 } else { 1 };
    request.meta = options.meta;
    let mut sink = VecSink::new();
    session.hubs(handle, &request, &mut sink).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(sink.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn hubs_takes_no_target_and_bounds_its_flags() {
        let plain = parse(&strings(&["hubs"])).expect("plain");
        assert_eq!((plain.request.limit, plain.request.budget.tokens(), plain.hubs), (20, 2000, true));
        let dot = parse(&strings(&["hubs", ".", "--limit", "60", "--json", "--meta"])).expect("dot");
        assert!(dot.meta && matches!(dot.format, Format::Json) && dot.request.limit == 60);
        for bad in [vec!["hubs", "src"], vec!["hubs", "--limit", "61"], vec!["hubs", "--limit", "0"], vec!["hubs", "--budget", "2001"], vec!["hubs", "--depth", "2"], vec!["hubs", "--limit"]] {
            assert!(parse(&strings(&bad)).is_err(), "{bad:?}");
        }
        assert!(super::super::args::HELP.contains("hubs"));
    }

    #[test]
    fn hubs_lists_the_most_called_symbol_first_and_the_directories() {
        let fixture = tempfile::tempdir().expect("fixture");
        let (place, cache) = (fixture.path().join("place"), fixture.path().join("cache"));
        std::fs::create_dir_all(place.join("lib")).expect("lib");
        std::fs::create_dir_all(place.join("app")).expect("app");
        std::fs::write(place.join("lib/util.py"), "def helper():\n    return 1\n\ndef rare():\n    return 2\n").expect("util");
        let caller = "from lib.util import helper\n\ndef one():\n    return helper()\n\ndef two():\n    return helper()\n\ndef three():\n    return helper() + rare()\n";
        std::fs::write(place.join("app/a.py"), caller).expect("a");
        let run = |extra: &[&str]| {
            let mut args = strings(&["hubs", "--place", &place.display().to_string()]);
            args.extend(strings(extra));
            super::super::execute_with(&args, |at| host::open_mapped_at(at, Some(&cache))).expect("hubs")
        };
        let text = run(&[]);
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("# ") && lines[0].ends_with(" hubs"), "{text}");
        assert_eq!(lines[1], "s:lib/util.py#helper\tfn\tlib/util.py\t3", "{text}");
        assert_eq!(lines[2], "s:lib/util.py#rare\tfn\tlib/util.py\t1", "{text}");
        assert!(lines.contains(&"app/\t1 files\t3 symbols") && lines.contains(&"lib/\t1 files\t2 symbols"), "{text}");
        let json = run(&["--json"]);
        assert!(json.contains("\"in_degree\":3") && json.contains("\"dirs\":["), "{json}");
    }
}
