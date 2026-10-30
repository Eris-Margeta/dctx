//! `dctx mcp`: a stdio MCP server (JSON-RPC 2.0, one message per line) exposing the map verbs as tools.
//! Every `tools/call` builds the argument vector of `dctx map <verb> ...` and runs `map::execute`, the CLI's own code path.
//! Only JSON-RPC lines go to stdout; diagnostics stay on stderr. The server exits 0 at end of input.

mod json;
mod tools;

use json::Json;
use std::io::{BufRead, Write};

/// Protocol version answered when the client names none.
const DEFAULT_PROTOCOL: &str = "2025-06-18";
/// Usage text for `dctx mcp --help`.
const HELP: &str = "Usage: dctx mcp [PATH]\n  Serve the map verbs (map, find, neighbors, explain, read, hubs, grep) as MCP tools over stdio.\n  One JSON-RPC 2.0 message per line. Tools run in PATH (default: the current directory).\n";

/// Entry point for `dctx mcp [PATH]`: serves stdin to stdout until end of input.
pub(crate) fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => {}
        [flag] if flag == "--help" || flag == "-h" => {
            print!("{HELP}");
            return Ok(());
        }
        [path] if !path.starts_with('-') => std::env::set_current_dir(path).map_err(|error| format!("cannot enter {path}: {error}"))?,
        _ => return Err("usage: dctx mcp [PATH]".into()),
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock(), &crate::map::execute).map_err(|error| error.to_string())
}

/// Reads one message per line from `input` and writes replies to `output`, running tools through `exec`.
fn serve(mut input: impl BufRead, mut output: impl Write, exec: &dyn Fn(&[String]) -> Result<String, String>) -> std::io::Result<()> {
    let mut raw = Vec::new();
    loop {
        raw.clear();
        if input.read_until(b'\n', &mut raw)? == 0 {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&raw);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(line.trim(), exec) {
            writeln!(output, "{}", reply.to_line())?;
            output.flush()?;
        }
    }
}

fn error(id: Json, code: i32, message: &str) -> Json {
    let body = Json::obj(vec![("code", Json::Num(code.to_string())), ("message", Json::str(message))]);
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id), ("error", body)])
}

fn success(id: Json, result: Json) -> Json {
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id), ("result", result)])
}

/// Handles one message line; `None` means no reply (notifications and stray responses).
fn handle(line: &str, exec: &dyn Fn(&[String]) -> Result<String, String>) -> Option<Json> {
    let message = match json::parse(line) {
        Ok(message) => message,
        Err(why) => return Some(error(Json::Null, -32700, &format!("parse error: {why}"))),
    };
    if !matches!(message, Json::Obj(_)) {
        return Some(error(Json::Null, -32600, "invalid request: expected a JSON object"));
    }
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Json::as_str) else {
        // A response from the client carries no method and needs no answer.
        if message.get("result").is_some() || message.get("error").is_some() {
            return None;
        }
        return Some(error(id.unwrap_or(Json::Null), -32600, "invalid request: missing method"));
    };
    let id = id?;
    let params = message.get("params");
    Some(match method {
        "initialize" => {
            let version = params.and_then(|p| p.get("protocolVersion")).and_then(Json::as_str).unwrap_or(DEFAULT_PROTOCOL);
            let info = Json::obj(vec![("name", Json::str("dctx")), ("version", Json::str(env!("CARGO_PKG_VERSION")))]);
            let capabilities = Json::obj(vec![("tools", Json::obj(Vec::new()))]);
            success(id, Json::obj(vec![("protocolVersion", Json::str(version)), ("capabilities", capabilities), ("serverInfo", info)]))
        }
        "ping" => success(id, Json::obj(Vec::new())),
        "tools/list" => success(id, tools::list()),
        "tools/call" => call(id, params, exec),
        other => error(id, -32601, &format!("method not found: {other}")),
    })
}

fn call(id: Json, params: Option<&Json>, exec: &dyn Fn(&[String]) -> Result<String, String>) -> Json {
    let Some(name) = params.and_then(|p| p.get("name")).and_then(Json::as_str) else {
        return error(id, -32602, "invalid params: tool name is required");
    };
    if !tools::exists(name) {
        return error(id, -32602, &format!("unknown tool: {name}"));
    }
    let outcome = tools::to_args(name, params.and_then(|p| p.get("arguments"))).and_then(|args| exec(&args));
    let (text, failed) = match outcome {
        Ok(text) => (text, false),
        Err(message) => (message, true),
    };
    let content = Json::Arr(vec![Json::obj(vec![("type", Json::str("text")), ("text", Json::Str(text))])]);
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
            if args[0] == "find" && args[1] == "boom" { Err("bad target".into()) } else { Ok(format!("ran {}", args.join(" "))) }
        };
        let mut out = Vec::new();
        serve(lines.as_bytes(), &mut out, &exec).expect("serve");
        String::from_utf8(out).expect("utf8").lines().map(|l| json::parse(l).expect("reply is JSON")).collect()
    }

    #[test]
    fn protocol_flow_and_errors() {
        let replies = session(concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, "\n",
            "\n",
            r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#, "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find","arguments":{"query":"beta"}}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"find","arguments":{"query":"boom"}}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#, "\n",
            r#"{"jsonrpc":"2.0","id":6,"method":"resources/list"}"#, "\n",
            r#"{"jsonrpc":"2.0","method":"unknown/notice"}"#, "\n",
            "not json\n",
        ));
        assert_eq!(replies.len(), 8);
        let result = |n: usize, key: &str| replies[n].get("result").and_then(|r| r.get(key)).cloned();
        assert_eq!(result(0, "protocolVersion"), Some(Json::str("2024-11-05")));
        assert_eq!(replies[1].get("id"), Some(&Json::str("a")));
        assert!(matches!(result(2, "tools"), Some(Json::Arr(tools)) if tools.len() == 7));
        let text = |n: usize| match result(n, "content") {
            Some(Json::Arr(items)) => items[0].get("text").and_then(Json::as_str).map(str::to_string),
            _ => None,
        };
        assert_eq!(text(3).as_deref(), Some("ran find beta"));
        assert_eq!(text(4).as_deref(), Some("bad target"));
        assert_eq!(replies[4].get("result").and_then(|r| r.get("isError")), Some(&Json::Bool(true)));
        for (n, code) in [(5, "-32602"), (6, "-32601"), (7, "-32700")] {
            assert_eq!(replies[n].get("error").and_then(|e| e.get("code")), Some(&Json::Num(code.into())), "{n}");
        }
    }
}
