//! `CommonMark` 0.31 heading edge cases through the public API (spec examples 62–100).
//! Checks levels, titles, styles and that code, HTML and containers are honoured.
//! Uses only `dctx_lex_md::extract`.
use dctx_lex_md::{HeadingStyle, ReferenceKind, extract};

fn headings(source: &str) -> Vec<(u8, String)> {
    extract(source.as_bytes())
        .unwrap()
        .headings
        .into_iter()
        .map(|h| (h.level, h.title))
        .collect()
}

fn h(level: u8, title: &str) -> (u8, String) {
    (level, title.into())
}

#[test]
fn atx_levels_markers_and_indentation() {
    assert_eq!(
        headings("# foo\n## foo\n### foo\n#### foo\n##### foo\n###### foo"),
        [
            h(1, "foo"),
            h(2, "foo"),
            h(3, "foo"),
            h(4, "foo"),
            h(5, "foo"),
            h(6, "foo")
        ]
    );
    assert_eq!(headings("####### foo"), []);
    assert_eq!(headings("#5 bolt\n\n#hashtag"), []);
    assert_eq!(headings("\\## foo"), []);
    assert_eq!(
        headings(" ### foo\n  ## foo\n   # foo"),
        [h(3, "foo"), h(2, "foo"), h(1, "foo")]
    );
    assert_eq!(headings("    # foo"), []);
    assert_eq!(headings("foo\n    # bar"), []);
}

#[test]
fn atx_closing_sequences_and_empty_headings() {
    assert_eq!(
        headings("## foo ##\n  ###   bar    ###"),
        [h(2, "foo"), h(3, "bar")]
    );
    assert_eq!(
        headings("# foo ##################################\n##### foo ##"),
        [h(1, "foo"), h(5, "foo")]
    );
    assert_eq!(headings("### foo ### b"), [h(3, "foo ### b")]);
    assert_eq!(headings("# foo#"), [h(1, "foo#")]);
    assert_eq!(headings("## \n#\n### ###"), [h(2, ""), h(1, ""), h(3, "")]);
    let doc = extract(b"#\n").unwrap();
    assert_eq!(doc.headings[0].name, "section");
}

#[test]
fn atx_headings_interrupt_paragraphs() {
    assert_eq!(headings("****\n## foo\n****"), [h(2, "foo")]);
    assert_eq!(headings("Foo bar\n# baz\nBar foo"), [h(1, "baz")]);
}

#[test]
fn setext_underlines_and_multiline_content() {
    assert_eq!(
        headings("Foo *bar*\n=========\n\nFoo *bar*\n---------"),
        [h(1, "Foo *bar*"), h(2, "Foo *bar*")]
    );
    assert_eq!(headings("Foo *bar\nbaz*\n===="), [h(1, "Foo *bar\nbaz*")]);
    assert_eq!(
        headings("  Foo *bar\nbaz*\t\n===="),
        [h(1, "Foo *bar\nbaz*")]
    );
    assert_eq!(
        headings("Foo\n-------------------------\n\nFoo\n="),
        [h(2, "Foo"), h(1, "Foo")]
    );
    assert_eq!(
        headings("   Foo\n---\n\n  Foo\n-----\n\n  Foo\n  ==="),
        [h(2, "Foo"), h(2, "Foo"), h(1, "Foo")]
    );
    assert_eq!(headings("Foo\n   ----      "), [h(2, "Foo")]);
    let doc = extract(b"Foo\n===\n").unwrap();
    assert_eq!(doc.headings[0].style, HeadingStyle::Setext);
}

#[test]
fn setext_refusals() {
    assert_eq!(headings("    Foo\n    ---\n\n    Foo\n---"), []);
    assert_eq!(headings("Foo\n    ---"), []);
    assert_eq!(headings("Foo\n= =\n\nFoo\n--- -"), []);
    assert_eq!(headings("> Foo\n---"), []);
    assert_eq!(headings("> foo\nbar\n==="), []);
    assert_eq!(headings("- Foo\n---"), []);
    assert_eq!(headings("\n===="), []);
    assert_eq!(headings("---\n---"), []);
    assert_eq!(headings("- foo\n-----"), []);
    assert_eq!(headings("    foo\n---"), []);
    assert_eq!(headings("> foo\n-----"), []);
    assert_eq!(headings("\\> foo\n------").len(), 1);
}

#[test]
fn setext_paragraph_boundaries() {
    assert_eq!(headings("Foo\n\nbar\n---\nbaz"), [h(2, "bar")]);
    assert_eq!(
        headings("Foo\nbar\n\n---\n\nbaz"),
        Vec::<(u8, String)>::new()
    );
    assert_eq!(headings("Foo\nbar\n* * *\nbaz"), Vec::<(u8, String)>::new());
    assert_eq!(
        headings("***\nFoo\n---\nBar\n---\nBaz"),
        [h(2, "Foo"), h(2, "Bar")]
    );
    // At the start of a file the same lines are front matter, which is skipped.
    assert_eq!(headings("---\nFoo\n---\nBar\n---\nBaz"), [h(2, "Bar")]);
    assert_eq!(headings("`Foo\n----\n`"), [h(2, "`Foo")]);
}

#[test]
fn code_blocks_never_yield_headings_or_references() {
    let doc = extract(b"# A\n\n````\n```\n# x\n```\n````\n\n~~~ md\n# y `z`\n").unwrap();
    assert_eq!(doc.headings.len(), 1);
    assert_eq!(doc.references, []);
    assert_eq!(doc.code_blocks.len(), 2);
    assert_eq!(doc.code_blocks[1].lines, [9, 10]);
}

#[test]
fn heading_tree_and_sections() {
    let source = "# A\n## B\ntext\n### C\n## D\n# E\n";
    let doc = extract(source.as_bytes()).unwrap();
    let parents: Vec<_> = doc.headings.iter().map(|h| h.parent).collect();
    assert_eq!(parents, [None, Some(0), Some(1), Some(0), None]);
    assert_eq!(doc.headings[1].section.text(source), "## B\ntext\n### C\n");
    assert_eq!(doc.headings[1].lines, [2, 4]);
    assert_eq!(doc.headings[0].lines, [1, 5]);
    assert_eq!(doc.headings[4].lines, [6, 6]);
    assert_eq!(doc.headings[1].summary, "text");
    assert_eq!(doc.headings[3].summary, "D");
}

#[test]
fn references_carry_kinds_spans_and_owners() {
    let source = "Before `x`.\n# A\nSee `Widget`, [p](src/lib.rs), [u](https://e.x), [f](#a).\n";
    let doc = extract(source.as_bytes()).unwrap();
    let refs: Vec<_> = doc
        .references
        .iter()
        .map(|r| {
            (
                r.owner,
                r.kind,
                r.name.as_str(),
                r.span.text(source),
                r.lines,
            )
        })
        .collect();
    assert_eq!(
        refs,
        [
            (None, ReferenceKind::CodeSpan, "x", "`x`", [1, 1]),
            (
                Some(0),
                ReferenceKind::CodeSpan,
                "Widget",
                "`Widget`",
                [3, 3]
            ),
            (
                Some(0),
                ReferenceKind::PathLink,
                "src/lib.rs",
                "src/lib.rs",
                [3, 3]
            ),
            (
                Some(0),
                ReferenceKind::ExternalLink,
                "https://e.x",
                "https://e.x",
                [3, 3]
            ),
            (Some(0), ReferenceKind::AnchorLink, "#a", "#a", [3, 3]),
        ]
    );
}

#[test]
fn references_spanning_lines_map_back_to_source() {
    let source = "# A\n> one `two\n> three` four\n";
    let doc = extract(source.as_bytes()).unwrap();
    assert_eq!(doc.references.len(), 1);
    assert_eq!(doc.references[0].name, "two three");
    assert_eq!(doc.references[0].lines, [2, 3]);
    assert_eq!(doc.references[0].span.text(source), "`two\n> three`");
}
