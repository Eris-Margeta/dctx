//! Gap oracles for C# raw-string delimiters, reopened namespaces and hostile limits.
//! Spec: docs/reviews/dctx-test-gaps.md P05 CS1–CS3; PARSERS-v1 lexical mask.
//! Depends on the public `CsLexer` API; used by `cargo nextest -p dctx-lex-cs`.
//! A quote run of length N glued to a name stays text; do not weaken these oracles.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use dctx_lex_cs::{Completion, CsLexer, DiagnosticReason, LexError, LexedFile, Limits};

    /// CS1: `$` count × quote count × brace run, with a closed control beside each truncated closer.
    #[test]
    fn raw_delimiter_matrix() {
        for dollars in 1..=4 {
            for quotes in 3..=5 {
                for extra in 0..=2 {
                    let closed = raw_source(dollars, quotes, extra, false);
                    assert_raw(&closed, &["anchor", "live", "control"], true, dollars, quotes, extra);
                    let truncated = raw_source(dollars, quotes, extra, true);
                    // `live()` is the positive control inside the refused tail: ghost facts stay absent.
                    assert_raw(&truncated, &["anchor", "live"], false, dollars, quotes, extra);
                }
            }
        }
    }

    fn raw_source(dollars: usize, quotes: usize, extra: usize, truncate: bool) -> String {
        let open_delim = "\"".repeat(quotes);
        let close_len = if truncate { quotes - 1 } else { quotes };
        let close_delim = "\"".repeat(close_len);
        let dollars_s = "$".repeat(dollars);
        // A run one quote shorter than the delimiter stays text. A run of exactly N quotes
        // glued to a name (`"""Z`) stays text too; only a later run ends the literal.
        let decoy = format!(
            "{}Z {}Z ghost() class Ghost {{}}",
            "\"".repeat(quotes - 1),
            "\"".repeat(quotes),
        );
        let literal_braces = if dollars == 1 {
            "{{ghost()}}".to_string()
        } else {
            format!("{}ghost(){}", "{".repeat(dollars - 1), "}".repeat(dollars - 1))
        };
        let hole = if dollars == 1 {
            format!("{}{{live()}}", "{{}}".repeat(extra))
        } else {
            // Extra leading `{` are content; `ghost()` after the real closer must stay literal.
            format!("{}live(){} ghost()", "{".repeat(dollars + extra), "}".repeat(dollars))
        };
        format!(
            "class Host {{ void M() {{ anchor(); }} }}\nstring s = {dollars_s}{open_delim}Z{decoy} {literal_braces} {hole} Q{close_delim};\ncontrol();\n"
        )
    }

    fn assert_raw(source: &str, calls: &[&str], closed: bool, dollars: usize, quotes: usize, extra: usize) {
        let facts = CsLexer::default().extract("raw.cs", source.as_bytes()).expect("extract");
        let got: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "calls")
            .map(|reference| reference.fact.name.as_str())
            .collect();
        assert_eq!(got, calls, "dollars {dollars} quotes {quotes} extra {extra} closed {closed}\n{source}");
        assert!(facts.nodes.iter().any(|node| node.fact.name == "Host"), "{source}");
        assert!(
            facts.nodes.iter().all(|node| !node.fact.name.contains("Ghost"))
                && facts.references.iter().all(|reference| {
                    let name = reference.fact.name.as_str();
                    name != "ghost" && !name.contains("Ghost")
                }),
            "{source}"
        );
        if closed {
            assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
            assert_eq!(facts.diagnostics, []);
        } else {
            assert_eq!(facts.completion, Completion::Partial);
            assert!(facts.diagnostics.iter().any(|diagnostic| diagnostic.reason == DiagnosticReason::UnterminatedString));
        }
    }

    /// CS2: two occurrences of one namespace own their own partial classes, methods and calls.
    /// Not a defect: lexical parent indexes already remap onto ordinal ids after assignment.
    #[test]
    fn reopened_namespace_ownership() {
        for (ns, class_name, method, first, second) in [
            ("N", "A", "F", "one", "two"),
            ("名前", "クラス", "関数", "一", "二"),
        ] {
            let source = reopen(ns, class_name, method, first, second, " ");
            let shifted = format!("\n\n{source}");
            let between = reopen(ns, class_name, method, first, second, "\n\n");
            let base = ownership(&source, ns, class_name, method, first, second);
            let down = ownership(&shifted, ns, class_name, method, first, second);
            let split = ownership(&between, ns, class_name, method, first, second);
            assert_eq!(base.ids, down.ids, "{ns}");
            assert_eq!(base.ids, split.ids, "{ns}");
            assert_eq!(down.lines, base.lines.iter().map(|line| line + 2).collect::<Vec<_>>());
            assert_eq!(split.lines[0], base.lines[0]);
            assert!(split.lines[3] >= base.lines[3] + 2, "{ns}");
            assert_eq!(base.call_src, down.call_src);
            assert_eq!(base.call_src, split.call_src);
        }
    }

    struct Owned {
        ids: Vec<String>,
        lines: Vec<u32>,
        call_src: Vec<String>,
    }

    fn reopen(ns: &str, class_name: &str, method: &str, first: &str, second: &str, gap: &str) -> String {
        format!(
            "namespace {ns} {{ partial class {class_name} {{ void {method}() {{ {first}(); }} }} }}{gap}namespace {ns} {{ partial class {class_name} {{ void {method}() {{ {second}(); }} }} }}"
        )
    }

    fn ownership(source: &str, ns: &str, class_name: &str, method: &str, first: &str, second: &str) -> Owned {
        let facts = CsLexer::default().extract("p.cs", source.as_bytes()).expect("extract");
        assert_eq!(facts.completion, Completion::Complete, "{source} {:?}", facts.diagnostics);
        let symbols: Vec<_> = facts.nodes.iter().skip(1).collect();
        let class_path = format!("{ns}::{class_name}");
        let method_path = format!("{ns}::{class_name}::{method}");
        let names: Vec<_> = symbols.iter().map(|node| node.fact.name.as_str()).collect();
        assert_eq!(
            names,
            vec![ns, class_path.as_str(), method_path.as_str(), ns, class_path.as_str(), method_path.as_str()],
            "{source}"
        );
        let ids: Vec<String> = symbols.iter().map(|node| node.fact.id.as_str().to_string()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::BTreeSet<_>>().len(), "{ids:?}");
        assert!(ids[0].ends_with("~1") && ids[3].ends_with("~2"), "{ids:?}");
        assert!(ids[2].ends_with("~1") && ids[5].ends_with("~2"), "{ids:?}");
        let parent = |child| {
            facts
                .edges
                .iter()
                .find(|edge| &edge.fact.dst == child)
                .map(|edge| &edge.fact.src)
                .expect("parent")
        };
        let module = &facts.nodes[0].fact.id;
        assert_eq!(parent(&symbols[0].fact.id), module);
        assert_eq!(parent(&symbols[1].fact.id), &symbols[0].fact.id);
        assert_eq!(parent(&symbols[2].fact.id), &symbols[1].fact.id);
        assert_eq!(parent(&symbols[3].fact.id), module);
        assert_eq!(parent(&symbols[4].fact.id), &symbols[3].fact.id);
        assert_eq!(parent(&symbols[5].fact.id), &symbols[4].fact.id);
        assert!(facts.edges.iter().all(|edge| edge.fact.owner_path == "p.cs"));
        let calls: Vec<_> = facts
            .references
            .iter()
            .filter(|reference| reference.fact.kind.as_str() == "calls")
            .collect();
        assert_eq!(calls.len(), 2, "{source}");
        assert_eq!(calls[0].fact.name, first);
        assert_eq!(calls[1].fact.name, second);
        assert_eq!(calls[0].fact.src, symbols[2].fact.id);
        assert_eq!(calls[1].fact.src, symbols[5].fact.id);
        assert_eq!(calls[0].fact.scope, method_path);
        assert_eq!(calls[1].fact.scope, method_path);
        assert_ne!(calls[0].fact.src, calls[1].fact.src);
        Owned {
            ids,
            lines: symbols.iter().map(|node| node.fact.lines[0]).collect(),
            call_src: calls.iter().map(|reference| reference.fact.src.as_str().to_string()).collect(),
        }
    }

    /// CS3: byte ceiling, fact-limit boundary, zero limits and the diagnostic cap.
    /// Not a defect: refusals and the 64-diagnostic bound already match these oracles.
    #[test]
    fn hostile_limit_boundaries() {
        let ceiling = Limits::CEILING;
        assert_eq!(ceiling.max_bytes, 8 << 20);

        let sample = b"class A {}";
        let positive = CsLexer::default().extract("z.cs", sample).expect("control");
        assert_eq!(positive.completion, Completion::Complete);
        assert!(positive.nodes.len() > 1);

        let zero = Limits { max_bytes: 0, max_depth: 0, max_facts: 0 };
        assert!(matches!(
            CsLexer::new(zero).extract("z.cs", sample),
            Err(LexError::TooLarge { bytes, limit: 0 }) if bytes == sample.len()
        ));
        assert!(matches!(
            CsLexer::new(Limits { max_bytes: 0, ..Limits::default() }).extract("z.cs", sample),
            Err(LexError::TooLarge { bytes, limit: 0 }) if bytes == sample.len()
        ));
        assert!(matches!(
            CsLexer::new(Limits { max_facts: 0, ..Limits::default() }).extract("z.cs", b""),
            Err(LexError::TooManyFacts { limit: 0 })
        ));
        let flat = CsLexer::new(Limits { max_depth: 0, ..Limits::default() });
        let flat_ok = flat.extract("z.cs", b"int x;").expect("no brackets");
        assert_eq!(flat_ok.completion, Completion::Complete);
        assert_ne!(flat_ok.nodes, []);
        assert!(matches!(
            flat.extract("z.cs", b"\nclass A {}"),
            Err(LexError::TooDeep { line: 2, limit: 0 })
        ));
        let depth_ok = CsLexer::new(Limits { max_depth: 1, ..Limits::default() })
            .extract("z.cs", sample)
            .expect("one brace level");
        assert!(depth_ok.nodes.iter().any(|node| node.fact.name == "A"));

        let base = fact_program(8);
        let base_file = extract_ok("n.cs", &base);
        let facts_n = base_file.nodes.len() + base_file.references.len();
        let wider = format!("{base} extra();");
        let wider_file = extract_ok("n.cs", &wider);
        let facts_n1 = wider_file.nodes.len() + wider_file.references.len();
        assert_eq!(facts_n1, facts_n + 1);
        assert!(wider_file.references.iter().any(|reference| reference.fact.name == "extra"));
        let at_n = CsLexer::new(Limits { max_facts: facts_n, ..Limits::default() });
        assert!(at_n.extract("n.cs", base.as_bytes()).is_ok());
        assert!(matches!(
            at_n.extract("n.cs", wider.as_bytes()),
            Err(LexError::TooManyFacts { limit }) if limit == facts_n
        ));
        let at_n1 = CsLexer::new(Limits { max_facts: facts_n1, ..Limits::default() });
        let accepted = at_n1.extract("n.cs", wider.as_bytes()).expect("N+1");
        assert_eq!(accepted.nodes.len() + accepted.references.len(), facts_n1);

        let mut broken = String::new();
        for _ in 0..70 {
            broken.push('"');
            broken.push('\n');
        }
        let damaged = CsLexer::default().extract("bad.cs", broken.as_bytes()).expect("partial");
        assert_eq!(damaged.completion, Completion::Partial);
        assert_eq!(damaged.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
        assert_eq!(LexedFile::MAX_DIAGNOSTICS, 64);
        for (index, diagnostic) in damaged.diagnostics.iter().enumerate() {
            assert_eq!(diagnostic.reason, DiagnosticReason::UnterminatedString);
            assert_eq!(diagnostic.byte, u32::try_from(index * 2).expect("bounded test coordinate"));
            assert_eq!(diagnostic.line, u32::try_from(index + 1).expect("bounded test coordinate"));
        }
        assert!(damaged.diagnostics.iter().all(|diagnostic| diagnostic.byte < 128));
        let sound = CsLexer::default().extract("ok.cs", b"class A { void M() { ok(); } }").expect("control");
        assert_eq!(sound.completion, Completion::Complete);
        assert_eq!(sound.diagnostics, []);
        assert!(sound.references.iter().any(|reference| reference.fact.name == "ok"));

        let full = dense_program(ceiling.max_bytes);
        assert_eq!(full.len(), ceiling.max_bytes);
        let wide = CsLexer::default().extract("wide.cs", &full).expect("ceiling");
        assert_eq!(wide.completion, Completion::Complete);
        assert_eq!(wide.diagnostics, []);
        assert!(wide.nodes.iter().any(|node| node.fact.name == "C"));
        assert!(wide.nodes.len() + wide.references.len() < ceiling.max_facts);
        let mut over = full;
        over.push(b' ');
        assert!(matches!(
            CsLexer::default().extract("wide.cs", &over),
            Err(LexError::TooLarge { bytes, limit }) if bytes == ceiling.max_bytes + 1 && limit == ceiling.max_bytes
        ));
    }

    fn fact_program(methods: usize) -> String {
        let mut source = String::from("class A {");
        for index in 0..methods {
            write!(source, "void F{index}(){{c{index}();}}").expect("writing to a String cannot fail");
        }
        source.push('}');
        source
    }

    fn extract_ok(path: &str, source: &str) -> LexedFile {
        let facts = CsLexer::default().extract(path, source.as_bytes()).expect(path);
        assert_eq!(facts.completion, Completion::Complete, "{:?}", facts.diagnostics);
        facts
    }

    fn dense_program(len: usize) -> Vec<u8> {
        let prefix = b"class C{void M(){int x=";
        let suffix = b";}}";
        assert!(len > prefix.len() + suffix.len());
        let middle = len - prefix.len() - suffix.len();
        let mut body = Vec::with_capacity(len);
        body.extend_from_slice(prefix);
        for _ in 0..(middle / 2) {
            body.extend_from_slice(b"0+");
        }
        if middle % 2 == 1 {
            body.push(b'0');
        }
        body.extend_from_slice(suffix);
        assert_eq!(body.len(), len);
        body
    }
}
