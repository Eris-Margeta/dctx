// SPDX-License-Identifier: Apache-2.0
//! `dctx map export`: the whole graph as one deterministic document, graph JSON or a Mermaid flowchart.
//! Parses its own few flags, so the five verbs and their bounds in `args` stay as they are.
//! It is an export, not an answer: no target, no 2,000-token cap; `--output PATH` writes a file instead of stdout.

use dctx::{MapRequest, Session, SnapshotHandle, Verb};
use dctx_map_query::{Format, Request, Verb as QueryVerb};
use std::path::PathBuf;

use super::args::Options;

/// What `export` was asked for.
#[derive(Debug, Clone)]
pub(super) struct Export {
    /// `json` or `mermaid`.
    pub format: String,
    /// File to write instead of stdout.
    pub output: Option<PathBuf>,
}

/// Parses `export [--format json|mermaid] [--output PATH] [--place PATH]`. The default format is `json`.
pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let request = Request::new(QueryVerb::Map, "");
    let (mut place, mut format, mut output) = (PathBuf::from("."), "json".to_string(), None);
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        match flag.as_str() {
            "--format" | "--output" | "--place" => {
                let value = args.get(index).ok_or_else(|| format!("missing value for {flag}"))?;
                index += 1;
                match flag.as_str() {
                    "--format" => format = value.clone(),
                    "--output" => output = Some(PathBuf::from(value)),
                    _ => place = PathBuf::from(value),
                }
            }
            _ => return Err(format!("export takes no target and only --format, --output and --place; got {flag}")),
        }
    }
    if !matches!(format.as_str(), "json" | "mermaid") {
        return Err(format!("unknown export format: {format}; expected json or mermaid"));
    }
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options {
        request,
        format: Format::Text,
        place,
        continuation: None,
        budget_set: false,
        profile: false,
        meta: false,
        hubs: false,
        grep: false,
        ignore_case: false,
        skeleton: false,
        to: None,
        export: Some(Export { format, output }),
    })
}

/// Runs the export on `handle`. Returns the document, or an empty string after writing it to `--output`.
pub(super) fn answer(session: &mut Session, handle: &SnapshotHandle, options: &Options) -> Result<String, String> {
    let export = options.export.as_ref().ok_or("export requires its options")?;
    let request = MapRequest::new(Verb::Neighbors);
    let document = session.export_graph(handle, &export.format, &request).map_err(|e| e.to_string())?;
    match &export.output {
        Some(path) => {
            std::fs::write(path, document).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            Ok(String::new())
        }
        None => Ok(document),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn export_takes_a_format_an_output_and_a_place_only() {
        let plain = parse(&strings(&["export"])).expect("plain");
        assert_eq!(plain.export.as_ref().map(|e| e.format.as_str()), Some("json"));
        let full = parse(&strings(&["export", "--format", "mermaid", "--output", "g.mmd"])).expect("full");
        assert_eq!(full.export.and_then(|e| e.output), Some(PathBuf::from("g.mmd")));
        for bad in [vec!["export", "--format", "nope"], vec!["export", "src"], vec!["export", "--json"], vec!["export", "--format"]] {
            assert!(parse(&strings(&bad)).is_err(), "{bad:?}");
        }
        assert!(super::super::args::HELP.contains("export"));
    }

    #[test]
    fn export_writes_the_graph_to_stdout_or_a_file_and_repeats_byte_for_byte() {
        let fixture = tempfile::tempdir().expect("fixture");
        let (place, cache) = (fixture.path().join("place"), fixture.path().join("cache"));
        std::fs::create_dir_all(&place).expect("place");
        std::fs::write(place.join("a.py"), "def alpha():\n    return beta()\n\ndef beta():\n    return 1\n").expect("source");
        let run = |extra: &[&str]| {
            let mut args = strings(&["export", "--place", &place.display().to_string()]);
            args.extend(strings(extra));
            super::super::execute_with(&args, |at| host::open_mapped_at(at, Some(&cache)))
        };
        let json = run(&["--format", "json"]).expect("json");
        assert!(json.contains("{\"src\":\"s:a.py#alpha\",\"edge\":\"calls\",\"dst\":\"s:a.py#beta\""), "{json}");
        let mermaid = run(&["--format", "mermaid"]).expect("mermaid");
        assert!(mermaid.starts_with("flowchart LR\n") && mermaid.contains("-->|calls|"), "{mermaid}");
        assert_eq!(mermaid, run(&["--format", "mermaid"]).expect("again"));
        let file = fixture.path().join("out.json");
        assert_eq!(run(&["--output", &file.display().to_string()]).expect("file"), "");
        assert_eq!(std::fs::read_to_string(&file).expect("written"), json);
        assert!(run(&["--format", "nope"]).is_err());
    }
}
