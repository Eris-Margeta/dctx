// SPDX-License-Identifier: Apache-2.0
//! Spelling helpers for reference resolution: call and import segments, and module keys of a path.
//! Implements the name side of SPEC-MAP-MODE 6.3 for every first-party lexer's spelling (`a.b`, `a::b`, `a->b`, `./a/b`).
//! Pure string functions; the resolver in `refs` owns the tables.

/// Extensions removed from a path-shaped import such as `./util.js` or `lib/x.rb`.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "py", "pyi", "rs", "go", "java", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs",
    "kt", "kts", "rb", "php", "swift",
];

/// Leading words of a call spelling that name no segment.
const CALL_NOISE: &[&str] = &["new", "await", "async", "self", "this", "Self", "super", "crate", "static", "parent"];

/// Leading words of an import spelling that name no segment.
const IMPORT_NOISE: &[&str] = &["static", "crate", "self", "super", "use", "import", "include", "require", "from"];

/// Words of `name`, split on everything that cannot be part of an identifier.
/// For PowerShell, `-` is allowed in identifiers (e.g., `Get-Config`).
fn words(name: &str, language: Option<&str>) -> Vec<String> {
    let is_powershell = language == Some("powershell");
    name.split(|c: char| !(c.is_alphanumeric() || c == '_' || (is_powershell && c == '-')))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Segments of a call spelling without receiver noise: `self.helper` is `[helper]`, `a::b.c` is `[a, b, c]`.
/// For PowerShell, `-` is not a segment separator (e.g., `Read-File` stays as one segment).
pub(super) fn call_segments(name: &str, language: Option<&str>) -> Vec<String> {
    let mut all = words(name, language);
    let skip = all.iter().take_while(|word| CALL_NOISE.contains(&word.as_str())).count();
    all.drain(..skip);
    all
}

/// Segments of an import spelling. A trailing source extension, quotes, relative dots and `*` are dropped.
pub(super) fn import_segments(name: &str) -> Vec<String> {
    let mut spelled = name.trim();
    if let Some((stem, extension)) = spelled.rsplit_once('.')
        && SOURCE_EXTENSIONS.contains(&extension)
        && (spelled.contains('/') || spelled.starts_with('.') || matches!(extension, "h" | "hpp" | "hh"))
    {
        spelled = stem;
    }
    let mut all = words(spelled, None);
    let skip = all.iter().take_while(|word| IMPORT_NOISE.contains(&word.as_str())).count();
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
    if segments.len() > 1 && segments.last().is_some_and(|last| matches!(*last, "__init__" | "index" | "mod" | "lib" | "main")) {
        segments.pop();
    }
    (0..segments.len()).map(|start| (segments[start..].join("::"), start == 0)).collect()
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
        assert_eq!(call_segments("Read-File", Some("powershell")), ["Read-File"]);
        assert_eq!(call_segments("Get-Config", Some("powershell")), ["Get-Config"]);
        assert_eq!(call_segments("Get-ChildItem", Some("powershell")), ["Get-ChildItem"]);
        // Other languages still split on hyphen.
        assert_eq!(call_segments("Read-File", Some("rust")), ["Read", "File"]);
    }

    #[test]
    fn import_segments_work() {
        assert_eq!(import_segments("./util.js"), ["util"]);
        assert_eq!(import_segments("a::beta"), ["a", "beta"]);
        assert_eq!(import_segments("static java.util.Objects.*"), ["java", "util", "Objects"]);
        assert_eq!(import_segments("crate::a::B"), ["a", "B"]);
        assert_eq!(import_segments("stdio.h"), ["stdio"]);
    }

    #[test]
    fn last_segment_works() {
        assert_eq!(last_segment("Box::Nested::ping"), "ping");
    }

    #[test]
    fn module_keys_cover_every_suffix_and_index_files() {
        let keys = module_keys("src/pkg/a.py");
        assert_eq!(keys, [("src::pkg::a".to_string(), true), ("pkg::a".to_string(), false), ("a".to_string(), false)]);
        assert_eq!(module_keys("pkg/__init__.py"), [("pkg".to_string(), true)]);
        assert_eq!(module_keys("a.py"), [("a".to_string(), true)]);
    }
}
