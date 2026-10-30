//! Parsing for the five read-only map verbs and their bounded options.
//! Implements SPEC-MAP-MODE sections 4 and 9.1 with no admin commands.
//! Uses map-query request validation. Execution is the facade in the parent module.
//! A read with no `--budget` leaves the facade read default, which fits a typical module.
use dctx_map_query::{AnswerBudget, Direction, EdgeKind, Format, Request, Verb};
use std::path::PathBuf;

pub(super) struct Options {
    pub request: Request,
    pub format: Format,
    pub place: PathBuf,
    /// Opaque `next` token from an earlier page. Forwarded as `MapRequest.continuation`.
    pub continuation: Option<String>,
    /// True when the caller passed `--budget`. Read otherwise uses the facade default.
    pub budget_set: bool,
    /// True when the caller passed `--profile`. `DCTX_PROFILE=1` is applied by the caller.
    pub profile: bool,
    /// True when the caller passed `--meta`: the snapshot digest, generation and age come back in the answer.
    pub meta: bool,
    /// True for `dctx map hubs`, which has no target and its own flags (see `hubs`).
    pub hubs: bool,
    /// True for `grep`. The request then carries `Verb::Find` and `request.target` is the literal.
    pub grep: bool,
    /// True when the caller passed `-i` or `--ignore-case` to grep.
    pub ignore_case: bool,
    /// True for `skeleton`: parsed as a map request, sent to the facade as the skeleton verb.
    pub skeleton: bool,
}
pub(super) const HELP: &str = "Usage: dctx map <map|find|neighbors|explain|read|skeleton|grep> <TARGET> [OPTIONS]
       dctx map hubs [--limit N] [--json] [--place PATH] [--budget TOKENS] [--meta]
         Most-called symbols (incoming calls and imports, 1..60 lines, default 20) and per-directory file and symbol counts. No TARGET.
  TARGET                  Required. map accepts a stable id or a place-relative file.
                          skeleton takes a file (or m:<path>) and prints its declaration signatures, no bodies,
                          in source order: id, kind, L<start>-<end>, signature.
                          A file path is read as m:<path>. Omitting TARGET does not scan.
  --json                  Facade JSON v2, including the precision label
  --place PATH            Place or worktree root (default: current directory)
  --meta                  Restore the snapshot digest and generation in the text header and the snapshot, generation and age_s JSON fields.
                          Without it the answer bytes do not change across unrelated edits or time.
  --profile               Stderr stage timings and the five slowest files. DCTX_PROFILE=1 is the same switch.
  DCTX_MAX_SOURCE_BYTES   Env: total source bytes one scan may read, 1..17179869184 (default 1073741824 = 1 GiB).
  --budget TOKENS         Output tokens, 1..2000 (explain max: 1000).
                          Read with no --budget uses the 2,000-token hard cap.
  --depth N               Map: 1..3; neighbors: 1..4. Explain and read take no depth.
  --limit N               Map, find and neighbors page size. Explain and read accept only 1.
  --offset N              Refused. Pass the previous page's next value to --cursor.
  --cursor TOKEN          Opaque continuation. A stale token is rejected.
  --kind fn,type,doc      Find kinds
  --in PATH               Find or grep within a relative path
  grep LITERAL            Exhaustive literal search of indexed files, hits grouped by enclosing symbol
  -i, --ignore-case       Grep: ASCII case-insensitive match
  --edge calls,imports    Neighbor edge filter
  --dir in|out|both       Neighbor direction
  --lines A-B --full      Read those absolute source lines inside the node; full permits 400
  --rev SHA               Refused: this build does not link git
Precision: structural declarations, imports, headings and conservative calls.
Not exact semantic calls, type resolution or macro expansion.
Token estimate ceil(2 * UTF-8 bytes / 7) is not a tokenizer guarantee.
Examples:
  dctx map find 'ignore rules' --json
  dctx map map crates/dctx-cli/src/lib.rs --depth 2
  dctx map map m:crates/dctx-cli/src/lib.rs --cursor '<next from the previous page>'
";

#[allow(clippy::too_many_lines, reason = "Keep this ordered command transaction or flag dispatch with its shared validation state and existing error ordering.")]
pub(super) fn parse(args: &[String]) -> Result<Options, String> {
    if args.first().map(String::as_str) == Some("hubs") {
        return super::hubs::parse(args);
    }
    let skeleton = args.first().map(String::as_str) == Some("skeleton");
    let verb = match args.first().map(String::as_str) {
        Some("map" | "skeleton") => Verb::Map,
        Some("find") => Verb::Find,
        Some("neighbors") => Verb::Neighbors,
        Some("explain") => Verb::Explain,
        Some("read") => Verb::Read,
        // Grep shares find's bounds (one literal, --in, --limit); the facade request is rewritten in the parent module.
        Some("grep") => Verb::Find,
        _ => {
            return Err(
                "expected map, find, neighbors, explain, read, skeleton, grep or hubs; try: dctx map --help".into(),
            )
        }
    };
    let grep = args.first().map(String::as_str) == Some("grep");
    let mut ignore_case = false;
    let mut request = Request::new(verb, "");
    let mut place = PathBuf::from(".");
    let mut format = Format::Text;
    let mut target = None;
    let mut continuation = None;
    let mut budget_set = false;
    let mut profile = false;
    let mut meta = false;
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
        if flag == "--profile" {
            profile = true;
            continue;
        }
        if flag == "--meta" {
            meta = true;
            continue;
        }
        if grep && (flag == "-i" || flag == "--ignore-case") {
            ignore_case = true;
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
                request.budget = AnswerBudget::new(number(value)?).map_err(|e| e.to_string())?;
                budget_set = true;
            }
            "--cursor" => {
                if value.is_empty() {
                    return Err("continuation cursor is empty".into());
                }
                if continuation.replace(value.clone()).is_some() {
                    return Err("--cursor was given twice".into());
                }
            }
            "--depth" if !skeleton && matches!(verb, Verb::Map | Verb::Neighbors) => {
                request.depth = number(value)?;
            }
            "--limit" if matches!(verb, Verb::Map | Verb::Find | Verb::Neighbors | Verb::Explain | Verb::Read) => {
                let limit = number(value)?;
                if matches!(verb, Verb::Explain | Verb::Read) && limit != 1 {
                    return Err("limit exceeds the verb hard cap: records".into());
                }
                request.limit = limit;
            }
            "--offset" if matches!(verb, Verb::Map | Verb::Find | Verb::Neighbors) => {
                request.offset = number(value)?;
            }
            "--kind" if verb == Verb::Find && !grep => {
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
                request.edges = value.split(',').map(edge).collect::<Result<_, _>>()?;
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
    if request.verb == Verb::Map && request.target.is_empty() {
        let name = if skeleton { "skeleton" } else { "map" };
        return Err(format!(
            "{name} requires a target: a stable id (m:, s:, …) or a place-relative file path"
        ));
    }
    request.validate().map_err(|e| e.to_string())?;
    Ok(Options {
        request,
        format,
        place,
        continuation,
        budget_set,
        profile,
        meta,
        hubs: false,
        grep,
        ignore_case,
        skeleton,
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
        parse(&args.iter().map(std::string::ToString::to_string).collect::<Vec<_>>())
    }
    #[test]
    fn five_verbs_and_bounds() {
        for verb in ["find", "neighbors", "explain", "read"] {
            assert!(options(&[verb, "target", "--json"]).is_ok());
        }
        let missing = options(&["map"]).err().expect("target");
        assert!(missing.contains("requires a target"), "{missing}");
        assert!(options(&["read", "s:a.rs#a", "--limit", "1"]).is_ok());
        let plain = options(&["read", "m:a.rs"]).expect("read");
        assert!(!plain.budget_set);
        let sized = options(&["read", "m:a.rs", "--budget", "8"]).expect("budget");
        assert!(sized.budget_set);
        assert_eq!(sized.request.budget.tokens(), 8);
        assert!(options(&["map", "m:a.rs", "--cursor", "v1|1|map|s:a.rs#a"]).is_ok());
        let profiled = options(&["find", "--profile", "kept"]).expect("profile");
        assert!(profiled.profile);
        assert_eq!(profiled.request.target, "kept");
        assert!(!options(&["find", "kept"]).expect("plain").profile);
        assert!(HELP.contains("--profile"));
        assert!(HELP.contains("--meta"));
        assert!(options(&["find", "--meta", "kept"]).expect("meta").meta);
        assert!(!options(&["find", "kept"]).expect("plain").meta);
        assert!(HELP.contains("DCTX_PROFILE=1"));
        assert!(HELP.contains("DCTX_MAX_SOURCE_BYTES"));
        let grepped = options(&["grep", "NEEDLE", "-i", "--in", "src"]).expect("grep");
        assert!(grepped.grep && grepped.ignore_case);
        assert_eq!(grepped.request.target, "NEEDLE");
        assert_eq!(grepped.request.within.as_deref(), Some("src"));
        assert!(!options(&["find", "NEEDLE"]).expect("find").grep);
        assert!(options(&["find", "NEEDLE", "-i"]).is_err());
        assert!(options(&["grep", "NEEDLE", "--kind", "fn"]).is_err());
        assert!(options(&["grep"]).is_err());
        let skeleton = options(&["skeleton", "a.py", "--json"]).expect("skeleton");
        assert!(skeleton.skeleton && skeleton.request.verb == Verb::Map);
        assert!(!options(&["map", "a.py"]).expect("map").skeleton);
        assert!(options(&["skeleton"]).err().expect("target").contains("skeleton requires a target"));
        assert!(options(&["skeleton", "a.py", "--depth", "2"]).is_err());
        assert!(HELP.contains("|skeleton|grep>"));
        assert_eq!(
            options(&["read", "s:a.rs#a", "--lines", "35-36"])
                .expect("lines")
                .request
                .lines,
            Some([35, 36])
        );
        for args in [
            vec!["index"],
            vec!["stats"],
            vec!["find"],
            vec!["find", "x", "--budget", "2001"],
            vec!["neighbors", "x", "--depth", "5"],
            vec!["read", "x", "--lines", "4-2"],
            vec!["read", "x", "--limit", "2"],
            vec!["map", "--dir", "out"],
            vec!["map", "m:a.rs", "--cursor", ""],
            vec!["explain", "s:a.rs#a", "--limit", "60"],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
