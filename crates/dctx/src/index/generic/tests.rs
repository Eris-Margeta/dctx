//! Tests for the generic heuristic extractor: the six brace languages, the denylist, masking and malformed input.

use super::{brace, handles, mask, symbols};
use crate::index::extract::{extract_structural_refs, language_of};

const TAGS: &[&str] = &["scala", "dart", "zig", "solidity", "gleam", "groovy", "perl", "powershell"];

fn named(tag: &str, text: &str) -> Vec<(String, String)> {
    symbols(tag, text).into_iter().map(|symbol| (symbol.name, symbol.kind)).collect()
}

fn pair(name: &str, kind: &str) -> (String, String) {
    (name.to_string(), kind.to_string())
}

#[test]
fn every_brace_language_finds_its_types_and_functions() {
    let scala = "object Main {\n  def run(x: Int): Int = helper(x)\n  def helper(y: Int): Int = {\n    if (y > 0) { y + 1 } else { 0 }\n  }\n}\nclass Box(val v: Int) {\n  def get: Int = v\n}\ntrait Shape\n";
    assert_eq!(
        named("scala", scala),
        [pair("Main", "type"), pair("Main::run", "fn"), pair("Main::helper", "fn"), pair("Box", "type"), pair("Box::v", "const"), pair("Box::get", "fn"), pair("Shape", "type")]
    );
    let dart = "class Greeter {\n  String greet(String n) {\n    if (n.isEmpty) { return \"x\"; }\n    return \"hi\";\n  }\n}\nint main() {\n  return 0;\n}\nFuture<void> later() async {}\n";
    assert_eq!(named("dart", dart), [pair("Greeter", "type"), pair("Greeter::greet", "fn"), pair("main", "fn"), pair("later", "fn")]);
    let zig = "const std = @import(\"std\");\npub fn main() void {\n    helper();\n}\nfn helper() void {\n    while (true) {}\n}\nconst Point = struct { x: i32 };\n";
    assert_eq!(named("zig", zig), [pair("main", "fn"), pair("helper", "fn"), pair("Point", "type"), pair("Point::x", "field")]);
    let sol = "contract Token {\n    function transfer(address to, uint v) public returns (bool) {\n        return true;\n    }\n    event Sent(address to);\n}\n";
    assert_eq!(named("solidity", sol), [pair("Token", "type"), pair("Token::transfer", "fn"), pair("Token::Sent", "fn")]);
    let gleam = "pub fn main() {\n  helper()\n}\nfn helper() {\n  1\n}\npub type Shape {\n  Circle\n}\n";
    assert_eq!(named("gleam", gleam), [pair("main", "fn"), pair("helper", "fn"), pair("Shape", "type")]);
    let powershell = "function Get-Config {\n    param([string]$Path)\n    return Read-File -Path $Path\n}\n\nfunction Read-File {\n    param($Path)\n    Get-Content $Path\n}\n\nclass Deployer {\n    [string] Run() {\n        return Get-Config -Path \"x\"\n    }\n}\n";
    assert_eq!(named("powershell", powershell), [pair("Get-Config", "fn"), pair("Read-File", "fn"), pair("Deployer", "type"), pair("Deployer::Run", "fn")]);
    let groovy = "class Svc {\n  def call(String a) {\n    return a\n  }\n}\ndef x = 1\n";
    assert_eq!(named("groovy", groovy), [pair("Svc", "type"), pair("Svc::call", "fn")]);
    let perl = "package Shop::Cart;\nsub total {\n    my ($self) = @_;\n    return add_tax($self->{sum}) + round_it $self->{sum};\n}\nsub add_tax {\n    my ($x) = @_;\n    return $x * 1.2;\n}\nsub round_it { return int(shift) }\n";
    assert_eq!(named("perl", perl), [pair("Shop::Cart", "type"), pair("Shop::Cart::total", "fn"), pair("Shop::Cart::add_tax", "fn"), pair("Shop::Cart::round_it", "fn")]);
}

#[test]
fn symbols_carry_lines_signature_and_the_sig_summary() {
    let text = "object Main {\n  def run(x: Int): Int = x\n}\n";
    let found = symbols("scala", text);
    assert_eq!(found[0].lines, [1, 3]);
    assert_eq!(found[0].signature, "object Main");
    assert_eq!(found[1].lines, [2, 2]);
    assert_eq!(found[1].signature, "def run(x: Int): Int = x");
    assert_eq!(found[1].summary, found[1].signature);
    assert_eq!(found[1].summary_source, "sig");
    assert_eq!(found[1].span, [14, 40]);
}

#[test]
fn a_brace_on_its_own_line_still_opens_the_body() {
    let found = symbols("solidity", "contract A\n{\n  function f() public\n  {\n  }\n}\n");
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["A", "A::f"]);
    assert_eq!(found[0].lines, [1, 6]);
    assert_eq!(found[1].lines, [3, 5]);
}

#[test]
fn denylisted_keywords_are_never_symbols() {
    for word in ["if", "else", "for", "while", "match", "switch", "return", "case", "catch", "when", "do", "try"] {
        for (tag, text) in [
            ("scala", format!("class K {{\n  def {word}(x: Int) = x\n}}\n")),
            ("groovy", format!("def {word}(a) {{\n}}\n")),
            ("gleam", format!("fn {word}() {{\n}}\n")),
            ("zig", format!("fn {word}() void {{\n}}\n")),
            ("solidity", format!("contract {word} {{\n}}\nfunction {word}() public {{}}\n")),
            ("dart", format!("class {word} {{\n}}\nint {word}() {{}}\n{word} foo() {{}}\n")),
            ("powershell", format!("function {word} {{\n}}\n")),
        ] {
            let found = named(tag, &text);
            assert!(found.iter().all(|(name, _)| !name.ends_with(word) || name == "K"), "{tag} {word}: {found:?}");
        }
    }
}

#[test]
fn comments_and_strings_hide_declarations_and_keep_offsets() {
    let text = "// def hidden()\n/* class Hidden {\n def nested() } */\nval s = \"def fake() = 1\"\n/* a /* b */ def still_hidden() */\ndef shown() = 1\n";
    let masked = mask(text, &brace::syntax_of("scala").expect("scala"));
    assert_eq!(masked.len(), text.len());
    assert_eq!(masked.matches('\n').count(), text.matches('\n').count());
    assert_eq!(named("scala", text), [pair("s", "const"), pair("shown", "fn")]);
    let multibyte = "// ünïcode \"quote\ndef ok() = 'é'\n";
    assert_eq!(mask(multibyte, &brace::syntax_of("scala").expect("scala")).len(), multibyte.len());
    assert_eq!(named("scala", multibyte), [pair("ok", "fn")]);
}

#[test]
fn malformed_input_never_panics_and_stays_bounded() {
    let samples = [
        "",
        "{{{{ class A { def f() { }",
        "}}}} class B {{ def g(",
        "class C { def h() = \"unterminated\n def i() = 1",
        "/* never closed class D { def j() = 1 }",
        "\"\"\" never closed triple def k() = 1",
        "def l(\u{1F600}) = '\u{e9}\n@Ann( unclosed class M {",
        "private[ class N\n",
        "class\n",
        "def\n",
        "const = struct\n",
        "const Q = packed",
        "Map<String, List<int> a() {",
        "Map<<<<<>>>>> a() {",
        "int a\u{e9}(",
        "\\\\ zig line\nfn z() void {\n",
        "\u{0}\u{1}\r\n\r\nclass R {\r\n  def s() = 1\r\n}\r\n",
        "function Get-Config {",
    ];
    for tag in TAGS {
        for sample in samples {
            let _ = symbols(tag, sample);
        }
        let deep = format!("class D {} {}", "{".repeat(5000), "}".repeat(10));
        let _ = symbols(tag, &deep);
        assert!(symbols(tag, "").is_empty());
    }
    assert!(symbols("unknown", "class A {}").is_empty());
}

#[test]
fn extensions_dispatch_to_the_generic_extractor() {
    for (path, tag) in [
        ("a.scala", "scala"),
        ("a.sc", "scala"),
        ("a.dart", "dart"),
        ("a.zig", "zig"),
        ("a.sol", "solidity"),
        ("a.gleam", "gleam"),
        ("a.pl", "perl"),
        ("a.pm", "perl"),
        ("a.t", "perl"),
        ("a.psgi", "perl"),
        ("a.groovy", "groovy"),
        ("a.ps1", "powershell"),
        ("build.gradle", "groovy"),
        ("A.SCALA", "scala"),
    ] {
        assert_eq!(language_of(path), tag, "{path}");
        assert!(handles(tag));
    }
    assert!(!handles("python") && !handles("opaque"));
    let (found, refs) = extract_structural_refs("src/Main.scala", b"object Main {\n  def run() = 1\n}\n");
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["Main", "Main::run"]);
    assert!(refs.is_empty());
    assert!(extract_structural_refs("a.scala", &[0xff]).0.is_empty());
}
