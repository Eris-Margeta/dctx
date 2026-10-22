//! Regression oracles for heredoc closers and `?>` inside comments.
//! Finding 2 is not a defect: the closer is already the whole label.
//! Finding 3 records PHP's rule that only `//` and `#` comments end at `?>`.
use dctx_lex_php::{Completion, DiagnosticReason, LexedFile, PhpLexer};
use dctx_map_model::EdgeKind;

fn parse(path: &str, source: &str) -> LexedFile {
    PhpLexer::default()
        .extract(path, source.as_bytes())
        .unwrap()
}

fn names(file: &LexedFile) -> Vec<&str> {
    file.nodes.iter().skip(1).map(|n| n.fact.name.as_str()).collect()
}

fn calls(file: &LexedFile) -> Vec<&str> {
    file.references
        .iter()
        .filter(|r| r.fact.kind == EdgeKind::Calls)
        .map(|r| r.fact.name.as_str())
        .collect()
}

#[test]
fn heredoc_closer_is_the_whole_label() {
    // Not a defect. A closer is the whole label, an optional semicolon, then
    // newline or EOF. A label prefix stays body text. Indented closers (PHP 7.3)
    // still end the literal; that indented nowdoc is the positive control.
    let review = r"<?php
$s = <<<END
END of the text
function ghost() {}
END;
function real() {}
";
    let review = parse("review.php", review);
    assert_eq!(review.completion, Completion::Complete, "{:?}", review.diagnostics);
    assert_eq!(names(&review), ["real"]);
    assert_eq!(calls(&review), Vec::<&str>::new());

    let prefix = r"<?php
$s = <<<'NOW'
NO;
function ghost_short() {}
NOW of the text
NOWING
function ghost_long() {}
NOW; function ghost_same() {}
function ghost_line() {}
    NOW;
function kept() { kept_call(); }
";
    let prefix = parse("prefix.php", prefix);
    assert_eq!(prefix.completion, Completion::Complete, "{:?}", prefix.diagnostics);
    assert_eq!(names(&prefix), ["kept"]);
    assert_eq!(calls(&prefix), ["kept_call"]);
}

#[test]
fn block_comment_close_tag_stays_in_php() {
    // `?>` does not end `/* */`. The function after `*/` is PHP (the review
    // expected HTML). With no `*/`, a later `<?php` stays inside the comment.
    // The `//` and `#` arms are the positive control: those comments do end at `?>`.
    let kept = "<?php\n/* ?>\n*/\nfunction kept() { kept_call(); }\n";
    let kept = parse("kept.php", kept);
    assert_eq!(kept.completion, Completion::Complete, "{:?}", kept.diagnostics);
    assert_eq!(names(&kept), ["kept"]);
    assert_eq!(calls(&kept), ["kept_call"]);

    let buried = "<?php\n/* ?>\n<?php\nfunction buried() { buried_call(); }\n";
    let buried = parse("buried.php", buried);
    assert_eq!(buried.completion, Completion::Partial);
    assert_eq!(names(&buried), Vec::<&str>::new());
    assert_eq!(calls(&buried), Vec::<&str>::new());
    assert_eq!(
        buried.diagnostics.iter().map(|d| d.reason).collect::<Vec<_>>(),
        [DiagnosticReason::UnterminatedComment]
    );

    let lines = "<?php\n// ?>\nfunction ghost_line() { line_call(); }\n<?php\n# ?>\nfunction ghost_hash() { hash_call(); }\n<?php\nfunction real() { real_call(); }\n";
    let lines = parse("lines.php", lines);
    assert_eq!(lines.completion, Completion::Complete, "{:?}", lines.diagnostics);
    assert_eq!(names(&lines), ["real"]);
    assert_eq!(calls(&lines), ["real_call"]);
}
