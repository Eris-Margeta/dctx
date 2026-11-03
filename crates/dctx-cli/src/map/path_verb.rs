// SPDX-License-Identifier: Apache-2.0
//! `dctx map path <FROM> <TO>`: the shortest edge path between two nodes (calls, imports, contains), at most 8 hops.
//! Parses its own few flags, so the five verbs and their bounds in `args` stay as they are.
//! Both ends are stable ids or place-relative files; an id that names no node is an error. The page fits the 2,000-token cap.

use dctx::{MapRequest, Session, SnapshotHandle, VecSink, Verb};
use dctx_map_query::{AnswerBudget, Format, Request, Verb as QueryVerb};
use std::path::PathBuf;

use super::args::Options;

/// Parses `path FROM TO [--json] [--place PATH] [--budget TOKENS] [--meta]`.
pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let mut request = Request::new(QueryVerb::Map, "");
    request.budget = AnswerBudget::new(2000).map_err(|e| e.to_string())?;
    let (mut place, mut format, mut meta, mut budget_set) = (PathBuf::from("."), Format::Text, false, false);
    let mut ends: Vec<String> = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        match flag.as_str() {
            "--json" => format = Format::Json,
            "--meta" => meta = true,
            "--place" | "--budget" => {
                let value = args.get(index).ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                if flag == "--place" {
                    place = PathBuf::from(value);
                } else {
                    let tokens = value.parse().map_err(|_| format!("invalid number: {value}"))?;
                    request.budget = AnswerBudget::new(tokens).map_err(|e| e.to_string())?;
                    budget_set = true;
                }
            }
            _ if flag.starts_with('-') => return Err(format!("path takes two ids and only --json, --place, --budget and --meta; got {flag}")),
            _ => ends.push(flag.clone()),
        }
    }
    let [from, to] = <[String; 2]>::try_from(ends).map_err(|_| "path requires two targets: path <FROM> <TO>".to_string())?;
    request.target = from;
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options { request, format, place, continuation: None, budget_set, profile: false, meta, hubs: false, grep: false, ignore_case: false, skeleton: false, to: Some(to), export: None })
}

/// Runs the path verb on `handle` and returns the page.
pub(super) fn answer(session: &mut Session, handle: &SnapshotHandle, options: &Options) -> Result<String, String> {
    let to = options.to.as_deref().ok_or("path requires two targets")?;
    let mut request = MapRequest::new(Verb::Neighbors);
    request.limits.estimated_tokens = Some(u64::from(options.request.budget.tokens()));
    request.limits.encoded_bytes = Some(u64::try_from(options.request.budget.bytes()).unwrap_or(u64::MAX));
    request.output_format = if matches!(options.format, Format::Json) { 2 } else { 1 };
    request.meta = options.meta;
    let mut sink = VecSink::new();
    session.shortest_path(handle, &options.request.target, to, &request, &mut sink).map_err(|e| e.to_string())?;
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
    fn path_takes_exactly_two_targets_and_a_few_flags() {
        let plain = parse(&strings(&["path", "s:a.py#a1", "s:a.py#a3", "--json", "--meta"])).expect("plain");
        assert_eq!((plain.request.target.as_str(), plain.to.as_deref(), plain.meta), ("s:a.py#a1", Some("s:a.py#a3"), true));
        assert_eq!(plain.request.budget.tokens(), 2000);
        for bad in [vec!["path"], vec!["path", "s:a.py#a1"], vec!["path", "a", "b", "c"], vec!["path", "a", "b", "--depth", "2"], vec!["path", "a", "b", "--budget", "2001"], vec!["path", "a", "b", "--place"]] {
            assert!(parse(&strings(&bad)).is_err(), "{bad:?}");
        }
        assert!(super::super::args::HELP.contains("path <FROM> <TO>"));
    }

    #[test]
    fn path_lists_the_hops_says_no_path_and_refuses_an_unknown_id() {
        let fixture = tempfile::tempdir().expect("fixture");
        let (place, cache) = (fixture.path().join("place"), fixture.path().join("cache"));
        std::fs::create_dir_all(&place).expect("place");
        std::fs::write(place.join("a.py"), "def a1():\n    return a2()\n\ndef a2():\n    return a3()\n\ndef a3():\n    return 0\n\ndef lone():\n    return 0\n").expect("source");
        let run = |ends: &[&str], extra: &[&str]| {
            let mut args = strings(&["path"]);
            args.extend(strings(ends));
            args.extend(strings(&["--place", &place.display().to_string()]));
            args.extend(strings(extra));
            super::super::execute_with(&args, |at| host::open_mapped_at(at, Some(&cache)))
        };
        let text = run(&["s:a.py#a1", "s:a.py#a3"], &[]).expect("path");
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].starts_with("# ") && lines[0].ends_with(" path"), "{text}");
        assert_eq!(lines[1..], ["start\ts:a.py#a1\tfn\ta.py", "calls:out\ts:a.py#a2\tfn\ta.py", "calls:out\ts:a.py#a3\tfn\ta.py"], "{text}");
        assert!(run(&["s:a.py#a1", "s:a.py#lone"], &[]).expect("none").ends_with(" path\nno path\n"));
        assert!(run(&["a.py", "s:a.py#a3"], &["--json"]).expect("file").contains("\"edge\":\"contains\""));
        let unknown = run(&["s:a.py#a1", "s:a.py#nope"], &[]).expect_err("unknown");
        assert!(unknown.contains("unknown id") && unknown.contains("s:a.py#nope"), "{unknown}");
    }
}
