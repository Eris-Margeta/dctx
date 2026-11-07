// SPDX-License-Identifier: Apache-2.0
//! Record order for `map` of one file: what a reader of the outline needs first survives a page cut.
//! Tier 0 holds the file's exported top-level declarations, tier 1 every other declaration, tier 2 impl blocks,
//! constants, variables and tests. Each tier is in source order, so a page is the file's public surface plus the
//! head of its outline. Other map roots keep the id order. Visibility is read from the stored signature and name.

use super::IndexedNode;
use super::extract::language_of;

/// Kinds that an outline lists last: they name no callable or type of their own.
const LOW_KINDS: [&str; 10] = [
    "impl", "const", "static", "var", "let", "field", "property", "variable", "import", "use",
];

/// Outline tier of a child of a file root found at BFS `level` (1 = directly in the file).
pub(super) fn tier(node: &IndexedNode, path: &str, level: u32) -> u8 {
    if node.letter == b't' || LOW_KINDS.contains(&node.kind.as_str()) {
        return 2;
    }
    if level == 1 && exported(language_of(path), leaf(&node.name), &node.signature) {
        return 0;
    }
    1
}

/// The `signature` stored on the module node of `path` (DCX-55c): the file's explicit export list from
/// [`super::export_list::names`], space-separated, or empty when it has none. Computed from the bytes the walk already
/// read, so `map m:<file>` orders by it without rereading the file.
pub(super) fn export_signature(path: &str, bytes: &[u8]) -> String {
    let language = language_of(path);
    if !matches!(language, "python" | "javascript" | "typescript") {
        return String::new();
    }
    let Some(names) = super::export_list::names(language, &String::from_utf8_lossy(bytes)) else {
        return String::new();
    };
    names
        .into_iter()
        .filter(|name| !name.is_empty() && !name.contains(char::is_whitespace))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Last segment of a qualified name: `Type::method` is `method`.
fn leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

/// True when the declaration is visible outside its file by the language's own convention.
/// Rust needs `pub`, JavaScript and TypeScript `export`, Go a capital; Python and the rest a name without a leading `_`,
/// and the JVM, .NET and Swift families no `private`, `protected` or `internal` in the signature.
fn exported(language: &str, leaf: &str, signature: &str) -> bool {
    let signature = signature.trim_start();
    match language {
        "rust" => signature.starts_with("pub"),
        "javascript" | "typescript" => signature.starts_with("export"),
        "go" => leaf.chars().next().is_some_and(char::is_uppercase),
        "java" | "csharp" | "kotlin" | "swift" | "scala" | "dart" | "php" | "groovy" => {
            !leaf.starts_with('_')
                && !["private", "protected", "internal", "fileprivate"]
                    .iter()
                    .any(|word| signature.split_whitespace().any(|token| token == *word))
        }
        _ => !leaf.starts_with('_'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::TextSpan;

    fn node(kind: &str, name: &str, signature: &str) -> IndexedNode {
        IndexedNode {
            letter: b's',
            ordinal: 0,
            kind: kind.to_string(),
            name: name.to_string(),
            path: TextSpan::EMPTY,
            lines: [1, 1],
            span: [0, 0],
            summary: String::new(),
            summary_source: "sig".to_string(),
            signature: signature.to_string(),
        }
    }

    #[test]
    fn exported_top_level_declarations_come_first_and_impls_and_constants_last() {
        assert_eq!(
            tier(
                &node("fn", "convert::str", "pub(super) fn str(v: &OsStr)"),
                "defs.rs",
                1
            ),
            0
        );
        assert_eq!(tier(&node("fn", "helper", "fn helper()"), "defs.rs", 1), 1);
        assert_eq!(
            tier(&node("impl", "Flag::impl", "impl Flag for X"), "defs.rs", 1),
            2
        );
        assert_eq!(
            tier(
                &node("const", "LIMIT", "pub const LIMIT: u8 = 1;"),
                "defs.rs",
                1
            ),
            2
        );
        assert_eq!(
            tier(
                &node("method", "Graph::add_node", "def add_node(self, key)"),
                "graph.py",
                2
            ),
            1
        );
        assert_eq!(
            tier(&node("class", "Node", "class Node:"), "graph.py", 1),
            0
        );
        assert_eq!(
            tier(
                &node("fn", "_cargo_tree", "def _cargo_tree(path)"),
                "iso.py",
                1
            ),
            1
        );
        assert_eq!(
            tier(
                &node("fn", "writeNote", "export function writeNote("),
                "notes.ts",
                1
            ),
            0
        );
        assert_eq!(
            tier(&node("fn", "slug", "function slug("), "notes.ts", 1),
            1
        );
        assert_eq!(tier(&node("fn", "Open", "func Open("), "file.go", 1), 0);
        assert_eq!(tier(&node("fn", "open", "func open("), "file.go", 1), 1);
        assert_eq!(
            tier(&node("method", "run", "private void run()"), "A.java", 1),
            1
        );
        let mut test = node("fn", "it_works", "fn it_works()");
        test.letter = b't';
        assert_eq!(tier(&test, "lib.rs", 1), 2);
    }

    #[test]
    fn export_signature_holds_the_explicit_list_of_export_languages_only() {
        assert_eq!(
            export_signature("pkg/mod.py", b"__all__ = ['beta', \"Gamma\"]\n"),
            "Gamma beta"
        );
        assert_eq!(
            export_signature("stubs/mod.PYI", b"__all__ = ['beta']\n"),
            "beta"
        );
        assert_eq!(
            export_signature("lib.cjs", b"module.exports = { run };\n"),
            "run"
        );
        assert_eq!(
            export_signature("src/api.tsx", b"export { a, b as c };\n"),
            "a b"
        );
        assert_eq!(
            export_signature("pkg/plain.py", b"def alpha():\n    pass\n"),
            ""
        );
        assert_eq!(export_signature("src/main.rs", b"__all__ = ['beta']\n"), "");
        assert_eq!(export_signature("Makefile", b"__all__ = ['beta']\n"), "");
    }
}
