// SPDX-License-Identifier: Apache-2.0
//! Gap oracles for PHP template regions, literal frames and scoped aliases.
//! Implements DX-G10 / PHP1–PHP3. Positive controls sit beside every refusal.
//! Existing fixtures and the public extractor API stay unchanged.
use dctx_lex_php::{Completion, DiagnosticReason, LexError, LexedFile, Limits, PhpLexer};
use dctx_map_model::EdgeKind;

fn parse(path: &str, source: &str) -> LexedFile {
    PhpLexer::default()
        .extract(path, source.as_bytes())
        .unwrap()
}

fn names(file: &LexedFile) -> Vec<&str> {
    file.nodes
        .iter()
        .skip(1)
        .map(|n| n.fact.name.as_str())
        .collect()
}

fn calls(file: &LexedFile) -> Vec<&str> {
    file.references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Calls)
        .map(|r| r.fact.name.as_str())
        .collect()
}

#[test]
fn php1_template_region_properties() {
    let control = parse(
        "control.php",
        "<?php function real(): void { real_call(); }\n",
    );
    assert_eq!(names(&control), ["real"]);
    assert_eq!(calls(&control), ["real_call"]);
    assert_eq!(control.completion, Completion::Complete);

    let html = parse(
        "only.phtml",
        "<div>function decoy() { decoy_call(); }</div>\n",
    );
    assert_eq!(html.nodes.len(), 1);
    assert_eq!(html.references, []);

    let source = r#"<?php
$single = '?> function decoy_single() { single_call(); }';
$double = "?> function decoy_double() { double_call(); }";
$shell = `?> function decoy_shell() { shell_call(); }`;
$doc = <<<DOC
?> function decoy_doc() { doc_call(); }
DOC;
$now = <<<'NOW'
?> function decoy_now() { now_call(); }
NOW;
function kept(): void { kept_call(); }
// line ?>
function decoy_line() { line_call(); }
<?php
# hash ?>
function decoy_hash() { hash_call(); }
<?php
/* block ?> */
function decoy_block() { block_call(); }
<?php
function real(): void { real_call(); }
?>
<div>function decoy_html() { html_call(); }</div>
<?= echo_call(); ?>
"#;
    let file = parse("regions.phtml", source);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    // `?>` does not end `/* */`, so the function after `*/` is PHP.
    // Only the `//` and `#` comments above end at `?>`.
    assert_eq!(names(&file), ["kept", "decoy_block", "real"]);
    assert_eq!(
        calls(&file),
        ["kept_call", "block_call", "real_call", "echo_call"]
    );
    assert!(file.references.iter().all(|r| {
        !r.fact.name.contains("decoy")
            && !r.fact.name.contains("html")
            && !r.fact.name.contains("single")
            && !r.fact.name.contains("double")
            && !r.fact.name.contains("shell")
            && !r.fact.name.contains("doc_")
            && !r.fact.name.contains("now_")
            && !r.fact.name.contains("line_")
            && !r.fact.name.contains("hash")
    }));
}

#[test]
fn php2_heredoc_and_interpolation_depth() {
    let source = r#"<?php
$q = "function decoy_q() { q_call(); }";
$b = `function decoy_b() { b_call(); }`;
$s = 'function decoy_s() { s_call(); }';
$n = <<<'NOW'
function decoy_now() { now_call(); }
{$now_call()}
NOWING
NOW junk
    NOW;
$h = <<<END
function decoy_here() { here_call(); }
ENDING
END junk
function ghost(): void { ghost_call(); }
    END;
function real(): void { real_call(); }
"#;
    let file = parse("literals.php", source);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    assert_eq!(names(&file), ["real"]);
    assert_eq!(calls(&file), ["real_call"]);

    let open = parse(
        "open.php",
        "<?php\n$s = \"function decoy() { decoy_call(); }\nfunction swallowed(): void { swallowed_call(); }\n",
    );
    assert_eq!(open.completion, Completion::Partial);
    assert_eq!(names(&open), Vec::<&str>::new());
    assert_eq!(
        open.diagnostics
            .iter()
            .map(|d| d.reason)
            .collect::<Vec<_>>(),
        [DiagnosticReason::UnterminatedString]
    );
    let closed = parse(
        "closed.php",
        "<?php\n$s = \"function decoy() { decoy_call(); }\";\nfunction kept(): void { kept_call(); }\n",
    );
    assert_eq!(closed.completion, Completion::Complete);
    assert_eq!(names(&closed), ["kept"]);
    assert_eq!(calls(&closed), ["kept_call"]);

    let open_doc = parse(
        "open-doc.php",
        "<?php\n$s = <<<END\nfunction swallowed(): void { swallowed_call(); }\n",
    );
    assert_eq!(open_doc.completion, Completion::Partial);
    assert_eq!(names(&open_doc), Vec::<&str>::new());
    assert_eq!(
        open_doc
            .diagnostics
            .iter()
            .map(|d| d.reason)
            .collect::<Vec<_>>(),
        [DiagnosticReason::UnterminatedHeredoc]
    );
    let closed_doc = parse(
        "closed-doc.php",
        "<?php\n$s = <<<END\nfunction decoy(): void { decoy_call(); }\nEND;\nfunction kept(): void { kept_call(); }\n",
    );
    assert_eq!(closed_doc.completion, Completion::Complete);
    assert_eq!(names(&closed_doc), ["kept"]);
    assert_eq!(calls(&closed_doc), ["kept_call"]);

    let limits = Limits {
        max_depth: 2,
        ..Limits::default()
    };
    let lexer = PhpLexer::new(limits);
    let accepted = "<?php\n$a = \"{$a{0} decoy_call();}\";\n$b = `{$b{0}}`;\n$c = '{${a{{0}}} decoy_call();}';\nfunction kept(): void { kept_call(); }\n";
    let kept = lexer.extract("depth.php", accepted.as_bytes()).unwrap();
    assert_eq!(
        kept.completion,
        Completion::Complete,
        "{:?}",
        kept.diagnostics
    );
    assert_eq!(names(&kept), ["kept"]);
    assert_eq!(calls(&kept), ["kept_call"]);
    let too_deep = "<?php\n$a = \"{$a{{0}}}\";\nfunction swallowed(): void { swallowed_call(); }\n";
    assert_eq!(
        lexer.extract("deep.php", too_deep.as_bytes()),
        Err(LexError::TooDeep { line: 2, limit: 2 })
    );
}

#[test]
fn php3_grouped_alias_scope_properties() {
    // Grouped aliases and trait adaptations already stay inside their namespace.
    // Not a defect; this pins the exact scoped records the blocks fixture omitted.
    let source = r"<?php
namespace One {
    use Foo\{
        Type as Shared,
        function bar as run,
        function baz as run,
        const FLAG as MARK
    };
    trait LocalTrait {
        public function orig(): void {}
    }
    class Box {
        use LocalTrait, Other\Adapted {
            LocalTrait::orig insteadof Other\Adapted;
            orig as private hidden;
            Other\Adapted::orig as shown;
        }
        public function work(): void { run(); shown(); }
    }
}
namespace Two {
    use Bar\{
        function qux as run,
        const FLAG as MARK,
        Type as Shared
    };
    function live(): void { run(); outside(); }
}
";
    let file = parse("aliases.php", source);
    assert_eq!(
        file.completion,
        Completion::Complete,
        "{:?}",
        file.diagnostics
    );
    assert_eq!(
        names(&file),
        [
            "One",
            "One::LocalTrait",
            "One::LocalTrait::orig",
            "One::Box",
            "One::Box::work",
            "Two",
            "Two::live"
        ]
    );
    let refs: Vec<_> = file
        .references
        .iter()
        .map(|r| {
            let owner = file
                .nodes
                .iter()
                .find(|n| n.fact.id == r.fact.src)
                .unwrap()
                .fact
                .name
                .as_str();
            (
                r.fact.scope.as_str(),
                r.fact.kind,
                r.fact.name.as_str(),
                r.fact.alias.as_deref(),
                owner,
                r.fact.owner_path.as_str(),
            )
        })
        .collect();
    assert_eq!(
        refs,
        [
            (
                "One",
                EdgeKind::Imports,
                "Foo\\Type",
                Some("Shared"),
                "One",
                "aliases.php"
            ),
            (
                "One",
                EdgeKind::Imports,
                "Foo\\bar",
                Some("run"),
                "One",
                "aliases.php"
            ),
            (
                "One",
                EdgeKind::Imports,
                "Foo\\baz",
                Some("run"),
                "One",
                "aliases.php"
            ),
            (
                "One",
                EdgeKind::Imports,
                "Foo\\FLAG",
                Some("MARK"),
                "One",
                "aliases.php"
            ),
            (
                "One::Box",
                EdgeKind::UsesType,
                "LocalTrait",
                None,
                "One::Box",
                "aliases.php"
            ),
            (
                "One::Box",
                EdgeKind::UsesType,
                "Other\\Adapted",
                None,
                "One::Box",
                "aliases.php"
            ),
            (
                "One::Box::work",
                EdgeKind::Calls,
                "run",
                None,
                "One::Box::work",
                "aliases.php"
            ),
            (
                "One::Box::work",
                EdgeKind::Calls,
                "shown",
                None,
                "One::Box::work",
                "aliases.php"
            ),
            (
                "Two",
                EdgeKind::Imports,
                "Bar\\qux",
                Some("run"),
                "Two",
                "aliases.php"
            ),
            (
                "Two",
                EdgeKind::Imports,
                "Bar\\FLAG",
                Some("MARK"),
                "Two",
                "aliases.php"
            ),
            (
                "Two",
                EdgeKind::Imports,
                "Bar\\Type",
                Some("Shared"),
                "Two",
                "aliases.php"
            ),
            (
                "Two::live",
                EdgeKind::Calls,
                "run",
                None,
                "Two::live",
                "aliases.php"
            ),
            (
                "Two::live",
                EdgeKind::Calls,
                "outside",
                None,
                "Two::live",
                "aliases.php"
            ),
        ]
    );
}
