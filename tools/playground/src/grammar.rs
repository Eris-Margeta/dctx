// SPDX-License-Identifier: Apache-2.0
//! The whitelisted command grammar. A request never reaches a shell: argv is checked here,
//! token by token, against the flags `dctx map --help` and `dctx --help` list for each verb.

/// The ten map verbs.
pub const VERBS: [&str; 10] = ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"];

/// Limits on uploaded files.
pub const MAX_FILES: usize = 48;
pub const MAX_FILE_BYTES: usize = 96 * 1024;
pub const MAX_TOTAL_BYTES: usize = 384 * 1024;
const MAX_ARGS: usize = 24;
const MAX_ARG_BYTES: usize = 400;

#[derive(Clone, Copy)]
enum Value {
    /// A flag without a value.
    None,
    /// An integer in the inclusive range.
    Int(u32, u32),
    /// One of the listed words.
    OneOf(&'static [&'static str]),
    /// A comma-separated subset of the listed words.
    ListOf(&'static [&'static str]),
    /// Lower-case words separated by commas.
    Words,
    /// A relative path inside the place.
    Path,
    /// `A-B` line numbers.
    Lines,
    /// An opaque cursor from an earlier page.
    Cursor,
    /// `on` or `off`, written `--flag=on`.
    Switch,
}

const COMMON_MAP: [(&str, Value); 3] = [("--json", Value::None), ("--meta", Value::None), ("--help", Value::None)];
const KINDS: &[&str] = &[
    "fn", "method", "struct", "enum", "trait", "impl", "const", "type", "macro", "class", "interface", "symbol",
    "module", "package", "place", "doc", "test",
];

/// (flag, value) pairs each verb accepts, besides `COMMON_MAP`.
fn verb_flags(verb: &str) -> &'static [(&'static str, Value)] {
    match verb {
        "map" => &[("--depth", Value::Int(1, 3)), ("--limit", Value::Int(1, 60)), ("--cursor", Value::Cursor), ("--budget", Value::Int(1, 2000))],
        "find" => &[
            ("--kind", Value::ListOf(KINDS)),
            ("--in", Value::Path),
            ("--limit", Value::Int(1, 40)),
            ("--cursor", Value::Cursor),
            ("--budget", Value::Int(1, 2000)),
        ],
        "neighbors" => &[
            ("--dir", Value::OneOf(&["in", "out", "both"])),
            ("--edge", Value::ListOf(&["calls", "imports"])),
            ("--depth", Value::Int(1, 4)),
            ("--limit", Value::Int(1, 60)),
            ("--cursor", Value::Cursor),
            ("--budget", Value::Int(1, 2000)),
        ],
        "explain" => &[("--budget", Value::Int(1, 1000))],
        "read" => &[("--lines", Value::Lines), ("--full", Value::None), ("--budget", Value::Int(1, 2000)), ("--cursor", Value::Cursor)],
        "skeleton" => &[("--budget", Value::Int(1, 2000))],
        "grep" => &[
            ("-i", Value::None),
            ("--ignore-case", Value::None),
            ("--in", Value::Path),
            ("--limit", Value::Int(1, 60)),
            ("--cursor", Value::Cursor),
            ("--budget", Value::Int(1, 2000)),
        ],
        "hubs" => &[("--limit", Value::Int(1, 60)), ("--budget", Value::Int(1, 2000))],
        "path" => &[("--budget", Value::Int(1, 2000))],
        "export" => &[("--format", Value::OneOf(&["json", "mermaid"]))],
        _ => &[],
    }
}

/// How many positionals each verb takes, and of which kind.
fn verb_positionals(verb: &str) -> &'static [Positional] {
    match verb {
        "map" | "neighbors" | "explain" | "read" => &[Positional::Target],
        "skeleton" => &[Positional::File],
        "find" | "grep" => &[Positional::Text],
        "path" => &[Positional::Target, Positional::Target],
        _ => &[],
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Positional {
    /// A stable id (`s:…`, `m:…`) or a place-relative file.
    Target,
    /// A place-relative file or `m:` id.
    File,
    /// Free text: a query or a literal.
    Text,
}

const PACK_FLAGS: &[(&str, Value)] = &[
    ("--json", Value::None),
    ("--binary", Value::None),
    ("--empty", Value::None),
    ("--non-empty", Value::None),
    ("--hidden", Value::None),
    ("--debug-ignored", Value::None),
    ("--ext", Value::Words),
    ("--include", Value::Path),
    ("--exclude", Value::Path),
    ("--large", Value::Int(0, 64)),
    ("--small", Value::Int(1, 64)),
    ("--tree-stamps", Value::Switch),
    ("--for", Value::None), // value handled below: free text
    ("--budget", Value::Int(100, 2000)),
    ("--help", Value::None),
];

/// Flags refused everywhere, with the reason sent back.
const REFUSED: &[(&str, &str)] = &[
    ("--place", "the place is the sample or your files"),
    ("--output", "the playground writes nothing"),
    ("-o", "the playground writes nothing"),
    ("--clipboard", "there is no clipboard on the server"),
    ("-c", "there is no clipboard on the server"),
    ("--rev", "this build does not link git"),
    ("--profile", "timings are not part of an answer"),
    ("--offset", "use --cursor"),
    ("-d", "the legacy snapshot flags are not offered"),
    ("-t", "the legacy snapshot flags are not offered"),
    ("-r", "the legacy snapshot flags are not offered"),
];

/// Validates argv (without the leading `dctx`). Returns the reason on refusal.
pub fn check(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    if argv.len() > MAX_ARGS {
        return Err(format!("at most {MAX_ARGS} arguments"));
    }
    for arg in argv {
        if arg.len() > MAX_ARG_BYTES {
            return Err(format!("an argument is longer than {MAX_ARG_BYTES} bytes"));
        }
        if arg.chars().any(|c| c == '\0' || c == '\n' || c == '\r') {
            return Err("arguments may not contain NUL or line breaks".into());
        }
        if let Some((_, why)) = REFUSED.iter().find(|(flag, _)| arg == flag || arg.starts_with(&format!("{flag}="))) {
            return Err(format!("{arg} is not allowed: {why}"));
        }
    }
    match argv[0].as_str() {
        "map" => check_map(&argv[1..]),
        "pack" => check_pack(&argv[1..]),
        "languages" => match &argv[1..] {
            [] => Ok(()),
            [flag] if flag == "--json" || flag == "--markdown" || flag == "--help" => Ok(()),
            _ => Err("languages takes --json, --markdown or nothing".into()),
        },
        "--help" | "--version" if argv.len() == 1 => Ok(()),
        other if ["cache", "template", "plugin", "git"].contains(&other) => {
            Err(format!("{other} is not offered in the playground: it manages the local machine, not a place"))
        }
        "." => Err("the legacy snapshot `dctx .` is not offered; use pack".into()),
        other => Err(format!("unknown command {other:?}: use pack, map or languages")),
    }
}

fn check_map(args: &[String]) -> Result<(), String> {
    let verb = args.first().ok_or("map needs a verb: map, find, neighbors, explain, read, skeleton, grep, hubs, path or export")?;
    if verb == "--help" && args.len() == 1 {
        return Ok(());
    }
    if !VERBS.contains(&verb.as_str()) {
        return Err(format!("unknown map verb {verb:?}"));
    }
    let flags = verb_flags(verb);
    let wanted = verb_positionals(verb);
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--help" && args.len() == 2 {
            return Ok(());
        }
        if arg.starts_with('-') && arg.len() > 1 {
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
                _ => (arg.as_str(), None),
            };
            let spec = COMMON_MAP.iter().chain(flags.iter()).find(|(flag, _)| *flag == name);
            let (_, kind) = spec.ok_or_else(|| format!("{name} is not an option of map {verb}"))?;
            if matches!(kind, Value::None) {
                if inline.is_some() {
                    return Err(format!("{name} takes no value"));
                }
            } else {
                let value = match inline {
                    Some(value) => value,
                    None => {
                        index += 1;
                        args.get(index).cloned().ok_or_else(|| format!("{name} needs a value"))?
                    }
                };
                check_value(name, *kind, &value)?;
            }
        } else {
            positionals.push(arg.clone());
        }
        index += 1;
    }
    if positionals.len() != wanted.len() {
        return Err(match wanted.len() {
            0 => format!("map {verb} takes no target"),
            1 => format!("map {verb} takes one target"),
            n => format!("map {verb} takes {n} targets"),
        });
    }
    for (value, kind) in positionals.iter().zip(wanted) {
        match kind {
            Positional::Target => check_target(value)?,
            Positional::File => {
                if let Some(rest) = value.strip_prefix("m:") {
                    check_path(rest)?;
                } else {
                    check_path(value)?;
                }
            }
            Positional::Text => {
                // An empty query is dctx's own usage error (exit 2); the playground lets it answer.
                if value.len() > 200 {
                    return Err("the query is at most 200 bytes".into());
                }
            }
        }
    }
    Ok(())
}

fn check_pack(args: &[String]) -> Result<(), String> {
    let mut index = 0;
    let mut has_for = false;
    let mut has_budget = false;
    let mut has_path = false;
    while index < args.len() {
        let arg = &args[index];
        if !arg.starts_with('-') {
            if arg != "." || has_path {
                return Err("pack takes `.` as its only path: the place is the sample or your files".into());
            }
            has_path = true;
            index += 1;
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg.as_str(), None),
        };
        let (_, kind) = PACK_FLAGS.iter().find(|(flag, _)| *flag == name).ok_or_else(|| format!("{name} is not an option of pack"))?;
        if name == "--for" {
            let value = match inline {
                Some(value) => value,
                None => {
                    index += 1;
                    args.get(index).cloned().ok_or("--for needs a text")?
                }
            };
            if value.trim().is_empty() || value.len() > 200 {
                return Err("--for needs 1 to 200 bytes of text".into());
            }
            has_for = true;
        } else if matches!(kind, Value::None) {
            if inline.is_some() {
                return Err(format!("{name} takes no value"));
            }
        } else if matches!(kind, Value::Switch) {
            let value = inline.ok_or_else(|| format!("write {name}=on or {name}=off"))?;
            if value != "on" && value != "off" {
                return Err(format!("{name} takes on or off"));
            }
        } else {
            let value = match inline {
                Some(value) => value,
                None => {
                    index += 1;
                    args.get(index).cloned().ok_or_else(|| format!("{name} needs a value"))?
                }
            };
            check_value(name, *kind, &value)?;
            has_budget |= name == "--budget";
        }
        index += 1;
    }
    if has_budget && !has_for {
        return Err("--budget belongs to pack --for".into());
    }
    Ok(())
}

fn check_value(name: &str, kind: Value, value: &str) -> Result<(), String> {
    match kind {
        Value::None | Value::Switch => Ok(()),
        Value::Int(low, high) => match value.parse::<u32>() {
            Ok(n) if (low..=high).contains(&n) && !value.starts_with('+') => Ok(()),
            _ => Err(format!("{name} takes a number from {low} to {high}")),
        },
        Value::OneOf(words) => {
            if words.contains(&value) {
                Ok(())
            } else {
                Err(format!("{name} takes one of {}", words.join(", ")))
            }
        }
        Value::ListOf(words) => {
            if !value.is_empty() && value.split(',').all(|part| words.contains(&part)) {
                Ok(())
            } else {
                Err(format!("{name} takes a comma-separated list of {}", words.join(", ")))
            }
        }
        Value::Words => {
            if !value.is_empty() && value.len() <= 120 && value.split(',').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric())) {
                Ok(())
            } else {
                Err(format!("{name} takes comma-separated extensions, for example rs,toml"))
            }
        }
        Value::Path => check_path(value).map_err(|why| format!("{name}: {why}")),
        Value::Lines => {
            let ok = value.split_once('-').is_some_and(|(a, b)| {
                matches!((a.parse::<u32>(), b.parse::<u32>()), (Ok(x), Ok(y)) if x >= 1 && y >= x && y <= 100_000)
            });
            if ok {
                Ok(())
            } else {
                Err(format!("{name} takes A-B line numbers"))
            }
        }
        Value::Cursor => {
            if !value.is_empty() && value.len() <= 400 && value.bytes().all(|b| b.is_ascii_graphic()) {
                Ok(())
            } else {
                Err(format!("{name} takes the next value of an earlier page"))
            }
        }
    }
}

/// A stable id (`s:`, `m:`, `d:`, `k:`, `p:` then 1..200 bytes) or a place-relative file.
pub fn check_target(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() >= 3 && b"smdkp".contains(&bytes[0]) && bytes[1] == b':' {
        if value.len() > 202 {
            return Err("an id is at most 200 bytes after its prefix".into());
        }
        if value[2..].contains("..") {
            return Err("an id may not contain ..".into());
        }
        return Ok(());
    }
    check_path(value)
}

/// A relative path inside the place: no `..`, no leading `/`, a safe character set.
pub fn check_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 120 {
        return Err("a path is 1 to 120 bytes".into());
    }
    if value.starts_with('/') || value.starts_with('-') {
        return Err("a path must be relative".into());
    }
    if !value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/-".contains(&b)) {
        return Err("a path may use only A-Z a-z 0-9 . _ / -".into());
    }
    if value.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return Err("a path may not contain empty, . or .. parts".into());
    }
    Ok(())
}

/// One uploaded file.
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub path: String,
    pub content: String,
}

/// Validates uploaded files: count, sizes, paths, duplicates and prefixes.
pub fn check_files(files: &[File]) -> Result<(), String> {
    if files.is_empty() {
        return Err("send at least one file".into());
    }
    if files.len() > MAX_FILES {
        return Err(format!("at most {MAX_FILES} files"));
    }
    let mut total = 0;
    let mut seen: Vec<&str> = Vec::new();
    for file in files {
        check_path(&file.path).map_err(|why| format!("{}: {why}", file.path))?;
        if file.content.len() > MAX_FILE_BYTES {
            return Err(format!("{}: a file is at most {} KiB", file.path, MAX_FILE_BYTES / 1024));
        }
        if file.content.contains('\0') {
            return Err(format!("{}: text files only", file.path));
        }
        total += file.content.len();
        for other in &seen {
            if *other == file.path || other.starts_with(&format!("{}/", file.path)) || file.path.starts_with(&format!("{other}/")) {
                return Err(format!("{}: duplicate or nested path", file.path));
            }
        }
        seen.push(&file.path);
    }
    if total > MAX_TOTAL_BYTES {
        return Err(format!("all files together are at most {} KiB", MAX_TOTAL_BYTES / 1024));
    }
    Ok(())
}

/// Splits a command line the way a POSIX shell would for words and quotes, without expansion.
/// A leading `dctx` is dropped, so both `map find greet` and `dctx map find greet` work.
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) => word.push(inner),
                        None => return Err("unclosed ' quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\')) => word.push(e),
                            Some(other) => {
                                word.push('\\');
                                word.push(other);
                            }
                            None => return Err("unclosed \" quote".into()),
                        },
                        Some(inner) => word.push(inner),
                        None => return Err("unclosed \" quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                word.push(chars.next().ok_or("a trailing backslash")?);
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '|' | ';' | '&' | '<' | '>' | '`' | '$' => return Err(format!("{c} is shell syntax; the playground runs one dctx command")),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    if words.first().map(String::as_str) == Some("dctx") {
        words.remove(0);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(line: &str) -> Vec<String> {
        split(line).unwrap()
    }

    #[test]
    fn accepts_every_captured_example() {
        for line in [
            "pack .",
            "pack . --tree-stamps=on",
            "pack . --ext rs",
            "pack . --for 'greet store' --budget 600",
            "map map src/util.rs --depth 2",
            "map find greet",
            "map find greet --json",
            "map neighbors 's:src/util.rs#greet' --edge calls --dir in",
            "map neighbors 's:src/util.rs#greet' --edge calls --dir in --json",
            "map explain 's:src/util.rs#greet'",
            "map read 's:src/util.rs#greet' --json",
            "map skeleton src/store.rs",
            "map grep example -i",
            "map hubs --limit 10",
            "map path 's:src/main.rs#main' 's:src/store.rs#save'",
            "map export --format mermaid",
            "map export --format json",
            "map find greet --json --meta",
            "map read m:nope.rs",
            "map find ''",
            "languages --markdown",
            "languages",
            "--help",
            "map --help",
            "map find --help",
            "dctx map find \"ignore rules\" --kind fn,doc --limit 5",
            "map read s:src/store.rs#save --lines 11-13",
            "map neighbors s:src/util.rs#greet --dir=out --depth=4",
        ] {
            assert_eq!(check(&argv(line)), Ok(()), "{line}");
        }
    }

    #[test]
    fn refuses_the_rejection_table() {
        let table = [
            "",
            "cache status",
            "cache purge --all --yes",
            "template build x.dctx_template",
            "plugin list",
            "git status",
            ".",
            ". -d -c",
            "rm -rf /",
            "pack /etc",
            "pack ../",
            "pack . --output out.txt",
            "pack . --clipboard",
            "pack . -c",
            "pack . --budget 600",
            "pack . --for greet --budget 50",
            "pack . --tree-stamps",
            "pack . --tree-stamps=maybe",
            "pack . --ext '../x'",
            "map",
            "map nope x",
            "map find",
            "map find greet --place /",
            "map find greet --limit 41",
            "map find greet --limit 0",
            "map find greet --kind banana",
            "map find greet --in ../secret",
            "map neighbors s:a --depth 5",
            "map neighbors s:a --dir sideways",
            "map neighbors s:a --edge contains",
            "map map src/util.rs --depth 4",
            "map read /etc/passwd",
            "map read m:../outside.rs",
            "map read s:../../x",
            "map read s:a --rev HEAD",
            "map read s:a --lines 9-3",
            "map explain s:a --budget 1001",
            "map hubs --limit 61",
            "map hubs extra",
            "map path s:a",
            "map export --format svg",
            "map export --output graph.mmd",
            "map skeleton src/a.rs --profile",
            "map grep x --offset 5",
            "languages --yaml",
            "--version --verbose",
        ];
        assert!(table.len() >= 25);
        for line in table {
            assert!(check(&argv(line)).is_err(), "{line:?} was accepted");
        }
    }

    #[test]
    fn refuses_nul_newline_and_long_arguments() {
        assert!(check(&["map".into(), "find".into(), "a\nb".into()]).is_err());
        assert!(check(&["map".into(), "find".into(), "a\0b".into()]).is_err());
        assert!(check(&["map".into(), "find".into(), "x".repeat(401)]).is_err());
        assert!(check(&vec!["languages".to_string(); 25]).is_err());
    }

    #[test]
    fn splits_like_a_shell_without_expansion() {
        assert_eq!(argv("dctx map find 'a b' \"c \\\"d\\\"\" e\\ f"), vec!["map", "find", "a b", "c \"d\"", "e f"]);
        assert!(split("map find 'open").is_err());
        assert!(split("pack . | head").is_err());
        assert!(split("map find $(id)").is_err());
        assert!(split("pack . > out").is_err());
    }

    #[test]
    fn paths_stay_inside_the_place() {
        for good in ["src/a.rs", "a", "web/app.ts", ".env", "a-b_c.d/e"] {
            assert_eq!(check_path(good), Ok(()), "{good}");
        }
        for bad in ["", "/abs", "../x", "a/../b", "a//b", "a/./b", "a b", "a\\b", "-x", &"a".repeat(121), "é.rs"] {
            assert!(check_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn file_limits() {
        let file = |path: &str, size: usize| File { path: path.into(), content: "x".repeat(size) };
        assert_eq!(check_files(&[file("src/a.rs", 10)]), Ok(()));
        assert!(check_files(&[]).is_err());
        assert!(check_files(&[file("a", MAX_FILE_BYTES + 1)]).is_err());
        assert!(check_files(&(0..49).map(|i| file(&format!("f{i}"), 1)).collect::<Vec<_>>()).is_err());
        assert!(check_files(&(0..5).map(|i| file(&format!("f{i}"), 90 * 1024)).collect::<Vec<_>>()).is_err());
        assert!(check_files(&[file("a", 1), file("a", 1)]).is_err());
        assert!(check_files(&[file("a", 1), file("a/b", 1)]).is_err());
        assert!(check_files(&[File { path: "a".into(), content: "x\0".into() }]).is_err());
        assert!(check_files(&[file("../a", 1)]).is_err());
    }
}
