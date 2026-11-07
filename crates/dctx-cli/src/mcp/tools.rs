// SPDX-License-Identifier: Apache-2.0
//! The MCP tool table: names, input schemas and the mapping from a call to `dctx map` arguments.
//! A call becomes the same argument vector the CLI would receive, so `map::execute` is the only code path.

use super::json::Json;

/// One input property of a tool.
struct Prop {
    /// Property name; the CLI flag is `--` plus this name with `_` replaced by `-`.
    name: &'static str,
    /// JSON Schema type: `string`, `integer`, `boolean` or `array` (of strings, joined with commas).
    ty: &'static str,
    /// Short description shown to the model.
    help: &'static str,
}

/// One tool: a `dctx map` verb with an optional positional first argument (and optionally a second).
struct Tool {
    /// Tool name, equal to the map verb.
    name: &'static str,
    /// Description shown to the model.
    help: &'static str,
    /// Property that becomes the first positional argument, if the verb has one.
    positional: Option<&'static str>,
    /// Property that becomes the second positional argument (used by `path` for `to`).
    second_positional: Option<&'static str>,
    /// All verb-specific properties, positional first.
    props: &'static [Prop],
}

const fn prop(name: &'static str, ty: &'static str, help: &'static str) -> Prop {
    Prop { name, ty, help }
}

/// Properties every verb accepts (some verbs drop some of them).
const COMMON: [Prop; 3] = [
    prop("budget", "integer", "Output token budget, 1..2000"),
    prop(
        "cursor",
        "string",
        "The next token of an earlier page, to continue it",
    ),
    prop("json", "boolean", "Return facade JSON v2 instead of text"),
];

const TOOLS: &[Tool] = &[
    Tool {
        name: "map",
        help: "Structural map of a file, directory or node: its symbols and their relations.",
        positional: Some("target"),
        second_positional: None,
        props: &[
            prop("target", "string", "Stable id (m:, s:, ...) or a place-relative file path"),
            prop("depth", "integer", "Levels to expand, 1..3"),
            prop("limit", "integer", "Page size"),
        ],
    },
    Tool {
        name: "find",
        help: "Find symbols, modules and docs by name or words.",
        positional: Some("query"),
        second_positional: None,
        props: &[
            prop("query", "string", "Words or a name to look for"),
            prop("kind", "array", "Kinds: fn, method, struct, enum, trait, impl, const, type, macro, class, interface, symbol, module, package, place, doc, test"),
            prop("in", "string", "Only within this place-relative path"),
            prop("limit", "integer", "Page size"),
        ],
    },
    Tool {
        name: "neighbors",
        help: "Nodes connected to a node by calls or imports, up to 4 hops.",
        positional: Some("id"),
        second_positional: None,
        props: &[
            prop("id", "string", "Stable node id from a find or map answer"),
            prop("edge", "array", "Edge kinds, for example calls, imports"),
            prop("dir", "string", "Direction: in, out or both"),
            prop("depth", "integer", "Hops, 1..4"),
            prop("limit", "integer", "Page size"),
        ],
    },
    Tool {
        name: "explain",
        help: "Describe one node: its kind, place, signature and relations.",
        positional: Some("id"),
        second_positional: None,
        props: &[prop("id", "string", "Stable node id")],
    },
    Tool {
        name: "read",
        help: "Read the source of one node, or of absolute lines inside it.",
        positional: Some("id"),
        second_positional: None,
        props: &[
            prop("id", "string", "Stable node id"),
            prop("lines", "string", "Absolute source lines inside the node, as A-B"),
            prop("full", "boolean", "Permit up to 400 lines"),
        ],
    },
    Tool {
        name: "hubs",
        help: "Most-called symbols and per-directory file and symbol counts of the place.",
        positional: None,
        second_positional: None,
        props: &[prop("limit", "integer", "Hub lines, 1..60 (default 20)")],
    },
    Tool {
        name: "grep",
        help: "Exhaustive literal search of indexed files, hits grouped by enclosing symbol.",
        positional: Some("query"),
        second_positional: None,
        props: &[
            prop("query", "string", "The literal text to search for"),
            prop("ignore_case", "boolean", "ASCII case-insensitive match"),
            prop("in", "string", "Only within this place-relative path"),
            prop("limit", "integer", "Page size"),
        ],
    },
    Tool {
        name: "skeleton",
        help: "Skeleton of a file or directory: its symbols and their relations, without depth expansion.",
        positional: Some("target"),
        second_positional: None,
        props: &[
            prop("target", "string", "Stable id (m:, s:, ...) or a place-relative file path"),
        ],
    },
    Tool {
        name: "path",
        help: "Shortest edge path between two nodes (calls, imports, contains), at most 8 hops.",
        positional: Some("from"),
        second_positional: Some("to"),
        props: &[
            prop("from", "string", "Start node id (s:...) or place-relative file"),
            prop("to", "string", "End node id (s:...) or place-relative file"),
            prop("meta", "boolean", "Include hop metadata"),
        ],
    },
    Tool {
        name: "export",
        help: "Whole graph as one deterministic document: graph JSON or a Mermaid flowchart.",
        positional: None,
        second_positional: None,
        props: &[
            prop("format", "string", "Output format: json or mermaid"),
        ],
    },
];

/// Every property `tool` accepts: its own, then the common ones not already in its own set.
fn accepted(tool: &Tool) -> Vec<&Prop> {
    let own_names: std::collections::HashSet<&str> = tool.props.iter().map(|p| p.name).collect();
    tool.props
        .iter()
        .chain(COMMON.iter().filter(|extra| {
            !(own_names.contains(extra.name)
                || tool.name == "hubs" && extra.name == "cursor"
                || tool.name == "export" && extra.name != "budget"
                || tool.name == "skeleton" && extra.name != "budget" && extra.name != "json")
        }))
        .collect()
}

fn schema_of(tool: &Tool) -> Json {
    let properties = accepted(tool)
        .into_iter()
        .map(|p| {
            let mut members = vec![
                ("type", Json::str(p.ty)),
                ("description", Json::str(p.help)),
            ];
            if p.ty == "array" {
                members.insert(1, ("items", Json::obj(vec![("type", Json::str("string"))])));
            }
            (p.name.to_string(), Json::obj(members))
        })
        .collect();
    let mut required_names = Vec::new();
    if let Some(name) = tool.positional {
        required_names.push(Json::str(name));
    }
    if let Some(name) = tool.second_positional {
        required_names.push(Json::str(name));
    }
    if tool.name == "export" {
        required_names.push(Json::str("format"));
    }
    Json::obj(vec![
        ("type", Json::str("object")),
        ("properties", Json::Obj(properties)),
        ("required", Json::Arr(required_names)),
        ("additionalProperties", Json::Bool(false)),
    ])
}

/// The `tools/list` result.
pub(super) fn list() -> Json {
    let tools = TOOLS
        .iter()
        .map(|tool| {
            Json::obj(vec![
                ("name", Json::str(tool.name)),
                ("description", Json::str(tool.help)),
                ("inputSchema", schema_of(tool)),
            ])
        })
        .collect();
    Json::obj(vec![("tools", Json::Arr(tools))])
}

/// True when `name` is a known tool.
pub(super) fn exists(name: &str) -> bool {
    TOOLS.iter().any(|tool| tool.name == name)
}

/// Turns a call into the argument vector of `dctx map`, or a message naming the bad argument.
pub(super) fn to_args(name: &str, arguments: Option<&Json>) -> Result<Vec<String>, String> {
    let tool = TOOLS
        .iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| format!("unknown tool {name}"))?;
    let members: &[(String, Json)] = match arguments {
        None | Some(Json::Null) => &[],
        Some(Json::Obj(members)) => members,
        Some(_) => return Err("arguments must be an object".into()),
    };
    let known = accepted(tool);
    for (key, _) in members {
        if !known.iter().any(|p| p.name == key) {
            return Err(format!("unknown argument {key} for {name}"));
        }
    }
    let mut out = vec![name.to_string()];
    if let Some(first) = tool.positional {
        let value = members
            .iter()
            .find(|(key, _)| key == first)
            .map(|(_, value)| value);
        match value.and_then(Json::as_str) {
            Some(text) if !text.is_empty() => out.push(text.to_string()),
            _ => {
                return Err(format!(
                    "{first} is required and must be a non-empty string"
                ))
            }
        }
    }
    if let Some(second) = tool.second_positional {
        let value = members
            .iter()
            .find(|(key, _)| key == second)
            .map(|(_, value)| value);
        match value.and_then(Json::as_str) {
            Some(text) if !text.is_empty() => out.push(text.to_string()),
            _ => {
                return Err(format!(
                    "{second} is required and must be a non-empty string"
                ))
            }
        }
    }
    if tool.name == "export" {
        let value = members
            .iter()
            .find(|(key, _)| key == "format")
            .map(|(_, value)| value);
        match value.and_then(Json::as_str) {
            Some(text) if !text.is_empty() => {
                out.extend(["--format".to_string(), text.to_string()])
            }
            _ => return Err("format is required and must be json or mermaid".into()),
        }
    }
    for p in known.iter().filter(|p| {
        Some(p.name) != tool.positional
            && Some(p.name) != tool.second_positional
            && !(tool.name == "export" && p.name == "format")
    }) {
        let Some((_, value)) = members.iter().find(|(key, _)| key == p.name) else {
            continue;
        };
        if matches!(value, Json::Null) {
            continue;
        }
        let flag = format!("--{}", p.name.replace('_', "-"));
        match (p.ty, value) {
            ("boolean", Json::Bool(true)) => out.push(flag),
            ("boolean", Json::Bool(false)) => {}
            ("integer", Json::Num(text) | Json::Str(text))
            | ("string" | "array", Json::Str(text)) => out.extend([flag, text.clone()]),
            ("array", Json::Arr(items)) => {
                let parts: Option<Vec<&str>> = items.iter().map(Json::as_str).collect();
                out.extend([
                    flag,
                    parts
                        .ok_or_else(|| format!("{} must be an array of strings", p.name))?
                        .join(","),
                ]);
            }
            _ => return Err(format!("{} must be of type {}", p.name, p.ty)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::json::parse;

    fn args(name: &str, json: &str) -> Result<Vec<String>, String> {
        to_args(name, Some(&parse(json).expect("json")))
    }

    #[test]
    fn calls_become_the_cli_argument_vector() {
        assert_eq!(
            args(
                "find",
                r#"{"query":"beta","kind":["fn","type"],"in":"src","json":true}"#
            )
            .unwrap(),
            ["find", "beta", "--kind", "fn,type", "--in", "src", "--json"]
        );
        assert_eq!(
            args("neighbors", r#"{"id":"s:a#b","depth":3,"dir":"in"}"#).unwrap(),
            ["neighbors", "s:a#b", "--dir", "in", "--depth", "3"]
        );
        assert_eq!(
            args("grep", r#"{"query":"x","ignore_case":true}"#).unwrap(),
            ["grep", "x", "--ignore-case"]
        );
        assert_eq!(args("hubs", "{}").unwrap(), ["hubs"]);
        assert_eq!(
            args("read", r#"{"id":"s:a#b","lines":"3-9"}"#).unwrap(),
            ["read", "s:a#b", "--lines", "3-9"]
        );
    }

    #[test]
    fn bad_arguments_are_named() {
        assert!(args("find", "{}").unwrap_err().contains("query"));
        assert!(args("find", r#"{"query":"a","bogus":1}"#)
            .unwrap_err()
            .contains("bogus"));
        assert!(args("find", r#"{"query":"a","limit":true}"#)
            .unwrap_err()
            .contains("limit"));
        assert!(args("hubs", r#"{"cursor":"x"}"#).is_err());
        assert!(list().get("tools").is_some() && exists("map") && exists("path"));
    }

    #[test]
    fn new_tools_become_cli_arguments() {
        assert_eq!(
            args("skeleton", r#"{"target":"m:a.py"}"#).unwrap(),
            ["skeleton", "m:a.py"]
        );
        assert_eq!(
            args("skeleton", r#"{"target":"m:a.py","json":true}"#).unwrap(),
            ["skeleton", "m:a.py", "--json"]
        );
        assert_eq!(
            args("path", r#"{"from":"s:a.py#a","to":"s:a.py#b"}"#).unwrap(),
            ["path", "s:a.py#a", "s:a.py#b"]
        );
        assert_eq!(
            args(
                "path",
                r#"{"from":"s:a.py#a","to":"s:a.py#b","json":true,"meta":true}"#
            )
            .unwrap(),
            ["path", "s:a.py#a", "s:a.py#b", "--meta", "--json"]
        );
        assert_eq!(
            args("export", r#"{"format":"json"}"#).unwrap(),
            ["export", "--format", "json"]
        );
        assert_eq!(
            args("export", r#"{"format":"mermaid"}"#).unwrap(),
            ["export", "--format", "mermaid"]
        );
        assert!(args("path", r#"{"from":"s:a"}"#)
            .unwrap_err()
            .contains("to"));
        assert!(args("export", "{}").unwrap_err().contains("format"));
    }

    #[test]
    fn all_ten_tools_are_registered() {
        let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "map",
                "find",
                "neighbors",
                "explain",
                "read",
                "hubs",
                "grep",
                "skeleton",
                "path",
                "export"
            ]
        );
    }
}
