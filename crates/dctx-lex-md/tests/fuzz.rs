//! Property tests: arbitrary bytes and Markdown-shaped noise never panic or break invariants.
//! Checks the bounded-parser rule (ARCHITECTURE-PROPOSAL §3) and span/line consistency.
//! Uses proptest and the public API only.
use dctx_lex_md::{Document, LexError, Limits, extract, extract_with};
use proptest::prelude::*;

fn check(source: &str, doc: &Document) {
    let lines = u32::try_from(source.lines().count().max(1)).expect("bounded test coordinate");
    assert_eq!(doc.lines, lines);
    for (index, heading) in doc.headings.iter().enumerate() {
        assert!((1..=6).contains(&heading.level));
        assert!(heading.span.start <= heading.span.end && heading.span.end <= heading.section.end);
        assert!(heading.section.end <= source.len());
        assert!(source.is_char_boundary(heading.span.start));
        assert!(source.is_char_boundary(heading.section.end));
        assert!(heading.lines[0] >= 1 && heading.lines[0] <= heading.lines[1]);
        assert!(heading.parent.is_none_or(|parent| parent < index));
        assert_ne!(heading.name, "");
    }
    for reference in &doc.references {
        assert_ne!(reference.name, "");
        assert!(reference.span.start < reference.span.end && reference.span.end <= source.len());
        assert!(source.is_char_boundary(reference.span.start));
        assert!(source.is_char_boundary(reference.span.end));
        assert!(reference.lines[0] >= 1 && reference.lines[0] <= reference.lines[1]);
        assert!(
            reference
                .owner
                .is_none_or(|owner| owner < doc.headings.len())
        );
    }
    for block in &doc.code_blocks {
        assert!(block.span.start <= block.span.end && block.span.end <= source.len());
        assert!(block.lines[0] <= block.lines[1] && block.lines[1] <= lines);
    }
}

fn markdownish() -> impl Strategy<Value = String> {
    let piece = prop::sample::select(vec![
        "# ",
        "## ",
        "###### ",
        "#",
        "\n",
        "\n\n",
        "    ",
        "\t",
        "> ",
        "- ",
        "1. ",
        "* * *",
        "---",
        "===",
        "```",
        "~~~",
        "`",
        "``",
        "[",
        "]",
        "(",
        ")",
        "![",
        "](",
        "<",
        ">",
        "\\",
        "\"",
        "<!--",
        "-->",
        "<div>",
        "text",
        "Žü",
        " ",
        "\r\n",
        "+++",
        "<https://x>",
    ]);
    prop::collection::vec(piece, 0..80).prop_map(|pieces| pieces.concat())
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        match extract(&bytes) {
            Ok(doc) => check(std::str::from_utf8(&bytes).unwrap(), &doc),
            Err(LexError::InvalidUtf8 { valid_up_to }) => {
                prop_assert!(std::str::from_utf8(&bytes[..valid_up_to]).is_ok());
            }
            Err(other) => prop_assert!(matches!(other, LexError::TooDeep { .. }), "{other}"),
        }
    }

    #[test]
    fn markdown_shaped_noise_keeps_invariants(source in markdownish()) {
        match extract(source.as_bytes()) {
            Ok(doc) => {
                check(&source, &doc);
                prop_assert_eq!(extract(source.as_bytes()).unwrap(), doc);
            }
            Err(error) => prop_assert!(matches!(error, LexError::TooDeep { .. }), "{error}"),
        }
    }
}

#[test]
fn deep_nesting_is_refused_with_a_positive_control() {
    let deep = "> ".repeat(40) + "# x";
    assert!(matches!(
        extract(deep.as_bytes()),
        Err(LexError::TooDeep { line: 1, max: 32 })
    ));
    let shallow = "> ".repeat(8) + "# x";
    assert_eq!(extract(shallow.as_bytes()).unwrap().headings.len(), 1);
    let lists = "1. ".repeat(40) + "x";
    assert!(matches!(
        extract(lists.as_bytes()),
        Err(LexError::TooDeep { .. })
    ));
}

#[test]
fn bracket_and_backtick_floods_stay_bounded() {
    let limits = Limits::default();
    let flood = format!("# A\n{}{}", "[".repeat(100_000), "`` ` ".repeat(20_000));
    let doc = extract_with(flood.as_bytes(), &limits).unwrap();
    assert!(doc.references.len() <= 20_000);
    let titles = format!("# A\n{}", "[a](b \"".repeat(20_000));
    assert_eq!(extract(titles.as_bytes()).unwrap().references, []);
}
