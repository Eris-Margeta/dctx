//! Language detection: the extension table, the Ruby and infra basenames, and the tag a path maps to.

/// Duplicate of `dctx-map-parse` `domain::facts::EXTENSIONS`.
///
/// This crate does not depend on `dctx-map-parse`, so the table cannot be
/// shared by a dependency. `tsx` is tagged `typescript` (the JS lexer owns it).
/// Tests lock this copy to the same literal list as map-parse.
pub(crate) const EXTENSIONS: &[(&str, &str)] = &[
    ("rs", "rust"),
    ("js", "javascript"),
    ("mjs", "javascript"),
    ("cjs", "javascript"),
    ("jsx", "javascript"),
    ("ts", "typescript"),
    ("mts", "typescript"),
    ("cts", "typescript"),
    ("tsx", "typescript"),
    ("md", "markdown"),
    ("markdown", "markdown"),
    ("py", "python"),
    ("pyi", "python"),
    ("go", "go"),
    ("java", "java"),
    ("c", "c"),
    ("h", "c"),
    ("cc", "c"),
    ("cpp", "c"),
    ("cxx", "c"),
    ("hpp", "c"),
    ("hh", "c"),
    ("json", "config"),
    ("jsonc", "config"),
    ("toml", "config"),
    ("yaml", "config"),
    ("yml", "config"),
    ("cs", "csharp"),
    ("kt", "kotlin"),
    ("kts", "kotlin"),
    ("rb", "ruby"),
    ("rake", "ruby"),
    ("gemspec", "ruby"),
    ("php", "php"),
    ("phtml", "php"),
    ("inc", "php"),
    ("swift", "swift"),
    ("scala", "scala"),
    ("sc", "scala"),
    ("dart", "dart"),
    ("zig", "zig"),
    ("sol", "solidity"),
    ("gleam", "gleam"),
    ("erl", "erlang"),
    ("hrl", "erlang"),
    ("groovy", "groovy"),
    ("gradle", "groovy"),
    ("ps1", "powershell"),
    ("psm1", "powershell"),
    ("psd1", "powershell"),
    ("txt", "text"),
    ("rst", "rst"),
    ("adoc", "asciidoc"),
    ("asciidoc", "asciidoc"),
    ("org", "org"),
    ("tex", "tex"),
    ("ex", "elixir"),
    ("exs", "elixir"),
    ("lua", "lua"),
    ("ml", "ocaml"),
    ("mli", "ocaml"),
    ("r", "r"),
    ("jl", "julia"),
    ("hs", "haskell"),
    ("tf", "hcl"),
    ("tfvars", "hcl"),
    ("hcl", "hcl"),
    ("sql", "sql"),
    ("sh", "shell"),
    ("bash", "shell"),
    ("zsh", "shell"),
    ("bats", "shell"),
    ("mk", "make"),
    ("proto", "proto"),
    ("pas", "pascal"),
    ("pp", "pascal"),
    ("dpr", "pascal"),
    ("graphql", "graphql"),
    ("gql", "graphql"),
    ("vue", "web"),
    ("svelte", "web"),
    ("astro", "web"),
    ("html", "web"),
    ("htm", "web"),
    ("clj", "clojure"),
    ("cljs", "clojure"),
    ("cljc", "clojure"),
    ("edn", "clojure"),
    ("lisp", "lisp"),
    ("lsp", "lisp"),
    ("cl", "lisp"),
    ("scm", "scheme"),
    ("ss", "scheme"),
    ("el", "elisp"),
    ("nix", "nix"),
];

/// Duplicate of `dctx-map-parse` `domain::facts::RUBY_BASENAMES`.
///
/// `dctx-lex-rb` accepts these final segments case-insensitively, and rejects
/// `Guardfile` and `config.ru`. A basename wins over the extension.
pub(crate) const RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

/// Duplicate of `dctx-map-parse` `domain::facts::INFRA_BASENAMES`: files with no extension that are still a known format.
///
/// Compared case-insensitively on the final path segment, like [`RUBY_BASENAMES`].
pub(crate) const INFRA_BASENAMES: &[(&str, &str)] =
    &[("Makefile", "make"), ("GNUmakefile", "make"), ("Dockerfile", "docker"), ("Containerfile", "docker")];

/// Language tag from the final path segment. Anything else is `opaque`.
///
/// Extensions are case-insensitive. See [`EXTENSIONS`] and [`RUBY_BASENAMES`].
pub(crate) fn language_of(path: &str) -> &'static str {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if RUBY_BASENAMES.iter().any(|name| base.eq_ignore_ascii_case(name)) {
        return "ruby";
    }
    if let Some((_, tag)) = INFRA_BASENAMES.iter().find(|(name, _)| base.eq_ignore_ascii_case(name)) {
        return tag;
    }
    if is_sql_template(base) {
        return "sql";
    }
    let Some(extension) = base.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()) else {
        return "opaque";
    };
    EXTENSIONS
        .iter()
        .find(|(name, _)| *name == extension)
        .map_or("opaque", |(_, tag)| *tag)
}

/// True for `pgtap.sql.in`: a SQL template that a build step turns into `.sql`. Duplicate of `dctx-map-parse` `facts.rs`.
fn is_sql_template(base: &str) -> bool {
    base.len() > ".sql.in".len() && base.get(base.len() - ".sql.in".len()..).is_some_and(|tail| tail.eq_ignore_ascii_case(".sql.in"))
}
