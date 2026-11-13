// SPDX-License-Identifier: Apache-2.0
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
    /// Ruby source (`.rb`), tasks (`.rake`), gem specifications (`.gemspec`),
    /// and the basenames `Rakefile` and `Gemfile`.
    Ruby,
    /// PHP source (`.php`), templates (`.phtml`) and includes (`.inc`).
    Php,
    /// Swift source (`.swift`).
    Swift,
    /// Scala source (`.scala`) and scripts (`.sc`). Heuristic symbols live in the facade.
    Scala,
    /// Dart source.
    Dart,
    /// Zig source.
    Zig,
    /// Solidity source (`.sol`).
    Solidity,
    /// Gleam source.
    Gleam,
    /// Perl source (`.pl`, `.pm`, `.t`, `.psgi`).
    Perl,
    /// Groovy source (`.groovy`) and Gradle build scripts (`.gradle`).
    Groovy,
    /// PowerShell scripts and modules (`.ps1`, `.psm1`, `.psd1`).
    Powershell,
    /// Fortran source (`.f`, `.for`, `.f90`, `.f95`, `.f03`, `.f08`; any case).
    Fortran,
    /// Verilog/SystemVerilog source (`.v`, `.vh`, `.sv`, `.svh`).
    Verilog,
    /// Plain-text documents (`.txt`, `.rst`, `.adoc`, `.asciidoc`, `.org`, `.tex`). Headings live in the facade.
    Text,
    /// Elixir source (`.ex`, `.exs`). Heuristic symbols live in the facade.
    Elixir,
    /// Lua source.
    Lua,
    /// OCaml source (`.ml`) and interfaces (`.mli`).
    OCaml,
    /// R source (`.r`, `.R`).
    R,
    /// Julia source.
    Julia,
    /// Haskell source.
    Haskell,
    /// Terraform/HCL (`.tf`, `.tfvars`, `.hcl`). Heuristic symbols live in the facade.
    Hcl,
    /// SQL (`.sql`).
    Sql,
    /// Shell scripts (`.sh`, `.bash`, `.zsh`).
    Shell,
    /// Makefiles (`.mk`, `Makefile`, `GNUmakefile`).
    Make,
    /// Dockerfiles (`Dockerfile`, `Containerfile`).
    Docker,
    /// Protocol Buffers (`.proto`).
    Proto,
    /// Pascal and Delphi (`.pas`, `.pp`, `.dpr`).
    Pascal,
    /// GraphQL schemas (`.graphql`, `.gql`).
    Graphql,
    /// Vue, Svelte, Astro and HTML (`.vue`, `.svelte`, `.astro`, `.html`, `.htm`). Script and heading symbols live in the facade.
    Web,
    /// Lisp family: Clojure (`.clj`, `.cljs`, `.cljc`, `.edn`), Common Lisp (`.lisp`, `.lsp`, `.cl`), Scheme (`.scm`, `.ss`), Emacs Lisp (`.el`).
    Lisp,
    /// Nix expressions (`.nix`).
    Nix,
    /// Erlang source (`.erl` and `.hrl`).
    Erlang,
    /// No grammar; only a module node is emitted.
    Unknown,
}

/// Single source for extension dispatch.
///
/// `dctx` does not depend on this crate, so `crates/dctx/src/index/extract.rs`
/// duplicates [`EXTENSIONS`] and [`RUBY_BASENAMES`]. The copies are locked to
/// the same literal list in this crate's `tests/lexers_extensions.rs` and in
/// `crates/dctx/tests/map_languages.rs` (plus the facade unit test).
///
/// Extensions are lowercase. Matching folds the final segment's extension.
/// `tsx` is its own grammar here; the facade tags that extension `typescript`.
const EXTENSIONS: &[(&str, Language)] = &[
    ("rs", Language::Rust),
    ("js", Language::JavaScript),
    ("mjs", Language::JavaScript),
    ("cjs", Language::JavaScript),
    ("jsx", Language::JavaScript),
    ("ts", Language::TypeScript),
    ("mts", Language::TypeScript),
    ("cts", Language::TypeScript),
    ("tsx", Language::Tsx),
    ("md", Language::Markdown),
    ("markdown", Language::Markdown),
    ("py", Language::Python),
    ("pyi", Language::Python),
    ("go", Language::Go),
    ("java", Language::Java),
    ("c", Language::C),
    ("h", Language::C),
    ("cc", Language::C),
    ("cpp", Language::C),
    ("cxx", Language::C),
    ("hpp", Language::C),
    ("hh", Language::C),
    ("json", Language::Config),
    ("jsonc", Language::Config),
    ("toml", Language::Config),
    ("yaml", Language::Config),
    ("yml", Language::Config),
    ("cs", Language::CSharp),
    ("kt", Language::Kotlin),
    ("kts", Language::Kotlin),
    ("rb", Language::Ruby),
    ("rake", Language::Ruby),
    ("gemspec", Language::Ruby),
    ("php", Language::Php),
    ("phtml", Language::Php),
    ("inc", Language::Php),
    ("swift", Language::Swift),
    ("scala", Language::Scala),
    ("sc", Language::Scala),
    ("dart", Language::Dart),
    ("zig", Language::Zig),
    ("sol", Language::Solidity),
    ("gleam", Language::Gleam),
    ("f90", Language::Fortran),
    ("f95", Language::Fortran),
    ("f03", Language::Fortran),
    ("f08", Language::Fortran),
    ("f", Language::Fortran),
    ("for", Language::Fortran),
    ("pl", Language::Perl),
    ("pm", Language::Perl),
    ("t", Language::Perl),
    ("psgi", Language::Perl),
    ("erl", Language::Erlang),
    ("hrl", Language::Erlang),
    ("groovy", Language::Groovy),
    ("gradle", Language::Groovy),
    ("ps1", Language::Powershell),
    ("psm1", Language::Powershell),
    ("psd1", Language::Powershell),
    ("v", Language::Verilog),
    ("vh", Language::Verilog),
    ("sv", Language::Verilog),
    ("svh", Language::Verilog),
    ("txt", Language::Text),
    ("rst", Language::Text),
    ("adoc", Language::Text),
    ("asciidoc", Language::Text),
    ("org", Language::Text),
    ("tex", Language::Text),
    ("ex", Language::Elixir),
    ("exs", Language::Elixir),
    ("lua", Language::Lua),
    ("ml", Language::OCaml),
    ("mli", Language::OCaml),
    ("r", Language::R),
    ("jl", Language::Julia),
    ("hs", Language::Haskell),
    ("tf", Language::Hcl),
    ("tfvars", Language::Hcl),
    ("hcl", Language::Hcl),
    ("sql", Language::Sql),
    ("sh", Language::Shell),
    ("bash", Language::Shell),
    ("zsh", Language::Shell),
    ("bats", Language::Shell),
    ("mk", Language::Make),
    ("proto", Language::Proto),
    ("pas", Language::Pascal),
    ("pp", Language::Pascal),
    ("dpr", Language::Pascal),
    ("graphql", Language::Graphql),
    ("gql", Language::Graphql),
    ("vue", Language::Web),
    ("svelte", Language::Web),
    ("astro", Language::Web),
    ("html", Language::Web),
    ("htm", Language::Web),
    ("clj", Language::Lisp),
    ("cljs", Language::Lisp),
    ("cljc", Language::Lisp),
    ("edn", Language::Lisp),
    ("lisp", Language::Lisp),
    ("lsp", Language::Lisp),
    ("cl", Language::Lisp),
    ("scm", Language::Lisp),
    ("ss", Language::Lisp),
    ("el", Language::Lisp),
    ("nix", Language::Nix),
];

/// Ruby basenames accepted by `dctx-lex-rb` (`Dialect::for_path`).
///
/// Compared case-insensitively on the final path segment, before the extension.
/// `Gemfile.lock` stays unknown. `Guardfile` and `config.ru` are not in the
/// lexer, so they are not in this list: classifying them would refuse the file.
const RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

/// Extensionless basenames of infra formats, compared case-insensitively on the final path segment.
/// Duplicated in `dctx` `index/extract.rs` and locked by `tests/lexers_extensions.rs`.
const INFRA_BASENAMES: &[(&str, Language)] = &[
    ("Makefile", Language::Make),
    ("GNUmakefile", Language::Make),
    ("Dockerfile", Language::Docker),
    ("Containerfile", Language::Docker),
];

impl Language {
    /// Selects a grammar from the final path segment.
    ///
    /// Ruby basenames `Rakefile` and `Gemfile` match case-insensitively and win
    /// over the extension.
    /// Otherwise the text after the last `.` is folded and looked up in
    /// the supported extension table. A segment with no `.` is [`Language::Unknown`].
    /// This classifies spelling only: it does not validate paths, access files
    /// or impose a path-length limit. [`crate::Parser::parse`] validates paths.
    #[allow(
        clippy::must_use_candidate,
        reason = "Preserve the existing public API's caller lint behavior; adding must_use would change that contract."
    )]
    pub fn for_path(path: &str) -> Self {
        let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if RUBY_BASENAMES
            .iter()
            .any(|name| base.eq_ignore_ascii_case(name))
        {
            return Self::Ruby;
        }
        if let Some((_, language)) = INFRA_BASENAMES
            .iter()
            .find(|(name, _)| base.eq_ignore_ascii_case(name))
        {
            return *language;
        }
        // `pgtap.sql.in`: a SQL template that a build step turns into `.sql`. Duplicated in `dctx` `index/extract.rs`.
        if base.len() > ".sql.in".len()
            && base
                .get(base.len() - ".sql.in".len()..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(".sql.in"))
        {
            return Self::Sql;
        }
        let Some(extension) = base
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
        else {
            return Self::Unknown;
        };
        EXTENSIONS
            .iter()
            .find(|(name, _)| *name == extension)
            .map_or(Self::Unknown, |(_, language)| *language)
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
    /// Supplied bytes are not UTF-8; decode or replace them before retrying.
    #[error(
        "invalid UTF-8: {0}; decode the file as UTF-8 or replace invalid bytes before retrying"
    )]
    InvalidUtf8(#[from] std::str::Utf8Error),
    /// Source or fact counts exceed the parser coordinate space.
    #[error(
        "file exceeds parser byte or fact limits; split the file (maximum 8 MiB, or 32 MiB for Markdown) or reduce its item count before retrying"
    )]
    TooLarge,
    /// Parser setup failed: an incompatible grammar or unavailable worker runtime.
    #[error(
        "parser unavailable: {0}; use PureParser or install a compatible parser runtime and retry"
    )]
    Grammar(String),
    /// Optional parser did not produce a tree.
    #[error("tree-sitter did not produce a syntax tree; retry with PureParser or reduce the input")]
    NoTree,
    /// A parser identity is not a valid evidence label.
    #[error(transparent)]
    Label(#[from] dctx_map_model::InvalidLabel),
    /// Source syntax contains an error or missing token.
    #[error(
        "invalid syntax in {path} at line {line}; correct the syntax or reduce unsupported nesting, then retry"
    )]
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
        for extension in ["php", "phtml", "inc", "PHP", "PHTML", "INC"] {
            assert_eq!(
                Language::for_path(&format!("src/app.{extension}")),
                Language::Php
            );
        }
        assert_eq!(Language::for_path("pkg/app.pyw"), Language::Unknown);
        assert_eq!(Language::for_path("Sources/App.swift"), Language::Swift);
        assert_eq!(Language::for_path("Sources/App.SWIFT"), Language::Swift);
        assert_eq!(Language::for_path("rs/file.txt"), Language::Text);
        assert_eq!(Language::for_path("rs/file.log"), Language::Unknown);
    }

    #[test]
    fn dispatch_table_matches_the_shared_literal() {
        // Same list as `extract.rs`, `tests/lexers_extensions.rs` and
        // `crates/dctx/tests/map_languages.rs`. `tsx` maps to `Tsx` here and to
        // the facade tag `typescript`.
        const LITERAL_EXTENSIONS: &[&str] = &[
            "rs", "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "md", "markdown", "py",
            "pyi", "go", "java", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "json", "jsonc",
            "toml", "yaml", "yml", "cs", "kt", "kts", "rb", "rake", "gemspec", "php", "phtml",
            "inc", "swift", "scala", "sc", "dart", "zig", "sol", "gleam", "f90", "f95", "f03",
            "f08", "f", "for", "pl", "pm", "t", "psgi", "erl", "hrl", "groovy", "gradle", "ps1",
            "psm1", "psd1", "v", "vh", "sv", "svh", "txt", "rst", "adoc", "asciidoc", "org", "tex",
            "ex", "exs", "lua", "ml", "mli", "r", "jl", "hs", "tf", "tfvars", "hcl", "sql", "sh",
            "bash", "zsh", "bats", "mk", "proto", "pas", "pp", "dpr", "graphql", "gql", "vue",
            "svelte", "astro", "html", "htm", "clj", "cljs", "cljc", "edn", "lisp", "lsp", "cl",
            "scm", "ss", "el", "nix",
        ];
        const LITERAL_RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];
        let extensions: Vec<&str> = EXTENSIONS.iter().map(|(extension, _)| *extension).collect();
        assert_eq!(extensions, LITERAL_EXTENSIONS);
        assert_eq!(RUBY_BASENAMES, LITERAL_RUBY_BASENAMES);
        assert_eq!(Language::for_path("sql/pgtap.sql.in"), Language::Sql);
        assert_eq!(Language::for_path("Makefile.in"), Language::Unknown);
        for (extension, language) in EXTENSIONS {
            assert_eq!(
                Language::for_path(&format!("src/app.{extension}")),
                *language
            );
            assert_eq!(
                Language::for_path(&format!("dir.dot/app.{}", extension.to_ascii_uppercase())),
                *language
            );
        }
        for basename in RUBY_BASENAMES {
            assert_eq!(Language::for_path(basename), Language::Ruby);
            assert_eq!(
                Language::for_path(&format!("lib/{}", basename.to_ascii_lowercase())),
                Language::Ruby
            );
            assert_eq!(
                Language::for_path(&format!("lib\\{basename}")),
                Language::Ruby
            );
        }
        // A dot-free segment is not an extension. This matches `language_of`.
        assert_eq!(Language::for_path("php"), Language::Unknown);
        assert_eq!(Language::for_path("swift"), Language::Unknown);
        assert_eq!(Language::for_path("inc"), Language::Unknown);
        assert_eq!(Language::for_path("Gemfile.lock"), Language::Unknown);
        // Not a defect: dctx-lex-rb rejects these, so they stay unknown modules.
        assert_eq!(Language::for_path("Guardfile"), Language::Unknown);
        assert_eq!(Language::for_path("config.ru"), Language::Unknown);
        assert_eq!(Language::for_path("lib/CONFIG.RU"), Language::Unknown);
    }
}
