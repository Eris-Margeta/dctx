//! Golden facts, malformed partials and proptest fuzz for the Python extractor.
//! Fixture sources live in this file: the packet owns this path, not a fixtures directory.
#![forbid(unsafe_code)]

use dctx_lex_py::{
    body_fingerprint, Completion, DiagnosticReason, Dialect, Labeled, LexError, LexedFile, Limits,
    Precision, PyLexer, PARSER_ID, PARSER_REVISION,
};
use dctx_map_model::{Confidence, EdgeKind, Node, NodeKind, SummarySource, SymbolKind};
use proptest::prelude::*;

const SAMPLE: &str = concat!(
    "\"\"\"Package entry. More text.\"\"\"\n",
    "import os\n",
    "import os.path as osp\n",
    "from . import sibling\n",
    "from ..pkg.mod import name as n, other\n",
    "from typing import *\n",
    "import a.b, c\n",
    "\n",
    "MAX = 3\n",
    "NAME: str = \"py\"\n",
    "\n",
    "@decorator\n",
    "@wrap(helper())\n",
    "async def run(n: int = default()) -> int:\n",
    "    \"\"\"Runs the job. Returns the count.\"\"\"\n",
    "    return os.getcwd()\n",
    "\n",
    "class Box(Base, factory()):\n",
    "    \"\"\"A box.\"\"\"\n",
    "\n",
    "    def __init__(self, size=seed()):\n",
    "        self.n = size\n",
    "\n",
    "    def _hidden(self):\n",
    "        return self.n\n",
    "\n",
    "    @staticmethod\n",
    "    def make():\n",
    "        return run()\n",
    "\n",
    "def nested():\n",
    "    def inner():\n",
    "        helper()\n",
);

const MASKING: &str = concat!(
    "def real():\n",
    "    return f\"x{compute(1)}y\"\n",
    "t\"t{compute(2)}\"\n",
    "s = b\"compute()\"\n",
    "s2 = \"compute()\"\n",
    "# compute()\n",
    "f\"{1 # compute()\n",
    "}\"\n",
    "f\"{x:#compute()}\"\n",
    "f\"{name:{fmt()}}\"\n",
);

const CONSTS: &str = concat!(
    "(A, B) = pair()\n",
    "A = B = 1\n",
    "C += 1\n",
    "NAME: int\n",
    "if True:\n",
    "    D = 1\n",
    "type Alias = int\n",
    "_MAX = 1\n",
);

const CONT: &str = "VALUE = helper(\\\n    1)\n";
const SEMI: &str = "MAX = 1; MIN = 2\n";

const MATCH_SRC: &str = concat!(
    "def outer():\n",
    "    match subject():\n",
    "        case Point(1, 2):\n",
    "            inside()\n",
    "        case other if guard():\n",
    "            taken()\n",
);

const PARTIAL_SRC: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = \"unterminated\n",
    "def also():\n",
    "    return call()\n",
);

const TRIPLE_OPEN: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = \"\"\"still\n",
    "def swallowed():\n",
    "    pass\n",
);

const TRIPLE_CLOSED: &str = concat!(
    "s = \"\"\"\n",
    "def fake():\n",
    "    pass\n",
    "\"\"\"\n",
    "real()\n",
);

const PAREN_SRC: &str = concat!(
    "def kept():\n",
    "    return 1\n",
    "x = (\n",
    "def later():\n",
    "    pass\n",
);

const IMPORTS_SRC: &str = concat!(
    "from ... import *\n",
    "def f():\n",
    "    from os import path as p, other\n",
    "    import a.b\n",
    "from x import (\n",
    "    a,\n",
    "    b as c,\n",
    ")\n",
    "from import missing\n",
);

const CALLS_SRC: &str = concat!(
    "def f():\n",
    "    (lambda: 1)()\n",
    "    (fn)()\n",
    "    obj[k]()\n",
    "    obj.method()\n",
    "    with open(path) as h:\n",
    "        h.read()\n",
    "    try:\n",
    "        pass\n",
    "    except Error():\n",
    "        recover()\n",
    "    return items()\n",
);

const ASYNC_FOR: &str = concat!(
    "async def f():\n",
    "    async for x in items():\n",
    "        use(x)\n",
);

const DOC_SRC: &str = concat!(
    "def raw():\n",
    "    r\"\"\"Raw doc. More.\"\"\"\n",
    "    return 1\n",
    "\n",
    "def binary():\n",
    "    b\"\"\"not a doc\"\"\"\n",
    "    return 1\n",
    "\n",
    "def escaped():\n",
    "    \"Line\\nTwo. More\"\n",
    "    return 1\n",
    "\n",
    "def joined():\n",
    "    \"Hello. \" \"World.\"\n",
    "    return 1\n",
    "\n",
    "def wrapped():\n",
    "    (\"Only one.\")\n",
    "    return 1\n",
    "\n",
    "def formatted():\n",
    "    f\"not {1} a doc\"\n",
    "    return 1\n",
);

const SHEBANG: &str = "#!/usr/bin/env python3\n# First sentence. Second.\ndef f():\n    pass\n";
const ORDINALS: &str = "def f():\n    pass\ndef f():\n    pass\n";
const BOM_SRC: &str = "\u{feff}def f():\n    pass\n";
const CRLF_SRC: &str = "def f():\r\n    return g()\r\n";
const TABS_SRC: &str = "def f():\n\treturn g()\n";
const MIXED_SRC: &str = "def f():\n    return 1\n \treturn g()\n";
const DEDENT_SRC: &str = "def f():\n    return 1\n  return 2\n";
const UNICODE_SRC: &str = "def café():\n    return \"数\"\n";
const ONELINER: &str = "def a(): def b(): def c(): pass\n";
const FNEST: &str = "x = f\"{f'{f\"{1}\"}'}\"\n";

const FIXTURES: &[&str] = &[
    SAMPLE,
    MASKING,
    CONSTS,
    CONT,
    SEMI,
    MATCH_SRC,
    PARTIAL_SRC,
    TRIPLE_OPEN,
    TRIPLE_CLOSED,
    PAREN_SRC,
    IMPORTS_SRC,
    CALLS_SRC,
    ASYNC_FOR,
    DOC_SRC,
    SHEBANG,
    UNICODE_SRC,
    CRLF_SRC,
    TABS_SRC,
    MIXED_SRC,
    DEDENT_SRC,
    ORDINALS,
    BOM_SRC,
    ONELINER,
    FNEST,
];

fn extract(path: &str, source: &str) -> LexedFile {
    PyLexer::default()
        .extract(path, source.as_bytes())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn line_of(source: &str, needle: &str) -> u32 {
    let byte = source
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    source[..byte].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn last_line_of(source: &str, needle: &str) -> u32 {
    let byte = source
        .rfind(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    source[..byte].bytes().filter(|byte| *byte == b'\n').count() as u32 + 1
}

fn node<'a>(facts: &'a LexedFile, name: &str) -> &'a Labeled<Node> {
    facts
        .nodes
        .iter()
        .find(|item| item.fact.name == name)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn names(facts: &LexedFile) -> Vec<&str> {
    facts.nodes.iter().map(|item| item.fact.name.as_str()).collect()
}

fn call_names(facts: &LexedFile) -> Vec<&str> {
    facts
        .references
        .iter()
        .filter(|item| item.fact.kind == EdgeKind::Calls)
        .map(|item| item.fact.name.as_str())
        .collect()
}

fn line_limit(source: &str) -> u32 {
    if source.contains('\r') {
        u32::MAX
    } else {
        source.split_inclusive('\n').count().max(1) as u32
    }
}

fn check_invariants(source: &str, facts: &LexedFile) {
    assert!(!facts.nodes.is_empty());
    assert_eq!(facts.edges.len(), facts.nodes.len() - 1);
    let module = &facts.nodes[0];
    assert_eq!(module.fact.kind, NodeKind::Module);
    assert!(module.fact.symbol_kind.is_none());
    assert!(!module.fact.public);
    assert_eq!(module.fact.name, facts.path);
    assert_eq!(module.fact.fingerprint, body_fingerprint(source));
    assert_eq!(module.precision, Precision::Structural);
    let limit = line_limit(source);
    let ids: Vec<&str> = facts.nodes.iter().map(|item| item.fact.id.as_str()).collect();
    for item in &facts.nodes {
        let [start, end] = item.fact.lines;
        assert!(start >= 1 && start <= end && end <= limit);
        assert_eq!(item.fact.path, facts.path);
    }
    for item in &facts.references {
        let [start, end] = item.fact.lines;
        assert!(start >= 1 && start <= end && end <= limit);
        assert!(ids.contains(&item.fact.src.as_str()));
        assert!(matches!(item.fact.kind, EdgeKind::Imports | EdgeKind::Calls));
        assert_eq!(item.fact.owner_path, facts.path);
    }
    for (index, edge) in facts.edges.iter().enumerate() {
        assert_eq!(edge.fact.kind, EdgeKind::Contains);
        assert_eq!(edge.fact.conf, Confidence::Exact);
        assert_eq!(edge.fact.owner_path, facts.path);
        assert_eq!(edge.fact.dst, facts.nodes[index + 1].fact.id);
        assert!(ids.contains(&edge.fact.src.as_str()));
        assert_eq!(edge.precision, facts.nodes[index + 1].precision);
    }
    assert!(facts.diagnostics.len() <= LexedFile::MAX_DIAGNOSTICS);
    let mut previous = 0u32;
    for diagnostic in &facts.diagnostics {
        assert!(diagnostic.byte >= previous);
        assert!(diagnostic.line >= 1);
        previous = diagnostic.byte;
    }
    if facts.diagnostics.is_empty() {
        assert_eq!(facts.completion, Completion::Complete);
        assert!(facts.nodes.iter().all(|item| item.precision != Precision::Partial));
        assert!(facts.references.iter().all(|item| item.precision != Precision::Partial));
    } else {
        assert_eq!(facts.completion, Completion::Partial);
    }
}

#[test]
fn parser_identity_and_refusal_strings() {
    assert_eq!(PARSER_ID, "dctx-lex-py");
    assert_eq!(PARSER_REVISION, 1);
    assert_eq!(
        LexError::UnsupportedDialect.to_string(),
        "path is not a Python file"
    );
    assert_eq!(
        LexError::TooLarge { bytes: 9, limit: 4 }.to_string(),
        "file of 9 bytes exceeds the 4-byte limit"
    );
    assert_eq!(
        LexError::TooManyFacts { limit: 3 }.to_string(),
        "file yields more than 3 facts"
    );
    assert_eq!(
        LexError::TooDeep { line: 4, limit: 8 }.to_string(),
        "nesting deeper than 8 at line 4"
    );
    let lexer = PyLexer::default();
    assert!(matches!(lexer.extract("x.js", b""), Err(LexError::UnsupportedDialect)));
    assert!(matches!(lexer.extract("../x.py", b""), Err(LexError::InvalidId(_))));
    assert!(matches!(lexer.extract("/abs.py", b""), Err(LexError::InvalidId(_))));
    assert!(matches!(lexer.extract("a\\b.py", b""), Err(LexError::InvalidId(_))));
    assert!(matches!(lexer.extract("x.py", &[0xff]), Err(LexError::InvalidUtf8(_))));
    let small = PyLexer::new(Limits { max_bytes: 4, ..Limits::default() });
    assert!(matches!(
        small.extract("a.py", b"hello"),
        Err(LexError::TooLarge { bytes: 5, limit: 4 })
    ));
    assert!(small.extract("a.py", b"a=1").is_ok());
    let empty = extract("m.py", "");
    assert_eq!(empty.nodes.len(), 1);
    assert_eq!(empty.nodes[0].fact.lines, [1, 1]);
    assert!(empty.edges.is_empty());
    assert!(empty.references.is_empty());
    assert_eq!(empty.completion, Completion::Complete);
}

#[test]
fn sample_module_matches_the_golden_facts() {
    let facts = extract("pkg/sample.py", SAMPLE);
    let again = extract("pkg/sample.py", SAMPLE);
    assert_eq!(facts, again);
    check_invariants(SAMPLE, &facts);
    assert_eq!(facts.dialect, Dialect::Python);
    assert_eq!(facts.path, "pkg/sample.py");
    assert!(facts.diagnostics.is_empty());
    assert_eq!(
        names(&facts),
        [
            "pkg/sample.py",
            "MAX",
            "NAME",
            "run",
            "Box",
            "Box::__init__",
            "Box::_hidden",
            "Box::make",
            "nested",
            "nested::inner",
        ]
    );
    let kinds = [
        None,
        Some(SymbolKind::Const),
        Some(SymbolKind::Const),
        Some(SymbolKind::Function),
        Some(SymbolKind::Class),
        Some(SymbolKind::Method),
        Some(SymbolKind::Method),
        Some(SymbolKind::Method),
        Some(SymbolKind::Function),
        Some(SymbolKind::Function),
    ];
    for (item, kind) in facts.nodes.iter().zip(kinds) {
        assert_eq!(item.fact.symbol_kind, kind, "{}", item.fact.name);
        assert_eq!(item.fact.kind, if kind.is_none() { NodeKind::Module } else { NodeKind::Symbol });
    }
    let public = [false, true, true, true, true, true, false, true, true, true];
    for (item, is_public) in facts.nodes.iter().zip(public) {
        assert_eq!(item.fact.public, is_public, "{}", item.fact.name);
    }
    for item in &facts.nodes {
        let precision = if matches!(item.fact.symbol_kind, Some(SymbolKind::Const)) {
            Precision::Convention
        } else {
            Precision::Structural
        };
        assert_eq!(item.precision, precision, "{}", item.fact.name);
    }
    let ids = [
        "m:pkg/sample.py",
        "s:pkg/sample.py#MAX",
        "s:pkg/sample.py#NAME",
        "s:pkg/sample.py#run",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box::__init__",
        "s:pkg/sample.py#Box::_hidden",
        "s:pkg/sample.py#Box::make",
        "s:pkg/sample.py#nested",
        "s:pkg/sample.py#nested::inner",
    ];
    for (item, id) in facts.nodes.iter().zip(ids) {
        assert_eq!(item.fact.id.as_str(), id);
    }
    let spans = [
        ("pkg/sample.py", 1, SAMPLE.lines().count() as u32),
        ("MAX", line_of(SAMPLE, "MAX = 3"), line_of(SAMPLE, "MAX = 3")),
        ("NAME", line_of(SAMPLE, "NAME: str"), line_of(SAMPLE, "NAME: str")),
        ("run", line_of(SAMPLE, "@decorator"), line_of(SAMPLE, "return os.getcwd()")),
        ("Box", line_of(SAMPLE, "class Box"), line_of(SAMPLE, "return run()")),
        ("Box::__init__", line_of(SAMPLE, "def __init__"), line_of(SAMPLE, "self.n = size")),
        ("Box::_hidden", line_of(SAMPLE, "def _hidden"), line_of(SAMPLE, "return self.n")),
        ("Box::make", line_of(SAMPLE, "@staticmethod"), line_of(SAMPLE, "return run()")),
        ("nested", line_of(SAMPLE, "def nested"), last_line_of(SAMPLE, "helper()")),
        ("nested::inner", line_of(SAMPLE, "def inner"), last_line_of(SAMPLE, "helper()")),
    ];
    for (name, start, end) in spans {
        assert_eq!(node(&facts, name).fact.lines, [start, end], "{name}");
    }
    let signatures = [
        ("pkg/sample.py", ""),
        ("MAX", "MAX = 3"),
        ("NAME", "NAME: str = \"py\""),
        ("run", "@decorator @wrap(helper()) async def run(n: int = default()) -> int:"),
        ("Box", "class Box(Base, factory()):"),
        ("Box::__init__", "def __init__(self, size=seed()):"),
        ("Box::_hidden", "def _hidden(self):"),
        ("Box::make", "@staticmethod def make():"),
        ("nested", "def nested():"),
        ("nested::inner", "def inner():"),
    ];
    for (name, signature) in signatures {
        assert_eq!(node(&facts, name).fact.signature, signature, "{name}");
    }
    let summaries = [
        ("pkg/sample.py", SummarySource::Doc, "Package entry."),
        ("MAX", SummarySource::Signature, "MAX = 3"),
        ("NAME", SummarySource::Signature, "NAME: str = \"py\""),
        ("run", SummarySource::Doc, "Runs the job."),
        ("Box", SummarySource::Doc, "A box."),
        ("Box::__init__", SummarySource::Signature, "def __init__(self, size=seed()):"),
        ("Box::_hidden", SummarySource::Signature, "def _hidden(self):"),
        ("Box::make", SummarySource::Signature, "@staticmethod def make():"),
        ("nested", SummarySource::Signature, "def nested():"),
        ("nested::inner", SummarySource::Signature, "def inner():"),
    ];
    for (name, source, text) in summaries {
        let summary = &node(&facts, name).fact.summary;
        assert_eq!(summary.source, source, "{name}");
        assert_eq!(summary.text, text, "{name}");
    }
    let bodies = [
        ("pkg/sample.py", SAMPLE),
        ("MAX", "MAX = 3"),
        ("NAME", "NAME: str = \"py\""),
        ("run", "\"\"\"Runs the job. Returns the count.\"\"\" return os.getcwd()"),
        (
            "Box",
            "\"\"\"A box.\"\"\" def __init__(self, size=seed()): self.n = size def _hidden(self): return self.n @staticmethod def make(): return run()",
        ),
        ("Box::__init__", "self.n = size"),
        ("Box::_hidden", "return self.n"),
        ("Box::make", "return run()"),
        ("nested", "def inner(): helper()"),
        ("nested::inner", "helper()"),
    ];
    for (name, body) in bodies {
        assert_eq!(
            node(&facts, name).fact.fingerprint,
            body_fingerprint(body),
            "{name} {body}"
        );
    }
    let parents = [
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "m:pkg/sample.py",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box",
        "s:pkg/sample.py#Box",
        "m:pkg/sample.py",
        "s:pkg/sample.py#nested",
    ];
    for (edge, parent) in facts.edges.iter().zip(parents) {
        assert_eq!(edge.fact.src.as_str(), parent);
        assert_eq!(edge.fact.kind, EdgeKind::Contains);
        assert_eq!(edge.fact.conf, Confidence::Exact);
    }
    // Same-start references keep the longer span first (`c` before `a.b`, `other` before `name`).
    let expected = [
        ("m:pkg/sample.py", EdgeKind::Imports, "os", "", None, line_of(SAMPLE, "import os")),
        ("m:pkg/sample.py", EdgeKind::Imports, "os.path", "", Some("osp"), line_of(SAMPLE, "import os.path")),
        ("m:pkg/sample.py", EdgeKind::Imports, ".::sibling", "", None, line_of(SAMPLE, "from . import")),
        ("m:pkg/sample.py", EdgeKind::Imports, "..pkg.mod::other", "", None, line_of(SAMPLE, "from ..pkg.mod")),
        ("m:pkg/sample.py", EdgeKind::Imports, "..pkg.mod::name", "", Some("n"), line_of(SAMPLE, "from ..pkg.mod")),
        ("m:pkg/sample.py", EdgeKind::Imports, "typing::*", "", None, line_of(SAMPLE, "from typing")),
        ("m:pkg/sample.py", EdgeKind::Imports, "c", "", None, line_of(SAMPLE, "import a.b")),
        ("m:pkg/sample.py", EdgeKind::Imports, "a.b", "", None, line_of(SAMPLE, "import a.b")),
        ("m:pkg/sample.py", EdgeKind::Calls, "wrap", "", None, line_of(SAMPLE, "@wrap")),
        ("m:pkg/sample.py", EdgeKind::Calls, "helper", "", None, line_of(SAMPLE, "@wrap")),
        ("m:pkg/sample.py", EdgeKind::Calls, "default", "", None, line_of(SAMPLE, "default()")),
        ("s:pkg/sample.py#run", EdgeKind::Calls, "os.getcwd", "run", None, line_of(SAMPLE, "os.getcwd()")),
        ("m:pkg/sample.py", EdgeKind::Calls, "factory", "", None, line_of(SAMPLE, "factory()")),
        ("s:pkg/sample.py#Box", EdgeKind::Calls, "seed", "Box", None, line_of(SAMPLE, "seed()")),
        ("s:pkg/sample.py#Box::make", EdgeKind::Calls, "run", "Box::make", None, line_of(SAMPLE, "return run()")),
        ("s:pkg/sample.py#nested::inner", EdgeKind::Calls, "helper", "nested::inner", None, last_line_of(SAMPLE, "helper()")),
    ];
    assert_eq!(facts.references.len(), expected.len());
    for (item, (src, kind, name, scope, alias, line)) in facts.references.iter().zip(expected) {
        assert_eq!(item.precision, Precision::Structural, "{name}");
        assert_eq!(item.fact.src.as_str(), src, "{name}");
        assert_eq!(item.fact.kind, kind, "{name}");
        assert_eq!(item.fact.name, name);
        assert_eq!(item.fact.scope, scope, "{name}");
        assert_eq!(item.fact.alias.as_deref(), alias, "{name}");
        assert_eq!(item.fact.lines, [line, line], "{name}");
        assert_eq!(item.fact.owner_path, "pkg/sample.py");
    }
    let stub = extract("pkg/sample.pyi", SAMPLE);
    assert_eq!(stub.dialect, Dialect::Stub);
    assert_eq!(stub.completion, Completion::Complete);
    assert_eq!(stub.nodes.len(), facts.nodes.len());
    let windows = extract("pkg/sample.pyw", SAMPLE);
    assert_eq!(windows.dialect, Dialect::Python);
}

#[test]
fn strings_comments_and_t_strings_do_not_invent_declarations() {
    let facts = extract("m.py", MASKING);
    check_invariants(MASKING, &facts);
    assert_eq!(names(&facts), ["m.py", "real"]);
    assert_eq!(facts.completion, Completion::Complete);
    let calls: Vec<_> = facts
        .references
        .iter()
        .map(|item| (item.fact.name.as_str(), item.fact.scope.as_str()))
        .collect();
    assert_eq!(calls, [("compute", "real"), ("compute", ""), ("fmt", "")]);
}

#[test]
fn constants_are_module_level_all_caps_only() {
    let facts = extract("c.py", CONSTS);
    check_invariants(CONSTS, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    let ids: Vec<_> = facts.nodes.iter().skip(1).map(|item| item.fact.id.as_str()).collect();
    assert_eq!(
        ids,
        ["s:c.py#A~1", "s:c.py#B~1", "s:c.py#A~2", "s:c.py#B~2", "s:c.py#NAME"]
    );
    assert_eq!(facts.nodes[1].fact.signature, "(A, B) = pair()");
    assert_eq!(facts.nodes[2].fact.signature, "(A, B) = pair()");
    assert_eq!(facts.nodes[3].fact.signature, "A = B = 1");
    assert_eq!(facts.nodes[4].fact.signature, "A = B = 1");
    assert_eq!(facts.nodes[5].fact.signature, "NAME: int");
    assert!(facts.nodes.iter().skip(1).all(|item| item.precision == Precision::Convention));
    assert_eq!(call_names(&facts), ["pair"]);
    let semi = extract("s.py", SEMI);
    assert_eq!(semi.nodes[1].fact.signature, "MAX = 1");
    assert_eq!(semi.nodes[2].fact.signature, "MIN = 2");
    assert_eq!(semi.completion, Completion::Complete);
    let continued = extract("v.py", CONT);
    assert_eq!(continued.nodes[1].fact.name, "VALUE");
    assert_eq!(continued.nodes[1].precision, Precision::Convention);
    assert_eq!(continued.references[0].fact.name, "helper");
    assert_eq!(continued.references[0].fact.scope, "");
    assert_eq!(continued.completion, Completion::Complete);
}

#[test]
fn match_patterns_abstain_and_guards_are_calls() {
    let facts = extract("m.py", MATCH_SRC);
    check_invariants(MATCH_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(names(&facts), ["m.py", "outer"]);
    assert_eq!(node(&facts, "outer").fact.symbol_kind, Some(SymbolKind::Function));
    assert_eq!(call_names(&facts), ["subject", "inside", "guard", "taken"]);
    assert!(facts.references.iter().all(|item| item.fact.scope == "outer"));
    assert!(facts.references.iter().all(|item| item.fact.name != "Point"));
}

#[test]
fn imports_cover_relative_star_alias_and_function_scope() {
    let facts = extract("i.py", IMPORTS_SRC);
    check_invariants(IMPORTS_SRC, &facts);
    assert_eq!(facts.completion, Completion::Partial);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnexpectedToken));
    let mut got: Vec<_> = facts
        .references
        .iter()
        .map(|item| {
            (
                item.fact.name.as_str(),
                item.fact.alias.as_deref(),
                item.fact.scope.as_str(),
                item.fact.kind,
            )
        })
        .collect();
    got.sort();
    let mut expected = vec![
        ("...::*", None, "", EdgeKind::Imports),
        ("a.b", None, "f", EdgeKind::Imports),
        ("os::other", None, "f", EdgeKind::Imports),
        ("os::path", Some("p"), "f", EdgeKind::Imports),
        ("x::a", None, "", EdgeKind::Imports),
        ("x::b", Some("c"), "", EdgeKind::Imports),
    ];
    expected.sort();
    assert_eq!(got, expected);
    assert!(facts.references.iter().all(|item| item.fact.name != "missing"));
    let function = node(&facts, "f").fact.id.as_str();
    assert!(facts.references.iter().filter(|item| item.fact.scope == "f").all(|item| item.fact.src.as_str() == function));
}

#[test]
fn dynamic_callees_abstain_and_compound_headers_keep_the_enclosing_scope() {
    let facts = extract("c.py", CALLS_SRC);
    check_invariants(CALLS_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert_eq!(
        call_names(&facts),
        ["obj.method", "open", "h.read", "Error", "recover", "items"]
    );
    assert!(facts.references.iter().all(|item| item.fact.scope == "f"));
    let async_for = extract("a.py", ASYNC_FOR);
    assert_eq!(node(&async_for, "f").fact.symbol_kind, Some(SymbolKind::Function));
    assert_eq!(node(&async_for, "f").fact.signature, "async def f():");
    assert_eq!(call_names(&async_for), ["items", "use"]);
    assert!(async_for.references.iter().all(|item| item.fact.scope == "f"));
    assert_eq!(async_for.completion, Completion::Complete);
}

#[test]
fn docstrings_are_first_sentences_and_comments_skip_the_shebang() {
    let facts = extract("d.py", DOC_SRC);
    check_invariants(DOC_SRC, &facts);
    assert_eq!(facts.completion, Completion::Complete);
    assert!(facts.references.is_empty());
    let expect = [
        ("raw", SummarySource::Doc, "Raw doc."),
        ("binary", SummarySource::Signature, "def binary():"),
        ("escaped", SummarySource::Doc, "Line Two."),
        ("joined", SummarySource::Doc, "Hello."),
        ("wrapped", SummarySource::Doc, "Only one."),
        ("formatted", SummarySource::Signature, "def formatted():"),
    ];
    for (name, source, text) in expect {
        let summary = &node(&facts, name).fact.summary;
        assert_eq!(summary.source, source, "{name}");
        assert_eq!(summary.text, text, "{name}");
    }
    let shebang = extract("s.py", SHEBANG);
    assert_eq!(shebang.nodes[0].fact.summary.text, "First sentence.");
    assert_eq!(shebang.nodes[0].fact.summary.source, SummarySource::Doc);
    assert_eq!(node(&shebang, "f").fact.summary.text, "def f():");
    assert_eq!(shebang.completion, Completion::Complete);
}

#[test]
fn damaged_files_keep_earlier_facts_and_label_later_ones_partial() {
    let facts = extract("p.py", PARTIAL_SRC);
    check_invariants(PARTIAL_SRC, &facts);
    assert_eq!(names(&facts), ["p.py", "kept", "also"]);
    assert_eq!(node(&facts, "kept").precision, Precision::Structural);
    assert_eq!(node(&facts, "also").precision, Precision::Partial);
    assert_eq!(facts.nodes[0].precision, Precision::Structural);
    assert_eq!(facts.references.len(), 1);
    assert_eq!(facts.references[0].fact.name, "call");
    assert_eq!(facts.references[0].fact.scope, "also");
    assert_eq!(facts.references[0].precision, Precision::Partial);
    assert!(facts.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedString));
    let open = extract("t.py", TRIPLE_OPEN);
    assert_eq!(names(&open), ["t.py", "kept"]);
    assert_eq!(open.completion, Completion::Partial);
    assert!(open.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnterminatedString));
    assert!(open.nodes.iter().all(|item| item.fact.name != "swallowed"));
    let closed = extract("c.py", TRIPLE_CLOSED);
    assert_eq!(names(&closed), ["c.py"]);
    assert_eq!(call_names(&closed), ["real"]);
    assert_eq!(closed.completion, Completion::Complete);
    let paren = extract("b.py", PAREN_SRC);
    assert_eq!(names(&paren), ["b.py", "kept"]);
    assert_eq!(node(&paren, "kept").precision, Precision::Structural);
    assert_eq!(paren.completion, Completion::Partial);
    assert!(paren.diagnostics.iter().any(|item| item.reason == DiagnosticReason::UnbalancedBracket));
    assert!(paren.nodes.iter().all(|item| item.fact.name != "later"));
    let many = extract("e.py", &"else\n".repeat(80));
    assert_eq!(many.diagnostics.len(), LexedFile::MAX_DIAGNOSTICS);
    assert_eq!(many.completion, Completion::Partial);
    assert!(many.diagnostics.iter().all(|item| item.reason == DiagnosticReason::UnexpectedToken));
}

#[test]
fn indentation_newlines_unicode_and_ordinals_stay_bounded() {
    let crlf = extract("c.py", CRLF_SRC);
    assert_eq!(names(&crlf), ["c.py", "f"]);
    assert_eq!(call_names(&crlf), ["g"]);
    assert_eq!(crlf.references[0].fact.scope, "f");
    assert_eq!(crlf.completion, Completion::Complete);
    let tabs = extract("t.py", TABS_SRC);
    assert_eq!(names(&tabs), ["t.py", "f"]);
    assert_eq!(call_names(&tabs), ["g"]);
    assert_eq!(tabs.completion, Completion::Complete);
    let mixed = extract("m.py", MIXED_SRC);
    assert_eq!(names(&mixed), ["m.py", "f"]);
    assert_eq!(mixed.completion, Completion::Partial);
    assert!(mixed.diagnostics.iter().any(|item| item.reason == DiagnosticReason::MixedTabs));
    let dedent = extract("d.py", DEDENT_SRC);
    assert_eq!(names(&dedent), ["d.py", "f"]);
    assert_eq!(dedent.completion, Completion::Partial);
    assert!(dedent.diagnostics.iter().any(|item| item.reason == DiagnosticReason::InconsistentIndent));
    let unicode = extract("u.py", UNICODE_SRC);
    assert_eq!(names(&unicode), ["u.py", "café"]);
    assert!(node(&unicode, "café").fact.public);
    assert_eq!(unicode.completion, Completion::Complete);
    let ordinals = extract("o.py", ORDINALS);
    assert_eq!(ordinals.nodes[1].fact.id.as_str(), "s:o.py#f~1");
    assert_eq!(ordinals.nodes[2].fact.id.as_str(), "s:o.py#f~2");
    let bom = extract("b.py", BOM_SRC);
    assert_eq!(names(&bom), ["b.py", "f"]);
    assert_eq!(bom.completion, Completion::Complete);
}

#[test]
fn depth_and_fact_limits_refuse_with_a_positive_control() {
    let deep = PyLexer::new(Limits { max_depth: 2, ..Limits::default() });
    let bound = PyLexer::new(Limits { max_depth: 3, ..Limits::default() });
    assert!(matches!(
        deep.extract("d.py", ONELINER.as_bytes()),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    let accepted = bound.extract("d.py", ONELINER.as_bytes()).unwrap();
    assert_eq!(names(&accepted), ["d.py", "a", "a::b", "a::b::c"]);
    assert!(matches!(
        deep.extract("f.py", FNEST.as_bytes()),
        Err(LexError::TooDeep { limit: 2, .. })
    ));
    assert!(bound.extract("f.py", FNEST.as_bytes()).is_ok());
    let one = PyLexer::new(Limits { max_facts: 1, ..Limits::default() });
    let two = PyLexer::new(Limits { max_facts: 2, ..Limits::default() });
    let zero = PyLexer::new(Limits { max_facts: 0, ..Limits::default() });
    assert!(matches!(
        one.extract("i.py", b"import os\n"),
        Err(LexError::TooManyFacts { limit: 1 })
    ));
    assert!(two.extract("i.py", b"import os\n").is_ok());
    assert!(one.extract("e.py", b"").is_ok());
    assert!(matches!(
        zero.extract("e.py", b""),
        Err(LexError::TooManyFacts { limit: 0 })
    ));
}

#[test]
fn every_fixture_prefix_is_deterministic_and_bounded() {
    for source in FIXTURES {
        for end in 0..=source.len() {
            let bytes = &source.as_bytes()[..end];
            let first = PyLexer::default().extract("pkg/sample.py", bytes);
            let second = PyLexer::default().extract("pkg/sample.py", bytes);
            match (&first, &second) {
                (Ok(left), Ok(right)) => {
                    let text = std::str::from_utf8(bytes).unwrap();
                    check_invariants(text, left);
                    assert_eq!(left, right);
                }
                (Err(left), Err(right)) => assert_eq!(left.to_string(), right.to_string()),
                _ => panic!("non-deterministic prefix of length {end}"),
            }
            match first {
                Ok(_) => {}
                Err(LexError::InvalidUtf8(_)) => assert!(std::str::from_utf8(bytes).is_err()),
                Err(LexError::TooDeep { .. } | LexError::TooManyFacts { .. } | LexError::TooLarge { .. }) => {}
                Err(error) => panic!("unexpected {error}"),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        for path in ["f.py", "f.pyw", "f.pyi"] {
            let first = PyLexer::default().extract(path, &bytes);
            let second = PyLexer::default().extract(path, &bytes);
            match (first, second) {
                (Ok(left), Ok(right)) => {
                    let text = std::str::from_utf8(&bytes).unwrap();
                    check_invariants(text, &left);
                    prop_assert_eq!(left, right);
                }
                (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
                _ => prop_assert!(false, "non-deterministic outcome"),
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn truncated_fixture_prefixes_never_panic(index in 0..FIXTURES.len(), cut in 0usize..8_000) {
        let source = FIXTURES[index];
        let end = cut % (source.len() + 1);
        let bytes = &source.as_bytes()[..end];
        let first = PyLexer::default().extract("pkg/sample.py", bytes);
        let second = PyLexer::default().extract("pkg/sample.py", bytes);
        match (first, second) {
            (Ok(left), Ok(right)) => {
                let text = std::str::from_utf8(bytes).unwrap();
                check_invariants(text, &left);
                prop_assert_eq!(left, right);
            }
            (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
            _ => prop_assert!(false, "non-deterministic outcome"),
        }
    }

    #[test]
    fn python_shaped_text_is_deterministic(
        source in r##"([A-Za-z_]{1,3}|def |class |import |from |async |return |[ \n\t()\[\]{}:.,=*'\"#@]|é|数){0,60}"##
    ) {
        let first = PyLexer::default().extract("s.py", source.as_bytes());
        let second = PyLexer::default().extract("s.py", source.as_bytes());
        match (first, second) {
            (Ok(left), Ok(right)) => {
                check_invariants(&source, &left);
                prop_assert_eq!(left, right);
            }
            (Err(left), Err(right)) => prop_assert_eq!(left.to_string(), right.to_string()),
            _ => prop_assert!(false, "non-deterministic outcome"),
        }
    }
}
