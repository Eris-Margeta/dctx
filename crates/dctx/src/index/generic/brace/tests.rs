//! Tests for the member-level symbols of the brace family: one per language, plus the multi-line headers they depend on.

use super::symbols;

fn named(tag: &str, text: &str) -> Vec<(String, String)> {
    symbols(tag, text).into_iter().map(|symbol| (symbol.name, symbol.kind)).collect()
}

fn expect(found: Vec<(String, String)>, wanted: &[(&str, &str)]) {
    let wanted: Vec<(String, String)> = wanted.iter().map(|(name, kind)| (name.to_string(), kind.to_string())).collect();
    assert_eq!(found, wanted);
}

#[test]
fn scala_vals_vars_aliases_enum_cases_and_nested_defs_are_symbols() {
    let text = "object Cfg {\n  val port = 8080\n  var retries: Int = 3\n  lazy val name = \"x\"\n  type Alias = Int\n  def outer(): Int = {\n    val local = 1\n    def inner(x: Int) = x + 1\n    inner(1)\n  }\n}\nenum Color {\n  case Red, Green\n}\n";
    expect(
        named("scala", text),
        &[
            ("Cfg", "type"),
            ("Cfg::port", "const"),
            ("Cfg::retries", "var"),
            ("Cfg::name", "const"),
            ("Cfg::Alias", "type"),
            ("Cfg::outer", "fn"),
            ("Cfg::outer::inner", "fn"),
            ("Color", "type"),
            ("Color::Red", "const"),
            ("Color::Green", "const"),
        ],
    );
}

#[test]
fn dart_fields_constructors_accessors_enum_mixin_and_extension_are_symbols() {
    let text = "const kMax = 10;\nclass Box {\n  final int size;\n  Box(this.size);\n  Box.origin() : size = 0;\n  int get area => size * size;\n  set side(int v) {}\n  static Box empty() => Box(0);\n}\nenum Mode { fast, slow }\nmixin Logs {}\nextension BoxExt on Box {\n  int twice() => size * 2;\n}\n";
    expect(
        named("dart", text),
        &[
            ("kMax", "const"),
            ("Box", "type"),
            ("Box::size", "field"),
            ("Box::Box", "fn"),
            ("Box::origin", "fn"),
            ("Box::area", "fn"),
            ("Box::side", "fn"),
            ("Box::empty", "fn"),
            ("Mode", "type"),
            ("Mode::fast", "const"),
            ("Mode::slow", "const"),
            ("Logs", "type"),
            ("BoxExt", "type"),
            ("BoxExt::twice", "fn"),
        ],
    );
}

#[test]
fn zig_values_fields_enum_values_and_test_blocks_are_symbols() {
    let text = "const limit = 5;\nconst std = @import(\"std\");\nconst Point = struct {\n    x: i32,\n    y: i32,\n    pub const origin = 0;\n    pub fn len(self: Point) i32 {\n        return self.x;\n    }\n};\nconst Color = enum {\n    red,\n    green = 2,\n};\nvar counter: u32 = 0;\ntest \"point len\" {\n    _ = limit;\n}\n";
    expect(
        named("zig", text),
        &[
            ("limit", "const"),
            ("Point", "type"),
            ("Point::x", "field"),
            ("Point::y", "field"),
            ("Point::origin", "const"),
            ("Point::len", "fn"),
            ("Color", "type"),
            ("Color::red", "const"),
            ("Color::green", "const"),
            ("counter", "var"),
            ("test \"point len\"", "fn"),
        ],
    );
}

#[test]
fn solidity_state_variables_structs_enums_errors_and_split_headers_are_symbols() {
    let text = "pragma solidity ^0.8.0;\ncontract Vault {\n    uint256 public total;\n    uint256 constant MAX = 10;\n    mapping(address => uint256) balances;\n    struct Entry { uint256 amount; address who; }\n    enum State { Open, Closed }\n    error Insufficient();\n    modifier onlyOwner() { _; }\n    function deposit(\n        uint256 a\n    ) public\n        onlyOwner\n    {\n        total += a;\n    }\n}\n";
    expect(
        named("solidity", text),
        &[
            ("Vault", "type"),
            ("Vault::total", "var"),
            ("Vault::MAX", "const"),
            ("Vault::balances", "var"),
            ("Vault::Entry", "type"),
            ("Vault::Entry::amount", "field"),
            ("Vault::Entry::who", "field"),
            ("Vault::State", "type"),
            ("Vault::State::Open", "const"),
            ("Vault::State::Closed", "const"),
            ("Vault::Insufficient", "fn"),
            ("Vault::onlyOwner", "fn"),
            ("Vault::deposit", "fn"),
        ],
    );
    let found = symbols("solidity", text);
    let deposit = found.iter().find(|symbol| symbol.name == "Vault::deposit").expect("deposit");
    assert_eq!(deposit.lines, [10, 16]);
}

#[test]
fn gleam_constants_are_symbols() {
    let text = "pub const limit = 5\npub const name: String = \"x\"\nconst Hidden = 1\npub fn main() {\n  1\n}\npub type Shape {\n  Circle\n}\n";
    expect(named("gleam", text), &[("limit", "const"), ("name", "const"), ("Hidden", "const"), ("main", "fn"), ("Shape", "type")]);
}

#[test]
fn groovy_fields_in_a_class_are_symbols_and_script_variables_are_not() {
    let text = "class Svc {\n  static final String LIMIT = 'x'\n  private int count = 0\n  def label = 1\n  String title\n  def call(String a) {\n    return a\n  }\n}\ndef top = 1\n";
    expect(
        named("groovy", text),
        &[("Svc", "type"), ("Svc::LIMIT", "const"), ("Svc::count", "field"), ("Svc::label", "field"), ("Svc::title", "field"), ("Svc::call", "fn")],
    );
}

#[test]
fn an_open_parenthesis_does_not_turn_parameters_into_fields() {
    let zig = "const P = struct {\n    pub fn f(\n        a: i32,\n        b: i32,\n    ) void {\n    }\n    z: i32,\n};\n";
    expect(named("zig", zig), &[("P", "type"), ("P::f", "fn"), ("P::z", "field")]);
    let found = symbols("zig", zig);
    assert_eq!(found[1].lines, [2, 6]);
    // A stray parenthesis ends its effect after a while instead of hiding the rest of the file.
    let stray = format!("fn a( \n{}fn b() void {{\n}}\n", "x\n".repeat(130));
    assert!(named("zig", &stray).iter().any(|(name, _)| name == "b"));
}
