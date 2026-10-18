//! Public per-file parse records and explicit failure states.
//! Implements SPEC-MAP-MODE sections 5.3 and 6.2–6.3.
//! Uses model facts; a resolver consumes references before storage.
use dctx_map_model::{Edge, EdgeKind, Node, StableId};

/// Grammar used for this file, or the unsupported-language fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Rust source.
    Rust,
    /// JavaScript, including JSX.
    JavaScript,
    /// TypeScript source.
    TypeScript,
    /// TypeScript with JSX syntax.
    Tsx,
    /// Markdown block and inline syntax.
    Markdown,
    /// Python source (`.py`) and stubs (`.pyi`).
    Python,
    /// Go source.
    Go,
    /// Java source.
    Java,
    /// C and C++ source and headers.
    C,
    /// JSON, JSONC, TOML and YAML configuration.
    Config,
    /// C# source.
    CSharp,
    /// Kotlin source (`.kt`) and scripts (`.kts`).
    Kotlin,
    /// Ruby source (`.rb`), tasks (`.rake`) and gem specifications (`.gemspec`).
    Ruby,
    /// No grammar; only a module node is emitted.
    Unknown,
}

impl Language {
    /// Selects a grammar using the final extension, case-insensitively.
    pub fn for_path(path: &str) -> Self {
        match path
            .rsplit('.')
            .next()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("rs") => Self::Rust,
            Some("js" | "mjs" | "cjs" | "jsx") => Self::JavaScript,
            Some("ts" | "mts" | "cts") => Self::TypeScript,
            Some("tsx") => Self::Tsx,
            Some("md" | "markdown") => Self::Markdown,
            Some("py" | "pyi") => Self::Python,
            Some("go") => Self::Go,
            Some("java") => Self::Java,
            Some("c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh") => Self::C,
            Some("json" | "jsonc" | "toml" | "yaml" | "yml") => Self::Config,
            Some("cs") => Self::CSharp,
            Some("kt" | "kts") => Self::Kotlin,
            Some("rb" | "rake" | "gemspec") => Self::Ruby,
            _ => Self::Unknown,
        }
    }
}

/// A syntactic reference whose destination has not been resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReference {
    /// Source node, including its duplicate ordinal when needed.
    pub src: StableId,
    /// Relation to resolve; never `contains`.
    pub kind: EdgeKind,
    /// Referenced spelling (qualified path, receiver.method, or doc target).
    pub name: String,
    /// Lexical scope qualified with `::`; empty means the file root.
    pub scope: String,
    /// Explicit local binding from `use path as alias`, if any.
    pub alias: Option<String>,
    /// One-based inclusive lines of the reference site.
    pub lines: [u32; 2],
    /// File whose replacement invalidates this reference.
    pub owner_path: String,
}

/// Deterministic facts for one authorized file, in source traversal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    /// Validated, place-relative file path.
    pub path: String,
    /// Selected parser language.
    pub language: Language,
    /// Module, symbols, tests, and document sections.
    pub nodes: Vec<Node>,
    /// Exact containment relations owned by this file.
    pub edges: Vec<Edge>,
    /// Unresolved references; repeated sites remain separate records.
    pub references: Vec<RawReference>,
    /// Coverage, completion and parser identity for this file.
    /// Edge confidence stays on each edge; this label is not confidence.
    pub precision: dctx_map_model::Precision,
}

/// Parser errors never produce a partial replacement of stored facts.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The path or a generated item name cannot form a model identity.
    #[error(transparent)]
    InvalidId(#[from] dctx_map_model::InvalidId),
    /// Supplied bytes are not UTF-8.
    #[error(transparent)]
    InvalidUtf8(#[from] std::str::Utf8Error),
    /// Source or fact counts exceed the parser coordinate space.
    #[error("file exceeds parser coordinate limits")]
    TooLarge,
    /// Optional grammar is incompatible with its runtime.
    #[error("incompatible grammar: {0}")]
    Grammar(String),
    /// Optional parser did not produce a tree.
    #[error("tree-sitter did not produce a syntax tree")]
    NoTree,
    /// A parser identity is not a valid evidence label.
    #[error(transparent)]
    Label(#[from] dctx_map_model::InvalidLabel),
    /// Source syntax contains an error or missing token.
    #[error("invalid syntax in {path} at line {line}")]
    Syntax {
        /// File containing invalid syntax.
        path: String,
        /// One-based line of the first syntax error.
        line: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_dispatch_is_extension_based() {
        assert_eq!(Language::for_path("src/lib.rs"), Language::Rust);
        assert_eq!(Language::for_path("docs/README.MD"), Language::Markdown);
        assert_eq!(Language::for_path("docs/spec.markdown"), Language::Markdown);
        assert_eq!(Language::for_path("pkg/app.py"), Language::Python);
        assert_eq!(Language::for_path("pkg/app.PYI"), Language::Python);
        assert_eq!(Language::for_path("cmd/main.go"), Language::Go);
        assert_eq!(Language::for_path("src/App.JAVA"), Language::Java);
        for extension in ["c", "h", "cc", "cpp", "cxx", "hpp", "hh"] {
            assert_eq!(
                Language::for_path(&format!("src/app.{extension}")),
                Language::C
            );
        }
        for extension in ["json", "jsonc", "toml", "yaml", "yml"] {
            assert_eq!(
                Language::for_path(&format!("cfg/app.{extension}")),
                Language::Config
            );
        }
        assert_eq!(Language::for_path("src/App.CS"), Language::CSharp);
        assert_eq!(Language::for_path("src/App.kt"), Language::Kotlin);
        assert_eq!(Language::for_path("scripts/main.KTS"), Language::Kotlin);
        for extension in ["rb", "rake", "gemspec", "RB", "RAKE", "GEMSPEC"] {
            assert_eq!(
                Language::for_path(&format!("src/app.{extension}")),
                Language::Ruby
            );
        }
        assert_eq!(Language::for_path("pkg/app.pyw"), Language::Unknown);
        assert_eq!(Language::for_path("rs/file.txt"), Language::Unknown);
    }
}
