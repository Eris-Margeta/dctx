// SPDX-License-Identifier: Apache-2.0
//! `dctx mcp`: a stdio MCP server (JSON-RPC 2.0, one message per line) exposing the map verbs as tools.
//! Every `tools/call` builds the argument vector of `dctx map <verb> ...` and runs `map::execute`, the CLI's own code path.
//! Only JSON-RPC lines go to stdout; diagnostics stay on stderr. The server exits 0 at end of input.

mod json;
mod tools;

use json::Json;
use std::io::{BufRead, Write};
use tools::Surface;

use crate::exit::Failure;

/// Protocol version answered when the client names none.
const MAX_LINE_BYTES: u64 = 1024 * 1024;

const DEFAULT_PROTOCOL: &str = "2025-06-18";
/// Usage text for `dctx mcp --help`.
const HELP: &str = "Usage: dctx mcp [--profile default|lean | --tools NAME,...] [PATH]\n  Serve map verbs as MCP tools over stdio; one JSON-RPC 2.0 message per line.\n  Default: map, find, neighbors, explain, read, hubs, grep, skeleton, path, export.\n  --profile lean: find and read, with compact descriptions and all parameters.\n  --tools NAME,...: expose only the named tools, with compact descriptions.\n  Select either --profile or --tools, once. Tools run in PATH (default: current directory).\n  Use -- before a PATH starting with '-'. Restart the server to change tools.\nExample: dctx mcp ~/src/project\n          dctx mcp --profile lean ~/src/project\n          dctx mcp --tools find,read,grep ~/src/project\n";

/// Validate all arguments before changing directory or starting the protocol.
fn options(args: &[String]) -> Result<(Option<String>, Surface), Failure> {
    let mut path = None;
    let mut surface = None;
    let mut positional_only = false;
    let mut args = args.iter();
    let usage = |message| Failure::usage(message, "run `dctx mcp --help` for options and examples");
    while let Some(arg) = args.next() {
        if !positional_only && arg == "--" {
            positional_only = true;
        } else if !positional_only && arg.starts_with('-') {
            let (flag, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
            if !matches!(flag, "--profile" | "--tools") {
                return Err(usage(format!("unknown MCP option: {arg}")));
            }
            if surface.is_some() {
                return Err(usage("select either --profile or --tools, once".into()));
            }
            let value = inline
                .or_else(|| args.next().map(String::as_str))
                .ok_or_else(|| usage(format!("{flag} requires a value")))?;
            surface = Some(
                match flag {
                    "--profile" => Surface::profile(value),
                    _ => Surface::from_names(value),
                }
                .map_err(usage)?,
            );
        } else if path.replace(arg.clone()).is_some() {
            return Err(usage("expected at most one project PATH".into()));
        }
    }
    Ok((path, surface.unwrap_or_default()))
}

/// Entry point for `dctx mcp [PATH]`: serves stdin to stdout until end of input.
/// A bad argument is exit 2 and a PATH that cannot be entered exit 3 (docs/CLI-EXIT-CODES.md).
pub(crate) fn run(args: &[String]) -> Result<(), Failure> {
    if matches!(args, [flag] if flag == "--help" || flag == "-h") {
        print!("{HELP}");
        return Ok(());
    }
    let (path, surface) = options(args)?;
    if let Some(path) = path {
        std::env::set_current_dir(&path).map_err(|error| {
            Failure::index(
                format!("cannot enter {path}: {error}"),
                "pass an existing project directory: dctx mcp <PATH>",
            )
        })?;
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock(), &surface, &crate::map::execute).map_err(|error| {
        Failure::index(
            error.to_string(),
            "stdin or stdout failed; restart the server from the MCP client",
        )
    })
}

/// Reads one message per line from `input` and writes replies to `output`, running tools through `exec`.
fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    surface: &Surface,
    exec: &dyn Fn(&[String]) -> Result<String, String>,
) -> std::io::Result<()> {
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if std::io::Read::take(&mut input, MAX_LINE_BYTES + 1).read_until(b'\n', &mut raw)? == 0 {
            return Ok(());
        }
        if raw.len() as u64 > MAX_LINE_BYTES {
            // End the session: an attacker cannot force an unlimited drain of a line without LF.
            writeln!(
                output,
                "{}",
                error(Json::Null, -32700, "message exceeds 1048576 bytes").to_line()
            )?;
            output.flush()?;
            return Ok(());
        }
        let line = match std::str::from_utf8(&raw) {
            Ok(line) => line,
            Err(_) => {
                writeln!(
                    output,
                    "{}",
                    error(Json::Null, -32700, "message is not UTF-8").to_line()
                )?;
                output.flush()?;
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(line.trim(), surface, exec) {
            writeln!(output, "{}", reply.to_line())?;
            output.flush()?;
        }
    }
}

fn error(id: Json, code: i32, message: &str) -> Json {
    let body = Json::obj(vec![
        ("code", Json::Num(code.to_string())),
        ("message", Json::str(message)),
    ]);
    Json::obj(vec![
        ("jsonrpc", Json::str("2.0")),
        ("id", id),
        ("error", body),
    ])
}

fn success(id: Json, result: Json) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::str("2.0")),
        ("id", id),
        ("result", result),
    ])
}

/// Handles one message line; `None` means no reply (notifications and stray responses).
fn handle(
    line: &str,
    surface: &Surface,
    exec: &dyn Fn(&[String]) -> Result<String, String>,
) -> Option<Json> {
    let message = match json::parse(line) {
        Ok(message) => message,
        Err(why) => return Some(error(Json::Null, -32700, &format!("parse error: {why}"))),
    };
    if !matches!(message, Json::Obj(_)) {
        return Some(error(
            Json::Null,
            -32600,
            "invalid request: expected a JSON object",
        ));
    }
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Json::as_str) else {
        // A response from the client carries no method and needs no answer.
        if message.get("result").is_some() || message.get("error").is_some() {
            return None;
        }
        return Some(error(
            id.unwrap_or(Json::Null),
            -32600,
            "invalid request: missing method",
        ));
    };
    let id = id?;
    let params = message.get("params");
    Some(match method {
        "initialize" => {
            let version = params
                .and_then(|p| p.get("protocolVersion"))
                .and_then(Json::as_str)
                .unwrap_or(DEFAULT_PROTOCOL);
            let info = Json::obj(vec![
                ("name", Json::str("dctx")),
                ("version", Json::str(env!("CARGO_PKG_VERSION"))),
            ]);
            let capabilities = Json::obj(vec![("tools", Json::obj(Vec::new()))]);
            success(
                id,
                Json::obj(vec![
                    ("protocolVersion", Json::str(version)),
                    ("capabilities", capabilities),
                    ("serverInfo", info),
                ]),
            )
        }
        "ping" => success(id, Json::obj(Vec::new())),
        "tools/list" => success(id, tools::list(surface)),
        "tools/call" => call(id, params, surface, exec),
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

fn call(
    id: Json,
    params: Option<&Json>,
    surface: &Surface,
    exec: &dyn Fn(&[String]) -> Result<String, String>,
) -> Json {
    let Some(name) = params.and_then(|p| p.get("name")).and_then(Json::as_str) else {
        return error(id, -32602, "invalid params: tool name is required");
    };
    if !surface.contains(name) {
        return error(id, -32602, &format!("unknown tool: {name}"));
    }
    let outcome =
        tools::to_args(name, params.and_then(|p| p.get("arguments"))).and_then(|args| exec(&args));
    let (text, failed) = match outcome {
        Ok(text) => (text, false),
        Err(message) => (message, true),
    };
    let content = Json::Arr(vec![Json::obj(vec![
        ("type", Json::str("text")),
        ("text", Json::Str(text)),
    ])]);
    let mut members = vec![("content", content)];
    if failed {
        members.push(("isError", Json::Bool(true)));
    }
    success(id, Json::obj(members))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(lines: &str) -> Vec<Json> {
        let exec = |args: &[String]| -> Result<String, String> {
            if args[0] == "find" && args[1] == "boom" {
                Err("bad target".into())
            } else {
                Ok(format!("ran {}", args.join(" ")))
            }
        };
        let mut out = Vec::new();
        serve(lines.as_bytes(), &mut out, &Surface::Full, &exec).expect("serve");
        String::from_utf8(out)
            .expect("utf8")
            .lines()
            .map(|l| json::parse(l).expect("reply is JSON"))
            .collect()
    }

    #[test]
    fn oversized_frames_stop_after_a_bounded_read_without_dispatching() {
        let mut input = std::io::Cursor::new(vec![b'x'; MAX_LINE_BYTES as usize + 10]);
        let mut output = Vec::new();
        serve(&mut input, &mut output, &Surface::default(), &|_| {
            panic!("must not dispatch")
        })
        .unwrap();
        assert_eq!(input.position(), MAX_LINE_BYTES + 1);
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("message exceeds"));
    }

    #[test]
    fn invalid_utf8_is_rejected_and_the_next_frame_is_processed() {
        let mut output = Vec::new();
        serve(
            &b"\xff\n{\"id\":1,\"method\":\"ping\"}\n"[..],
            &mut output,
            &Surface::default(),
            &|_| panic!("must not dispatch"),
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.contains("message is not UTF-8"));
        assert!(text.contains("\"result\""));
    }

    #[test]
    fn protocol_flow_and_errors() {
        let replies = session(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find","arguments":{"query":"beta"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find","arguments":{"query":"boom"}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"unknown/notice"}"#,
            "\n",
            "not json\n",
        ));
        assert_eq!(replies.len(), 8);
        let result =
            |n: usize, key: &str| replies[n].get("result").and_then(|r| r.get(key)).cloned();
        assert_eq!(result(0, "protocolVersion"), Some(Json::str("2024-11-05")));
        assert_eq!(replies[1].get("id"), Some(&Json::str("a")));
        assert!(matches!(result(2, "tools"), Some(Json::Arr(tools)) if tools.len() == 10));
        let text = |n: usize| match result(n, "content") {
            Some(Json::Arr(items)) => items[0]
                .get("text")
                .and_then(Json::as_str)
                .map(str::to_string),
            _ => None,
        };
        assert_eq!(text(3).as_deref(), Some("ran find beta"));
        assert_eq!(text(4).as_deref(), Some("bad target"));
        assert_eq!(
            replies[4].get("result").and_then(|r| r.get("isError")),
            Some(&Json::Bool(true))
        );
        for (n, code) in [(5, "-32602"), (6, "-32601"), (7, "-32700")] {
            assert_eq!(
                replies[n].get("error").and_then(|e| e.get("code")),
                Some(&Json::Num(code.into())),
                "{n}"
            );
        }
    }

    #[test]
    fn options_accept_profiles_subsets_and_paths_in_either_order() {
        for (words, expected_path, expected_names) in [
            (
                vec![],
                None,
                vec![
                    "map",
                    "find",
                    "neighbors",
                    "explain",
                    "read",
                    "hubs",
                    "grep",
                    "skeleton",
                    "path",
                    "export",
                ],
            ),
            (
                vec!["--profile", "default"],
                None,
                vec![
                    "map",
                    "find",
                    "neighbors",
                    "explain",
                    "read",
                    "hubs",
                    "grep",
                    "skeleton",
                    "path",
                    "export",
                ],
            ),
            (
                vec!["--profile", "lean", "project dir"],
                Some("project dir"),
                vec!["find", "read"],
            ),
            (
                vec!["project", "--profile=lean"],
                Some("project"),
                vec!["find", "read"],
            ),
            (vec!["--tools", "read,find"], None, vec!["find", "read"]),
            (
                vec!["project", "--tools=grep"],
                Some("project"),
                vec!["grep"],
            ),
            (
                vec!["--profile", "lean", "--", "-project"],
                Some("-project"),
                vec!["find", "read"],
            ),
        ] {
            let args: Vec<_> = words.into_iter().map(str::to_string).collect();
            let (path, surface) = options(&args).unwrap();
            assert_eq!(path.as_deref(), expected_path);
            let listed = tools::list(&surface);
            let Some(Json::Arr(tools)) = listed.get("tools") else {
                panic!("tools array");
            };
            let names: Vec<_> = tools
                .iter()
                .map(|t| t.get("name").unwrap().as_str().unwrap())
                .collect();
            assert_eq!(names, expected_names);
        }
    }

    #[test]
    fn invalid_options_are_usage_errors_before_serving() {
        for words in [
            vec!["--tools", "find,nope"],
            vec!["--tools="],
            vec!["--tools", "find,find"],
            vec!["--tools", "find,"],
            vec!["--tools"],
            vec!["--profile"],
            vec!["--profile", "nope"],
            vec!["--profile="],
            vec!["--profile", "lean", "--tools", "find"],
            vec!["--tools", "find", "--profile", "lean"],
            vec!["--tools", "find", "--tools", "read"],
            vec!["--profile", "lean", "--profile", "default"],
            vec!["--bogus"],
            vec!["a", "b"],
        ] {
            let args: Vec<_> = words.into_iter().map(str::to_string).collect();
            let failure = options(&args).unwrap_err();
            assert_eq!(failure.code, crate::exit::USAGE, "{args:?}");
            assert!(!failure.hint.is_empty());
        }
    }

    #[test]
    fn restricted_sessions_list_and_dispatch_only_selected_tools() {
        let lines = concat!(
            "{\"id\":1,\"method\":\"initialize\"}\n",
            "{\"id\":2,\"method\":\"tools/list\"}\n",
            "{\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"find\",\"arguments\":{\"query\":\"beta\"}}}\n",
            "{\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{\"id\":\"s:a#b\"}}}\n",
            "{\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"map\"}}\n",
            "{\"id\":6,\"method\":\"tools/call\",\"params\":{\"name\":\"nope\"}}\n",
        );
        for surface in [
            Surface::profile("lean").unwrap(),
            Surface::from_names("read,find").unwrap(),
        ] {
            let exec = |args: &[String]| {
                assert!(matches!(args[0].as_str(), "find" | "read"));
                Ok(args.join(" "))
            };
            let mut out = Vec::new();
            serve(lines.as_bytes(), &mut out, &surface, &exec).unwrap();
            let out = String::from_utf8(out).unwrap();
            let replies: Vec<_> = out.lines().map(|line| json::parse(line).unwrap()).collect();
            assert_eq!(replies.len(), 6);
            assert_eq!(
                replies[0],
                session("{\"id\":1,\"method\":\"initialize\"}\n")[0]
            );
            assert_eq!(replies[1].get("result"), Some(&tools::list(&surface)));
            for (reply, expected) in [(&replies[2], "find beta"), (&replies[3], "read s:a#b")] {
                let Some(Json::Arr(content)) = reply.get("result").and_then(|r| r.get("content"))
                else {
                    panic!("tool content");
                };
                assert_eq!(content[0].get("text"), Some(&Json::str(expected)));
            }
            for reply in &replies[4..] {
                assert_eq!(
                    reply.get("error").and_then(|e| e.get("code")),
                    Some(&Json::Num("-32602".into()))
                );
            }
        }
    }
}
