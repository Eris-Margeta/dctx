// SPDX-License-Identifier: Apache-2.0
//! Spelling helpers for reference resolution: call and import segments, and module keys of a path.
//! Implements the name side of SPEC-MAP-MODE 6.3 for every first-party lexer's spelling (`a.b`, `a::b`, `a->b`, `./a/b`).
//! Pure string functions; the resolver in `refs` owns the tables.

use std::borrow::Cow;

/// Implicit JS/TS source extensions, in lookup order.
pub(super) const JS_SOURCE_EXTENSIONS: &[&str] =
    &["js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts"];

/// Source extensions stripped from path-shaped imports in other languages.
const OTHER_SOURCE_EXTENSIONS: &[&str] = &[
    "py", "pyi", "rs", "go", "java", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "kt", "kts",
    "rb", "php", "swift",
];

/// Leading words of a call spelling that name no segment.
const CALL_NOISE: &[&str] = &[
    "new", "await", "async", "self", "this", "Self", "super", "crate", "static", "parent",
];

/// Leading words of an import spelling that name no segment.
const IMPORT_NOISE: &[&str] = &[
    "static", "crate", "self", "super", "use", "import", "include", "require", "from",
];

/// Words of `name`, split on everything that cannot be part of an identifier.
/// For PowerShell, `-` is allowed in identifiers (e.g., `Get-Config`).
fn words(name: &str, language: Option<&str>) -> Vec<String> {
    let js = matches!(language, Some("javascript" | "typescript" | "web"));
    let is_powershell = language == Some("powershell");
    name.split(|c: char| {
        !(c.is_alphanumeric() || c == '_' || (is_powershell && c == '-') || (js && c == '$'))
    })
    .filter(|word| !word.is_empty())
    .map(str::to_string)
    .collect()
}

/// Segments of a call spelling without receiver noise: `self.helper` is `[helper]`, `a::b.c` is `[a, b, c]`.
/// For PowerShell, `-` is not a segment separator (e.g., `Read-File` stays as one segment).
pub(super) fn call_segments(name: &str, language: Option<&str>) -> Vec<String> {
    let mut all = words(name, language);
    let skip = all
        .iter()
        .take_while(|word| CALL_NOISE.contains(&word.as_str()))
        .count();
    all.drain(..skip);
    all
}

/// The module path and optional lexer-supplied binding of a relative JS import.
pub(super) fn relative_import(name: &str) -> Option<(&str, Option<&str>)> {
    let (module, member) = js_import_parts(name);
    if !module.starts_with("./") && !module.starts_with("../") {
        return None;
    }
    Some((module, member))
}

/// The JS lexer spells a selected member as `module::member`.
fn js_import_parts(name: &str) -> (&str, Option<&str>) {
    dctx_lex_js::RawReference::module_parts(name)
}

/// Local name evidence in an import fact, independent of its target.
pub(super) enum ImportBinding<'a> {
    None,
    Named(&'a str),
    /// The producer uses the import path for its implicit local names.
    Path,
}

pub(super) fn import_binding<'a>(
    family: &str,
    name: &'a str,
    alias: Option<&'a str>,
) -> ImportBinding<'a> {
    if let Some(local) = alias {
        return ImportBinding::Named(local);
    }
    if family == "javascript" {
        // The lexer appends a member for unaliased named imports. A module alone
        // (side-effect import or unassigned require) introduces no local name.
        let (_, member) = js_import_parts(name);
        return member.map_or(ImportBinding::None, ImportBinding::Named);
    }
    ImportBinding::Path
}

/// Segments of an import spelling. A trailing source extension, quotes, relative dots and `*` are dropped.
pub(super) fn import_segments(name: &str) -> Vec<String> {
    let (module, binding) = relative_import(name).unwrap_or((name, None));
    let mut segments = module_segments(module);
    segments.extend(binding.map(str::to_owned));
    segments
}

/// Splits a module spelling using the shared suffix resolver's rules.
pub(super) fn module_segments(name: &str) -> Vec<String> {
    let mut spelled = name.trim();
    if let Some((stem, extension)) = spelled.rsplit_once('.')
        && (JS_SOURCE_EXTENSIONS.contains(&extension)
            || OTHER_SOURCE_EXTENSIONS.contains(&extension))
        && (spelled.contains('/')
            || spelled.starts_with('.')
            || matches!(extension, "h" | "hpp" | "hh"))
    {
        spelled = stem;
    }
    let mut all = words(spelled, None);
    let skip = all
        .iter()
        .take_while(|word| IMPORT_NOISE.contains(&word.as_str()))
        .count();
    all.drain(..skip);
    all
}

/// Last identifier part of a stored node name (`Box::run` is `run`).
pub(super) fn last_segment(name: &str) -> &str {
    name.rsplit([':', '.']).next().unwrap_or(name)
}

/// Keys under which the module at `path` can be imported: every suffix of its path segments joined with `::`.
///
/// The flag is true for the key that spells the whole path. `pkg/__init__.py`, `a/index.js` and `a/mod.rs` stand for `pkg`, `a`, `a`.
pub(super) fn module_keys(path: &str) -> Vec<(String, bool)> {
    let stem = match path.rsplit_once('.') {
        Some((stem, extension)) if !extension.contains('/') => stem,
        _ => path,
    };
    let mut segments: Vec<&str> = stem.split('/').filter(|part| !part.is_empty()).collect();
    if segments.len() > 1
        && segments
            .last()
            .is_some_and(|last| matches!(*last, "__init__" | "index" | "mod" | "lib" | "main"))
    {
        segments.pop();
    }
    (0..segments.len())
        .map(|start| (segments[start..].join("::"), start == 0))
        .collect()
}

/// Join a literal relative operand within the project inventory.
pub(super) fn inventory_join(directory: &str, relative: &str) -> Option<String> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\0') {
        return None;
    }
    let mut parts: Vec<_> = directory.split('/').filter(|s| !s.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Try the joined path as spelled, with source extensions, then as a directory index.
/// A specifier ending in a slash, `.` or `..` names a directory only.
pub(super) fn js_relative_candidates<'a>(
    base: &'a str,
    specifier: &str,
) -> impl Iterator<Item = Cow<'a, str>> {
    let file = (!matches!(specifier.rsplit('/').next(), Some("" | "." | ".."))).then_some(base);
    let extensions = file.into_iter().flat_map(|stem| {
        JS_SOURCE_EXTENSIONS
            .iter()
            .map(move |ext| Cow::Owned(format!("{stem}.{ext}")))
    });
    let separator = if base.is_empty() { "" } else { "/" };
    let indexes = JS_SOURCE_EXTENSIONS
        .iter()
        .map(move |ext| Cow::Owned(format!("{base}{separator}index.{ext}")));
    file.into_iter()
        .map(Cow::Borrowed)
        .chain(extensions)
        .chain(indexes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_of_every_lexer_reduce_to_segments() {
        assert_eq!(call_segments("self.helper", None), ["helper"]);
        assert_eq!(call_segments("new Point", None), ["Point"]);
        assert_eq!(call_segments("$this->save", None), ["save"]);
        assert_eq!(call_segments("a::b.c", None), ["a", "b", "c"]);
        // PowerShell: hyphenated names stay as one segment.
        assert_eq!(
            call_segments("Read-File", Some("powershell")),
            ["Read-File"]
        );
        assert_eq!(
            call_segments("Get-Config", Some("powershell")),
            ["Get-Config"]
        );
        assert_eq!(
            call_segments("Get-ChildItem", Some("powershell")),
            ["Get-ChildItem"]
        );
        // Other languages still split on hyphen.
        assert_eq!(call_segments("Read-File", Some("rust")), ["Read", "File"]);
    }

    #[test]
    fn import_segments_work() {
        assert_eq!(import_segments("./util.js"), ["util"]);
        assert_eq!(import_segments("./util.js::helper"), ["util", "helper"]);
        assert_eq!(import_segments("a::beta"), ["a", "beta"]);
        assert_eq!(
            import_segments("static java.util.Objects.*"),
            ["java", "util", "Objects"]
        );
        assert_eq!(import_segments("crate::a::B"), ["a", "B"]);
        assert_eq!(import_segments("stdio.h"), ["stdio"]);
    }

    #[test]
    fn relative_import_preserves_literal_module_text() {
        let path = "./a\n\x1b\"/x.js";
        assert_eq!(
            relative_import(&format!("{path}::f")),
            Some((path, Some("f")))
        );
    }

    #[test]
    fn relative_js_candidates_preserve_file_and_directory_order() {
        let specifier = ".././x\n\x1b\"";
        let base = inventory_join("p/sub", specifier).expect("path within inventory");
        let candidates: Vec<_> = js_relative_candidates(&base, specifier).collect();
        assert_eq!(
            candidates,
            [
                "p/x\n\x1b\"",
                "p/x\n\x1b\".js",
                "p/x\n\x1b\".jsx",
                "p/x\n\x1b\".mjs",
                "p/x\n\x1b\".cjs",
                "p/x\n\x1b\".ts",
                "p/x\n\x1b\".tsx",
                "p/x\n\x1b\".mts",
                "p/x\n\x1b\".cts",
                "p/x\n\x1b\"/index.js",
                "p/x\n\x1b\"/index.jsx",
                "p/x\n\x1b\"/index.mjs",
                "p/x\n\x1b\"/index.cjs",
                "p/x\n\x1b\"/index.ts",
                "p/x\n\x1b\"/index.tsx",
                "p/x\n\x1b\"/index.mts",
                "p/x\n\x1b\"/index.cts",
            ]
        );
        for specifier in ["../", "../.", "./sub/.."] {
            let base = inventory_join("p", specifier).expect("path within inventory");
            let first = if base.is_empty() {
                "index.js"
            } else {
                "p/index.js"
            };
            assert_eq!(
                js_relative_candidates(&base, specifier).next().as_deref(),
                Some(first)
            );
        }
        assert_eq!(inventory_join("p", "../../x"), None);
    }

    #[test]
    fn a_side_effect_specifier_introduces_no_local_name() {
        let path = "./a\n\x1b\"/util.mjs";
        assert!(
            matches!(
                import_binding("javascript", path, None),
                ImportBinding::None
            ),
            "{path:?}"
        );
    }

    #[test]
    fn last_segment_works() {
        assert_eq!(last_segment("Box::Nested::ping"), "ping");
    }

    #[test]
    fn module_keys_cover_every_suffix_and_index_files() {
        let keys = module_keys("src/pkg/a.py");
        assert_eq!(
            keys,
            [
                ("src::pkg::a".to_string(), true),
                ("pkg::a".to_string(), false),
                ("a".to_string(), false)
            ]
        );
        assert_eq!(module_keys("pkg/__init__.py"), [("pkg".to_string(), true)]);
        assert_eq!(module_keys("a.py"), [("a".to_string(), true)]);
    }
}
