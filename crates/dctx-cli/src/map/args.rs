//! Parsing for the five read-only map verbs and their bounded options.
//! Implements SPEC-MAP-MODE sections 4 and 9.1 with no admin commands.
//! Uses map-query request validation and keeps snapshot-mode parsing separate.
use dctx_map_query::{AnswerBudget, Direction, EdgeKind, Format, Request, Verb};
use std::path::PathBuf;

pub(super) struct Options {
    pub request: Request,
    pub format: Format,
    pub place: PathBuf,
}
pub(super) const HELP: &str = "Usage: dctx map <map|find|neighbors|explain|read> [TARGET] [OPTIONS]
  --json                  Machine-readable bounded output
  --place PATH            Place or worktree root (default: current directory)
  --budget TOKENS         Output tokens, 1..2000 (explain max: 1000)
  --depth N               Map: 1..3; neighbors: 1..2
  --limit N --offset N    Bounded result pages
  --kind fn,type,doc      Find kinds
  --in PATH              Find within a relative path
  --edge calls,imports   Neighbor edge filter
  --dir in|out|both       Neighbor direction
  --lines A-B --full      Read exact source lines; full permits 400
  --rev SHA              Require the retained clean revision
Examples:
  dctx map find 'ignore rules' --json
  dctx map map crates/dctx-core --depth 2
";

pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    let verb = match args.first().map(String::as_str) {
        Some("map") => Verb::Map,
        Some("find") => Verb::Find,
        Some("neighbors") => Verb::Neighbors,
        Some("explain") => Verb::Explain,
        Some("read") => Verb::Read,
        _ => {
            return Err(
                "expected map, find, neighbors, explain or read; try: dctx map --help".into(),
            )
        }
    };
    let mut request = Request::new(verb, "");
    let mut place = PathBuf::from(".");
    let mut format = Format::Text;
    let mut target = None;
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        if flag == "--json" {
            format = Format::Json;
            continue;
        }
        if flag == "--full" {
            request.full = true;
            continue;
        }
        if !flag.starts_with('-') {
            if target.replace(flag.clone()).is_some() {
                return Err("quote a multi-word target as one argument".into());
            }
            continue;
        }
        let value = args
            .get(index)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        index += 1;
        match flag.as_str() {
            "--place" => place = PathBuf::from(value),
            "--budget" => {
                request.budget = AnswerBudget::new(number(value)?).map_err(|e| e.to_string())?
            }
            "--depth" if matches!(verb, Verb::Map | Verb::Neighbors) => {
                request.depth = number(value)?
            }
            "--limit" if matches!(verb, Verb::Map | Verb::Find | Verb::Neighbors) => {
                request.limit = number(value)?
            }
            "--offset" if matches!(verb, Verb::Map | Verb::Find | Verb::Neighbors) => {
                request.offset = number(value)?
            }
            "--kind" if verb == Verb::Find => {
                request.kinds = value.split(',').map(str::to_owned).collect();
                if request.kinds.iter().any(|k| {
                    ![
                        "fn",
                        "method",
                        "struct",
                        "enum",
                        "trait",
                        "impl",
                        "const",
                        "type",
                        "macro",
                        "class",
                        "interface",
                        "symbol",
                        "module",
                        "package",
                        "place",
                        "doc",
                        "test",
                    ]
                    .contains(&k.as_str())
                }) {
                    return Err("unknown node kind".into());
                }
            }
            "--in" if verb == Verb::Find => request.within = Some(value.clone()),
            "--edge" if verb == Verb::Neighbors => {
                request.edges = value.split(',').map(edge).collect::<Result<_, _>>()?
            }
            "--dir" if verb == Verb::Neighbors => {
                request.direction = match value.as_str() {
                    "in" => Direction::In,
                    "out" => Direction::Out,
                    "both" => Direction::Both,
                    _ => return Err("direction must be in, out or both".into()),
                }
            }
            "--lines" if verb == Verb::Read => {
                let (a, b) = value.split_once('-').ok_or("--lines requires A-B")?;
                request.lines = Some([number(a)?, number(b)?]);
            }
            "--rev" => request.revision = Some(value.clone()),
            _ => {
                return Err(format!(
                    "unknown or inapplicable option {flag}; try: dctx map --help"
                ))
            }
        }
    }
    request.target = target.unwrap_or_default();
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options {
        request,
        format,
        place,
    })
}
fn number<T: std::str::FromStr>(value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid number: {value}"))
}
fn edge(value: &str) -> Result<EdgeKind, String> {
    match value {
        "contains" => Ok(EdgeKind::Contains),
        "imports" => Ok(EdgeKind::Imports),
        "calls" => Ok(EdgeKind::Calls),
        "implements" => Ok(EdgeKind::Implements),
        "uses-type" => Ok(EdgeKind::UsesType),
        "tests" => Ok(EdgeKind::Tests),
        "documents" => Ok(EdgeKind::Documents),
        "reexports" => Ok(EdgeKind::Reexports),
        _ => Err(format!("unknown edge {value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(args: &[&str]) -> Result<Options, String> {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn five_verbs_and_bounds() {
        for verb in ["find", "neighbors", "explain", "read"] {
            assert!(options(&[verb, "target", "--json"]).is_ok());
        }
        assert!(options(&["map"]).is_ok());
        for args in [
            vec!["index"],
            vec!["stats"],
            vec!["find"],
            vec!["find", "x", "--budget", "2001"],
            vec!["neighbors", "x", "--depth", "3"],
            vec!["read", "x", "--lines", "4-2"],
            vec!["map", "--dir", "out"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
