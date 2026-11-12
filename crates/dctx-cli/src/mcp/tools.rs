// SPDX-License-Identifier: Apache-2.0
//! MCP tool names, input schemas, and the mapping from calls to CLI arguments.
//! Ordinary calls use map verbs; context reads and task packs use their existing CLI paths.

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

/// One read-only CLI operation with up to two positional arguments.
struct Tool {
    /// Stable MCP tool name.
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
    prop("budget", "integer", "Tokens, 1..2000."),
    prop("cursor", "string", "Resume previous next."),
    prop("json", "boolean", ""),
];

const TOOLS: &[Tool] = &[
    Tool {
        name: "map",
        help: "Map file/directory symbols and relations.",
        positional: Some("target"),
        second_positional: None,
        props: &[
            prop("target", "string", "Node id or relative file path."),
            prop("depth", "integer", "Levels, 1..3."),
            prop("limit", "integer", ""),
        ],
    },
    Tool {
        name: "find",
        help: "Find definitions by name or words; use read context=true for source and callers.",
        positional: Some("query"),
        second_positional: None,
        props: &[
            prop("query", "string", ""),
            prop("kind", "array", "fn, method, struct, enum, trait, impl, const, type, macro, class, interface, symbol, module, package, place, doc, test."),
            prop("in", "string", ""),
            prop("limit", "integer", ""),
        ],
    },
    Tool {
        name: "neighbors",
        help: "Follow calls/imports by node id; dir=in finds callers.",
        positional: Some("id"),
        second_positional: None,
        props: &[
            prop("id", "string", "Node id from find/map."),
            prop("edge", "array", ""),
            prop("dir", "string", "in, out, both."),
            prop("depth", "integer", "Hops, 1..4."),
            prop("limit", "integer", ""),
        ],
    },
    Tool {
        name: "explain",
        help: "Show a node signature, location and relations.",
        positional: Some("id"),
        second_positional: None,
        props: &[prop("id", "string", "")],
    },
    Tool {
        name: "read",
        help: "Read source/lines. context=true locates a name/file and includes source plus callers/imports.",
        positional: Some("id"),
        second_positional: None,
        props: &[
            prop("id", "string", "Node id; context also accepts names/files."),
            prop("lines", "string", "Absolute lines A-B within the node."),
            prop("full", "boolean", "Allow 400 lines."),
            prop("context", "boolean", "Source + callers/imports."),
            prop("session", "string", "Context session id."),
            prop("dedupe", "boolean", "Reference prior session chunks."),
            prop("budget", "integer", "Source 1..2000 (default 2000); context 100..100000 (default 3000)."),
        ],
    },
    Tool {
        name: "hubs",
        help: "Rank called symbols and count files/symbols by directory.",
        positional: None,
        second_positional: None,
        props: &[prop("limit", "integer", "")],
    },
    Tool {
        name: "grep",
        help: "Search indexed files literally, grouped by symbol.",
        positional: Some("query"),
        second_positional: None,
        props: &[
            prop("query", "string", ""),
            prop("ignore_case", "boolean", ""),
            prop("in", "string", ""),
            prop("limit", "integer", ""),
        ],
    },
    Tool {
        name: "skeleton",
        help: "List file/directory signatures without bodies.",
        positional: Some("target"),
        second_positional: None,
        props: &[
            prop("target", "string", "Node id or relative file path."),
        ],
    },
    Tool {
        name: "path",
        help: "Find a shortest undirected graph path (up to 8 hops).",
        positional: Some("from"),
        second_positional: Some("to"),
        props: &[
            prop("from", "string", ""),
            prop("to", "string", ""),
            prop("meta", "boolean", ""),
        ],
    },
    Tool {
        name: "trace",
        help: "Trace shortest directed call/reference paths between names or ids.",
        positional: Some("from"),
        second_positional: Some("to"),
        props: &[
            prop("from", "string", ""),
            prop("to", "string", ""),
            prop("max_depth", "integer", "Hops, 1..16."),
            prop("max_paths", "integer", "Paths, 1..100."),
            prop("max_bytes", "integer", "Output bytes, 256..1048576."),
            prop("also", "array", "Read-only roots (up to 16)."),
        ],
    },
    Tool {
        name: "impact",
        help: "Find direct reference sites and transitive callers by name or id.",
        positional: Some("symbol"),
        second_positional: None,
        props: &[
            prop("symbol", "string", ""),
            prop("max_depth", "integer", "Caller depth, 1..16."),
            prop("max_sites", "integer", "Sites/callers, 1..10000."),
            prop("max_bytes", "integer", "Output bytes, 256..1048576."),
            prop("also", "array", "Read-only roots (up to 16)."),
        ],
    },
    Tool {
        name: "export",
        help: "Export the whole graph as JSON or Mermaid (unbounded).",
        positional: None,
        second_positional: None,
        props: &[
            prop("format", "string", "json or mermaid."),
        ],
    },
    Tool {
        name: "pack",
        help: "Get task context: definitions, callers/imports and source in one bounded call.",
        positional: Some("task"),
        second_positional: None,
        props: &[
            prop("task", "string", ""),
            prop("budget", "integer", "Tokens, 100..100000."),
        ],
    },
];

/// Tool availability and description style, fixed for the lifetime of a server.
#[derive(Debug, Default)]
pub(super) enum Surface {
    /// All tools, with compact descriptions and complete schemas.
    #[default]
    Full,
    /// A non-empty subset, exposed in canonical table order.
    Selected(Vec<&'static str>),
}

impl Surface {
    pub(super) fn profile(name: &str) -> Result<Self, String> {
        match name {
            "default" => Ok(Self::Full),
            "lean" => Ok(Self::Selected(vec!["find", "read"])),
            "three" => Ok(Self::Selected(vec!["map", "find", "read"])),
            _ => Err(format!(
                "unknown MCP profile: {name}; expected default, lean or three"
            )),
        }
    }

    pub(super) fn from_names(names: &str) -> Result<Self, String> {
        let mut selected = Vec::new();
        for name in names.split(',') {
            let tool = TOOLS
                .iter()
                .find(|tool| tool.name == name)
                .ok_or_else(|| format!("unknown MCP tool: {name:?}"))?;
            if selected.contains(&tool.name) {
                return Err(format!("duplicate MCP tool: {name}"));
            }
            selected.push(tool.name);
        }
        Ok(Self::Selected(selected))
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        match self {
            Self::Full => TOOLS.iter().any(|tool| tool.name == name),
            Self::Selected(names) => names.contains(&name),
        }
    }
}

/// Every property `tool` accepts: its own, then the common ones not already in its own set.
fn accepted(tool: &Tool) -> Vec<&Prop> {
    let own_names: std::collections::HashSet<&str> = tool.props.iter().map(|p| p.name).collect();
    tool.props
        .iter()
        .chain(COMMON.iter().filter(|extra| {
            !(own_names.contains(extra.name)
                || matches!(tool.name, "trace" | "impact" | "pack") && extra.name != "json"
                || tool.name == "hubs" && extra.name == "cursor"
                || tool.name == "export" && extra.name != "budget"
                || tool.name == "skeleton" && extra.name != "budget" && extra.name != "json")
        }))
        .collect()
}

/// MCP-specific bounds; CLI users retain their existing defaults.
fn default(tool: &str, property: &str) -> Option<u32> {
    match (tool, property) {
        ("map", "limit") => Some(20),
        ("hubs", "limit") => Some(10),
        ("trace" | "impact", "max_bytes") => Some(8192),
        ("trace", "max_paths") => Some(5),
        ("impact", "max_sites") => Some(50),
        ("pack", "budget") => Some(2000),
        _ => None,
    }
}

fn schema_of(tool: &Tool) -> Json {
    let properties = accepted(tool)
        .into_iter()
        .map(|p| {
            let mut members = vec![("type", Json::str(p.ty))];
            if !p.help.is_empty() {
                members.push(("description", Json::str(p.help)));
            }
            if let Some(value) = default(tool.name, p.name).or(match (tool.name, p.name) {
                ("find" | "grep", "limit") => Some(10),
                ("neighbors", "limit") => Some(20),
                ("trace" | "impact", "max_depth") => Some(8),
                _ => None,
            }) {
                members.push(("default", Json::Num(value.to_string())));
            }
            if p.name == "json" {
                members.push(("default", Json::Bool(false)));
            }
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
pub(super) fn list(surface: &Surface) -> Json {
    let tools = TOOLS
        .iter()
        .filter(|tool| surface.contains(tool.name))
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

/// Turns a call into CLI-shaped arguments, or a message naming the bad argument.
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
    let named_context = name == "read"
        && members
            .iter()
            .find(|(key, _)| key == "context")
            .is_some_and(|(_, value)| matches!(value, Json::Bool(true)));
    let mut out = vec![if named_context { "context" } else { name }.to_string()];
    // Positionals are literal values: they go after a `--` terminator, never where a flag could be read.
    let mut positionals = Vec::new();
    for slot in [tool.positional, tool.second_positional]
        .into_iter()
        .flatten()
    {
        let value = members
            .iter()
            .find(|(key, _)| key == slot)
            .map(|(_, value)| value);
        match value.and_then(Json::as_str) {
            Some(text) if !text.is_empty() => positionals.push(text.to_string()),
            _ => return Err(format!("{slot} is required and must be a non-empty string")),
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
        if matches!(value, Json::Null) || named_context && p.name == "context" {
            continue;
        }
        // MCP stays pinned to the server's startup root and never writes files.
        if matches!(p.name, "place" | "output") {
            return Err(format!("{} is not accepted over MCP", p.name));
        }
        let flag = format!("--{}", p.name.replace('_', "-"));
        match (p.ty, value) {
            ("boolean", Json::Bool(true)) => out.push(flag),
            ("boolean", Json::Bool(false)) => {}
            ("integer", Json::Num(text) | Json::Str(text))
            | ("string" | "array", Json::Str(text)) => out.extend([flag, text.clone()]),
            ("array", Json::Arr(items)) => {
                let parts: Option<Vec<&str>> = items.iter().map(Json::as_str).collect();
                if p.name == "also" {
                    for part in parts.ok_or("also must be an array of strings")? {
                        out.extend([flag.clone(), part.to_string()]);
                    }
                    continue;
                }
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
    // Defaults are shared by the schema and execution; explicit values always win.
    for p in &known {
        if !members
            .iter()
            .any(|(key, value)| key == p.name && !matches!(value, Json::Null))
        {
            if let Some(value) = default(name, p.name) {
                out.extend([format!("--{}", p.name.replace('_', "-")), value.to_string()]);
            }
        }
    }
    if name == "pack" {
        out.push("--for".into());
        out.extend(positionals);
    } else if !positionals.is_empty() {
        // Ids, queries and paths are literal values, including when they start with '-'.
        // Keep all options before the terminator so no positional value is reinterpreted as a flag.
        out.push("--".to_string());
        out.extend(positionals);
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
            ["find", "--kind", "fn,type", "--in", "src", "--json", "--", "beta"]
        );
        assert_eq!(
            args("neighbors", r#"{"id":"s:a#b","depth":3,"dir":"in"}"#).unwrap(),
            ["neighbors", "--dir", "in", "--depth", "3", "--", "s:a#b"]
        );
        assert_eq!(
            args("grep", r#"{"query":"x","ignore_case":true}"#).unwrap(),
            ["grep", "--ignore-case", "--", "x"]
        );
        assert_eq!(args("hubs", "{}").unwrap(), ["hubs", "--limit", "10"]);
        assert_eq!(
            args("read", r#"{"id":"s:a#b","lines":"3-9"}"#).unwrap(),
            ["read", "--lines", "3-9", "--", "s:a#b"]
        );
    }

    #[test]
    fn context_arguments_preserve_flag_shaped_values() {
        assert_eq!(
            args(
                "read",
                r#"{"id":"--help","context":true,"session":"--context","dedupe":true,"json":true}"#
            )
            .unwrap(),
            [
                "context",
                "--session",
                "--context",
                "--dedupe",
                "--json",
                "--",
                "--help"
            ]
        );
    }

    #[test]
    fn flag_shaped_positionals_follow_the_terminator() {
        assert_eq!(
            args("read", r#"{"id":"--place","lines":"s.py"}"#).unwrap(),
            ["read", "--lines", "s.py", "--", "--place"]
        );
        assert_eq!(
            args("grep", r#"{"query":"--place","in":"--lines"}"#).unwrap(),
            ["grep", "--in", "--lines", "--", "--place"]
        );
        assert_eq!(
            args("path", r#"{"from":"--place","to":"--output"}"#).unwrap(),
            ["path", "--", "--place", "--output"]
        );
        for tool in TOOLS {
            assert!(
                accepted(tool)
                    .iter()
                    .all(|p| p.name != "place" && p.name != "output"),
                "{}",
                tool.name
            );
        }
        assert!(args("find", r#"{"query":"a","place":"/"}"#).is_err());
        assert!(args("export", r#"{"format":"json","output":"x"}"#).is_err());
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
        let surface = Surface::Full;
        assert!(list(&surface).get("tools").is_some());
        assert!(surface.contains("map") && surface.contains("path"));
    }

    #[test]
    fn new_tools_become_cli_arguments() {
        assert_eq!(
            args("skeleton", r#"{"target":"m:a.py"}"#).unwrap(),
            ["skeleton", "--", "m:a.py"]
        );
        assert_eq!(
            args("skeleton", r#"{"target":"m:a.py","json":true}"#).unwrap(),
            ["skeleton", "--json", "--", "m:a.py"]
        );
        assert_eq!(
            args("path", r#"{"from":"s:a.py#a","to":"s:a.py#b"}"#).unwrap(),
            ["path", "--", "s:a.py#a", "s:a.py#b"]
        );
        assert_eq!(
            args(
                "path",
                r#"{"from":"s:a.py#a","to":"s:a.py#b","json":true,"meta":true}"#
            )
            .unwrap(),
            ["path", "--meta", "--json", "--", "s:a.py#a", "s:a.py#b"]
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
    fn mcp_defaults_apply_only_to_omitted_or_null_values() {
        assert_eq!(
            args("map", r#"{"target":"a.rs"}"#).unwrap(),
            ["map", "--limit", "20", "--", "a.rs"]
        );
        assert_eq!(
            args("map", r#"{"target":"a.rs","limit":null}"#).unwrap(),
            ["map", "--limit", "20", "--", "a.rs"]
        );
        assert_eq!(
            args("map", r#"{"target":"a.rs","limit":60}"#).unwrap(),
            ["map", "--limit", "60", "--", "a.rs"]
        );
        assert_eq!(
            args("pack", r#"{"task":"--place"}"#).unwrap(),
            ["pack", "--budget", "2000", "--for", "--place"]
        );
        assert_eq!(
            args("pack", r#"{"task":"beacon","budget":3000,"json":true}"#).unwrap(),
            ["pack", "--budget", "3000", "--json", "--for", "beacon"]
        );
        assert!(args("pack", r#"{"task":"beacon","cursor":"x"}"#).is_err());
        assert!(args("pack", r#"{"task":"beacon","place":"elsewhere"}"#).is_err());
        assert!(args("pack", r#"{"task":"beacon","output":"dump"}"#).is_err());
    }

    #[test]
    fn all_tools_are_registered() {
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
                "trace",
                "impact",
                "export",
                "pack"
            ]
        );
    }

    #[test]
    fn selections_keep_complete_schemas_and_compact_descriptions() {
        let Json::Arr(full) = list(&Surface::Full).get("tools").unwrap().clone() else {
            panic!("tools array");
        };
        for (surface, expected) in [
            (
                Surface::profile("default").unwrap(),
                TOOLS.iter().map(|t| t.name).collect::<Vec<_>>(),
            ),
            (Surface::profile("lean").unwrap(), vec!["find", "read"]),
            (
                Surface::profile("three").unwrap(),
                vec!["map", "find", "read"],
            ),
            (
                Surface::from_names("read,grep,find").unwrap(),
                vec!["find", "read", "grep"],
            ),
            (Surface::from_names("export").unwrap(), vec!["export"]),
        ] {
            let listed = list(&surface);
            let Some(Json::Arr(selected)) = listed.get("tools") else {
                panic!("tools array");
            };
            let names: Vec<_> = selected
                .iter()
                .map(|t| t.get("name").unwrap().as_str().unwrap())
                .collect();
            assert_eq!(names, expected);
            for tool in selected {
                let original = full
                    .iter()
                    .find(|t| t.get("name") == tool.get("name"))
                    .unwrap();
                assert_eq!(tool.get("inputSchema"), original.get("inputSchema"));
                assert_eq!(tool, original);
            }
        }
    }

    #[test]
    fn every_tool_can_be_selected_but_invalid_selections_fail() {
        for tool in TOOLS {
            let surface = Surface::from_names(tool.name).unwrap();
            for other in TOOLS {
                assert_eq!(surface.contains(other.name), other.name == tool.name);
            }
        }
        for names in [
            "",
            "nope",
            "find,nope",
            "find,",
            ",read",
            "find,,read",
            "find,find",
            "Find",
            "find, read",
        ] {
            assert!(Surface::from_names(names).is_err(), "{names}");
        }
        assert!(Surface::profile("nope").is_err());
    }
}
