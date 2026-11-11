// SPDX-License-Identifier: Apache-2.0
//! `dctx map <verb> --help`: one short page per map verb with its usage, what it answers and one `Example:` line.
//! The full option list stays in `args::HELP` (`dctx map --help`); every verb page points there.
//! Used by the parent module before parsing, and by `failure` to name a verb's help in a hint.

/// The help page of `verb`, or `None` when `verb` is not a map verb.
pub(super) fn text(verb: &str) -> Option<String> {
    let (usage, about, example) = match verb {
        "map" => (
            "map <TARGET> [--depth 1..3] [--limit N] [--cursor TOKEN] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Outline of a file (m:<path> or a place-relative path) or a symbol and what it contains.",
            "dctx map map src/lib.rs --depth 2",
        ),
        "find" => (
            "find <QUERY> [--kind fn,type,doc] [--in PATH] [--limit 1..40] [--cursor TOKEN] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Ranked symbols, modules, headings and config keys whose name or summary matches QUERY.",
            "dctx map find 'ignore rules' --json",
        ),
        "neighbors" => (
            "neighbors <TARGET> [--dir in|out|both] [--edge calls,imports] [--depth 1..4] [--limit N] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Edges of one node: callers with --dir in --edge calls, callees with --dir out.",
            "dctx map neighbors 's:src/lib.rs#run' --dir in --edge calls",
        ),
        "explain" => (
            "explain <TARGET> [--budget 1..1000] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "One node with its signature and summary, no body.",
            "dctx map explain 's:src/lib.rs#run'",
        ),
        "read" => (
            "read <TARGET> [--lines A-B] [--full] [--budget TOKENS] [--cursor TOKEN] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "The verified source text of one node; a long node pages with --cursor.",
            "dctx map read 's:src/lib.rs#run' --lines 10-40",
        ),
        "skeleton" => (
            "skeleton <FILE> [--budget TOKENS] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Declaration signatures of one file in source order, no bodies.",
            "dctx map skeleton src/lib.rs",
        ),
        "grep" => (
            "grep <LITERAL> [-i] [--in PATH] [--limit N] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Exhaustive literal search of indexed files, hits grouped by enclosing symbol.",
            "dctx map grep TODO --in src",
        ),
        "hubs" => (
            "hubs [--limit 1..60] [--budget TOKENS] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Most-called symbols and per-directory file and symbol counts. No TARGET.",
            "dctx map hubs --limit 10",
        ),
        "path" => (
            "path <FROM> <TO> [--budget TOKENS] [--json] [--place PATH] [--also DIR ...] [--meta]",
            "Shortest edge path (calls, imports, contains; at most 8 hops) between two nodes, or `no path`.",
            "dctx map path 's:src/main.rs#main' 's:src/lib.rs#run'",
        ),
        "export" => (
            "export [--format json|mermaid] [--output PATH] [--place PATH] [--also DIR ...]",
            "The whole graph as one deterministic document. No TARGET, no token cap.",
            "dctx map export --format mermaid --output graph.mmd",
        ),
        _ => return None,
    };
    Some(format!(
        "Usage: dctx map {usage}\n  {about}\nExample: {example}\nEvery option and its bounds: dctx map --help\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_has_one_example_and_other_words_have_none() {
        for verb in [
            "map",
            "find",
            "neighbors",
            "explain",
            "read",
            "skeleton",
            "grep",
            "hubs",
            "path",
            "export",
        ] {
            let page = text(verb).expect(verb);
            assert!(
                page.starts_with(&format!("Usage: dctx map {verb}")),
                "{page}"
            );
            assert_eq!(page.matches("\nExample: dctx map ").count(), 1, "{page}");
        }
        assert!(text("index").is_none());
        assert!(text("--help").is_none());
    }
}
