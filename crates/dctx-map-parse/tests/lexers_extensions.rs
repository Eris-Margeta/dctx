// SPDX-License-Identifier: Apache-2.0
//! Shared extension and Ruby basename dispatch.
//! The literal list is the same one locked in `src/domain/facts.rs`,
//! `crates/dctx/src/index/extract.rs` and `crates/dctx/tests/map_languages.rs`.
//! Fixture bytes are the read-only Ruby sample. No fixture file is modified.
#![forbid(unsafe_code)]

use dctx_map_model::{EdgeKind, NodeKind, SymbolKind};
use dctx_map_parse::{Language, ParsedFile, Parser, PureParser};

/// Extension column of `facts::EXTENSIONS`. `tsx` is [`Language::Tsx`] here.
const SHARED_EXTENSIONS: &[(&str, Language)] = &[
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

/// Basenames `dctx-lex-rb` accepts. Same order as `facts::RUBY_BASENAMES`.
const SHARED_RUBY_BASENAMES: &[&str] = &["Rakefile", "Gemfile"];

const RUBY: &[u8] = include_bytes!("fixtures/sample.rb");
const REAL: &[u8] = b"def real\nend\n";

#[test]
fn for_path_matches_the_shared_literal() {
    let mut seen = Vec::new();
    for (extension, language) in SHARED_EXTENSIONS {
        seen.push(*extension);
        assert_eq!(
            Language::for_path(&format!("src/app.{extension}")),
            *language,
            "{extension}"
        );
        assert_eq!(
            Language::for_path(&format!("dir.dot/app.{}", extension.to_ascii_uppercase())),
            *language,
            "{extension}"
        );
    }
    assert_eq!(
        seen,
        [
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
        ]
    );
    for basename in SHARED_RUBY_BASENAMES {
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
    assert_eq!(SHARED_RUBY_BASENAMES, ["Rakefile", "Gemfile"]);
    // A bare word is not an extension. This agrees with facade `language_of`.
    assert_eq!(Language::for_path("php"), Language::Unknown);
    assert_eq!(Language::for_path("Gemfile.lock"), Language::Unknown);
    // Not a defect: `dctx-lex-rb` does not treat these as Ruby.
    assert_eq!(Language::for_path("Guardfile"), Language::Unknown);
    assert_eq!(Language::for_path("config.ru"), Language::Unknown);
    assert_eq!(Language::for_path("pkg/app.pyw"), Language::Unknown);
}

/// Extensionless infra basenames. Same list as `facts::INFRA_BASENAMES` and the facade's `index/extract.rs`.
const SHARED_INFRA_BASENAMES: &[(&str, Language)] = &[
    ("Makefile", Language::Make),
    ("GNUmakefile", Language::Make),
    ("Dockerfile", Language::Docker),
    ("Containerfile", Language::Docker),
];

#[test]
fn infra_basenames_match_the_shared_literal() {
    for (basename, language) in SHARED_INFRA_BASENAMES {
        assert_eq!(Language::for_path(basename), *language, "{basename}");
        assert_eq!(
            Language::for_path(&basename.to_ascii_lowercase()),
            *language
        );
        assert_eq!(Language::for_path(&format!("ci/{basename}")), *language);
        assert_eq!(Language::for_path(&format!("ci\\{basename}")), *language);
        let facts = PureParser.parse(basename, b"x\n").expect(basename);
        assert_eq!(facts.nodes.len(), 1, "module only: {basename}");
    }
    assert_eq!(Language::for_path("Makefile.bak"), Language::Unknown);
    assert_eq!(Language::for_path("Dockerfile.dev"), Language::Unknown);
}

#[test]
fn ruby_basename_facts_match_the_rb_control() {
    let control = real_facts("src/app.rb");
    assert_eq!(control.nodes[0].id.as_str(), "m:src/app.rb");
    assert_eq!(control.nodes[0].lines, [1, 3]);
    assert_eq!(control.nodes[1].id.as_str(), "s:src/app.rb#real");
    assert_eq!(control.nodes[1].lines, [1, 2]);
    assert_eq!(control.nodes[1].signature, "def real");
    assert_eq!(control.nodes[1].summary.text, "def real");
    assert_eq!(control.nodes[1].summary.source.as_str(), "sig");
    for path in [
        "src/app.RB",
        "tasks/app.RAKE",
        "demo.GEMSPEC",
        "Rakefile",
        "lib/Gemfile",
        "pkg/RAKEFILE",
        "GEMFILE",
    ] {
        let facts = real_facts(path);
        assert_eq!(facts.nodes[1].name, "real");
        assert_eq!(facts.nodes[1].id.as_str(), format!("s:{path}#real"));
        assert_eq!(facts.nodes[1].kind, control.nodes[1].kind);
        assert_eq!(facts.nodes[1].symbol_kind, control.nodes[1].symbol_kind);
        assert_eq!(facts.nodes[1].lines, control.nodes[1].lines);
        assert_eq!(facts.nodes[1].signature, control.nodes[1].signature);
        assert_eq!(facts.nodes[1].summary, control.nodes[1].summary);
        assert_eq!(facts.edges[0].dst, facts.nodes[1].id);
    }

    let fixture_control = fixture_facts("src/sample.rb");
    assert_eq!(
        fixture_control
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        ["src/sample.rb", "Box", "Box::run", "helper"]
    );
    for path in ["Rakefile", "lib/Gemfile", "tasks/app.RAKE", "demo.GEMSPEC"] {
        let facts = fixture_facts(path);
        assert_eq!(
            facts
                .nodes
                .iter()
                .skip(1)
                .map(|node| node.name.as_str())
                .collect::<Vec<_>>(),
            ["Box", "Box::run", "helper"]
        );
        assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Class));
        assert_eq!(facts.nodes[2].symbol_kind, Some(SymbolKind::Method));
        assert_eq!(facts.nodes[3].symbol_kind, Some(SymbolKind::Function));
        assert_eq!(facts.nodes[1].lines, fixture_control.nodes[1].lines);
        assert_eq!(facts.nodes[2].signature, fixture_control.nodes[2].signature);
        assert_eq!(facts.references.len(), fixture_control.references.len());
    }
}

fn real_facts(path: &str) -> ParsedFile {
    let facts = PureParser.parse(path, REAL).expect(path);
    assert_eq!(facts.language, Language::Ruby);
    assert_eq!(facts.precision.coverage().as_str(), "structural");
    assert_eq!(facts.precision.completion().as_str(), "complete");
    assert_eq!(facts.precision.parser().as_str(), "dctx-lex-rb:1");
    assert_eq!(facts.nodes.len(), 2);
    assert_eq!(facts.nodes[0].kind, NodeKind::Module);
    assert_eq!(facts.nodes[0].name, path);
    assert_eq!(facts.nodes[1].kind, NodeKind::Symbol);
    assert_eq!(facts.nodes[1].symbol_kind, Some(SymbolKind::Function));
    assert_eq!(facts.edges.len(), 1);
    assert_eq!(facts.edges[0].kind, EdgeKind::Contains);
    assert_eq!(facts.edges[0].src, facts.nodes[0].id);
    assert_eq!(facts.references, [] as [dctx_map_parse::RawReference; 0]);
    facts
}

fn fixture_facts(path: &str) -> ParsedFile {
    let facts = PureParser.parse(path, RUBY).expect(path);
    assert_eq!(facts.language, Language::Ruby);
    assert_eq!(facts.precision.parser().as_str(), "dctx-lex-rb:1");
    assert_eq!(facts.nodes[0].name, path);
    assert_eq!(facts.edges.len(), 3);
    facts
}
