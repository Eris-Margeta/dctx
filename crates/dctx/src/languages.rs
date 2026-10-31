//! Language registry: every language the structural index recognises, with its tag, file names, tier and confidence.
//! The single source for `dctx languages`, the README language table and [`crate::Capabilities::languages`].
//! Tags are the tags of the index's extension dispatch; `index/languages_tests.rs` locks this table to that dispatch,
//! to the heuristic families (confidence `guess`) and to the document formats (tier `docs`).

/// What the index promises for the files of one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Symbols and call edges. Call edges are lexical, not type-resolved.
    Full,
    /// Symbols only. No call edges are promised.
    Outline,
    /// Headed sections of a document.
    Docs,
}

impl Tier {
    /// Every tier, in display order.
    pub const ALL: [Tier; 3] = [Tier::Full, Tier::Outline, Tier::Docs];

    /// Wire name: `full`, `outline` or `docs`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::Full => "full",
            Tier::Outline => "outline",
            Tier::Docs => "docs",
        }
    }

    /// One-line meaning of the tier.
    #[must_use]
    pub const fn meaning(self) -> &'static str {
        match self {
            Tier::Full => "symbols and call edges; call edges are lexical, not type-resolved",
            Tier::Outline => "symbols only; no call edges are promised",
            Tier::Docs => "headed sections",
        }
    }
}

/// How the symbols of one language are found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A first-party lexer or the deterministic document rules.
    Structural,
    /// A heuristic outline family. Every record of these files carries confidence `guess`.
    Guess,
}

impl Confidence {
    /// Wire name: `structural` or `guess`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Confidence::Structural => "structural",
            Confidence::Guess => "guess",
        }
    }
}

/// One supported language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language {
    /// Display name.
    pub name: &'static str,
    /// Language tag of the extension dispatch.
    pub tag: &'static str,
    /// Lowercase file extensions without the dot. Matching is case-insensitive.
    pub extensions: &'static [&'static str],
    /// Whole file names (case-insensitive) or `*`-prefixed suffixes that select the language without an extension entry.
    pub files: &'static [&'static str],
    /// What the index promises.
    pub tier: Tier,
    /// How symbols are found.
    pub confidence: Confidence,
}

/// Defines [`LANGUAGES`] and [`TAGS`] from one list, so the tag list can never drift from the entries.
macro_rules! registry {
    ($( $name:literal, $tag:literal, [$($ext:literal),*], [$($file:literal),*], $tier:ident, $confidence:ident; )*) => {
        /// Every supported language, in display order: full, then outline, then docs.
        pub const LANGUAGES: &[Language] = &[$(Language {
            name: $name,
            tag: $tag,
            extensions: &[$($ext),*],
            files: &[$($file),*],
            tier: Tier::$tier,
            confidence: Confidence::$confidence,
        }),*];
        /// The tag of every entry of [`LANGUAGES`], in the same order.
        pub const TAGS: &[&str] = &[$($tag),*];
    };
}

registry! {
    "Rust", "rust", ["rs"], [], Full, Structural;
    "JavaScript", "javascript", ["js", "mjs", "cjs", "jsx"], [], Full, Structural;
    "TypeScript/TSX", "typescript", ["ts", "mts", "cts", "tsx"], [], Full, Structural;
    "Python", "python", ["py", "pyi"], [], Full, Structural;
    "Go", "go", ["go"], [], Full, Structural;
    "Java", "java", ["java"], [], Full, Structural;
    "C/C++", "c", ["c", "h", "cc", "cpp", "cxx", "hpp", "hh"], [], Full, Structural;
    "C#", "csharp", ["cs"], [], Full, Structural;
    "Kotlin", "kotlin", ["kt", "kts"], [], Full, Structural;
    "Ruby", "ruby", ["rb", "rake", "gemspec"], ["Rakefile", "Gemfile"], Full, Structural;
    "PHP", "php", ["php", "phtml", "inc"], [], Full, Structural;
    "Swift", "swift", ["swift"], [], Full, Structural;
    "Vue/Svelte/Astro/HTML script blocks", "web", ["vue", "svelte", "astro", "html", "htm"], [], Full, Structural;
    "Scala", "scala", ["scala", "sc"], [], Full, Guess;
    "Dart", "dart", ["dart"], [], Full, Guess;
    "Zig", "zig", ["zig"], [], Full, Guess;
    "Solidity", "solidity", ["sol"], [], Full, Guess;
    "Gleam", "gleam", ["gleam"], [], Full, Guess;
    "Groovy", "groovy", ["groovy", "gradle"], [], Full, Guess;
    "PowerShell", "powershell", ["ps1", "psm1", "psd1"], [], Full, Guess;
    "Elixir", "elixir", ["ex", "exs"], [], Full, Guess;
    "Lua", "lua", ["lua"], [], Full, Guess;
    "OCaml", "ocaml", ["ml", "mli"], [], Full, Guess;
    "R", "r", ["r"], [], Full, Guess;
    "Julia", "julia", ["jl"], [], Full, Guess;
    "Haskell", "haskell", ["hs"], [], Full, Guess;
    "Erlang", "erlang", ["erl", "hrl"], [], Full, Guess;
    "Terraform/HCL", "hcl", ["tf", "tfvars", "hcl"], [], Full, Guess;
    "SQL", "sql", ["sql"], ["*.sql.in"], Full, Guess;
    "Clojure", "clojure", ["clj", "cljs", "cljc", "edn"], [], Full, Guess;
    "Common Lisp", "lisp", ["lisp", "lsp", "cl"], [], Full, Guess;
    "Scheme", "scheme", ["scm", "ss"], [], Full, Guess;
    "Emacs Lisp", "elisp", ["el"], [], Full, Guess;
    "JSON/JSONC/TOML/YAML", "config", ["json", "jsonc", "toml", "yaml", "yml"], [], Outline, Structural;
    "Shell", "shell", ["sh", "bash", "zsh", "bats"], [], Outline, Guess;
    "Makefile", "make", ["mk"], ["Makefile", "GNUmakefile"], Outline, Guess;
    "Dockerfile", "docker", [], ["Dockerfile", "Containerfile"], Outline, Guess;
    "Protobuf", "proto", ["proto"], [], Outline, Guess;
    "Pascal", "pascal", ["pas", "pp", "dpr"], [], Outline, Guess;
    "GraphQL", "graphql", ["graphql", "gql"], [], Outline, Guess;
    "Nix", "nix", ["nix"], [], Outline, Guess;
    "Markdown", "markdown", ["md", "markdown"], [], Docs, Structural;
    "Plain text", "text", ["txt"], [], Docs, Structural;
    "reStructuredText", "rst", ["rst"], [], Docs, Structural;
    "AsciiDoc", "asciidoc", ["adoc", "asciidoc"], [], Docs, Structural;
    "Org", "org", ["org"], [], Docs, Structural;
    "LaTeX", "tex", ["tex"], [], Docs, Structural;
}

/// The registry entry for `tag`, or `None` for a tag the index does not recognise.
#[must_use]
pub fn by_tag(tag: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|language| language.tag == tag)
}
