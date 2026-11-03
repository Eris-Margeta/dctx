// SPDX-License-Identifier: Apache-2.0
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
fn perl_packages_and_subs_are_symbols() {
    let text = "package Shop::Cart;\nsub total {\n    my ($self) = @_;\n    return add_tax($self->{sum}) + round_it $self->{sum};\n}\nsub add_tax {\n    my ($x) = @_;\n    return $x * 1.2;\n}\nsub round_it { return int(shift) }\n";
    expect(named("perl", text), &[("Shop::Cart", "type"), ("Shop::Cart::total", "fn"), ("Shop::Cart::add_tax", "fn"), ("Shop::Cart::round_it", "fn")]);
}

#[test]
fn perl_pod_masked() {
    let text = "package P;\n# sub hidden(1)\nsub run {\n    my $c = P->new;\n    print $c->total;\n    &help;\n    my $x = helper(1);\n}\nsub help {\n    1\n}\n=pod\n\nsub in_pod { }\n\n=cut\n\n1;\n";
    let names: Vec<_> = symbols("perl", text).into_iter().map(|symbol| symbol.name).collect();
    assert_eq!(names, ["P", "P::run", "P::help"]);
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
fn powershell_symbols_and_methods_are_recognized() {
    let text = "class Deployer {\n  [string] Run() {\n    return $null\n  }\n}\nfunction Get-Config {\n}\nfunction Read-File {\n}\nenum Status {\n  Good\n  Bad\n}\n";
    expect(
        named("powershell", text),
        &[("Deployer", "type"), ("Deployer::Run", "fn"), ("Get-Config", "fn"), ("Read-File", "fn"), ("Status", "type"), ("Status::Good", "const"), ("Status::Bad", "const")]
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

/// Fansi's `ColorCategory`: a second parameter list on its own line, then `extends .. {`; its `val` parameter is a member.
#[test]
fn scala_class_params_split_headers_and_defs_deep_in_a_body_are_symbols() {
    let text = "abstract class ColorCategory(offset: Int, width: Int, val colorCode: Int)\n                            (implicit catName: sourcecode.Name)\n                             extends Category (offset, width)(catName){\n  private[this] def True0(r: Int) = {\n    r\n  }\n}\nobject Str {\n  def apply(raw: CharSequence): Str = {\n    while(i < 1){\n      if (c == 1) {\n        def isDigit(index: Int) = {\n          index < 3\n        }\n      }\n    }\n  }\n}\nobject FansiTests extends TestSuite{\n  val tests = Tests{\n    test(\"x\"){\n      def tabulate(all: Seq[Int]) = {\n        all\n      }\n    }\n  }\n}\n";
    expect(
        named("scala", text),
        &[
            ("ColorCategory", "type"),
            ("ColorCategory::colorCode", "const"),
            ("ColorCategory::True0", "fn"),
            ("Str", "type"),
            ("Str::apply", "fn"),
            ("Str::apply::isDigit", "fn"),
            ("FansiTests", "type"),
            ("FansiTests::tests", "const"),
            ("FansiTests::tests::tabulate", "fn"),
        ],
    );
    let found = symbols("scala", text);
    assert_eq!(found[0].lines, [1, 7]);
    assert_eq!(found[5].lines, [12, 14]);
}

/// zig-args shapes: a type declared inside a function or a test, the members of a returned `struct`, and anonymous field types.
#[test]
fn zig_types_inside_functions_returned_structs_and_anonymous_field_types_are_symbols() {
    let text = "fn parseInternal() void {\n    while (true) {\n        const Pair = struct {\n            name: []const u8,\n        };\n    }\n    var found = false;\n}\npub fn Parser(comptime Generic: type) type {\n    return struct {\n        const Self = @This();\n        arena: u32,\n        pub fn deinit(self: Self) void {\n            _ = self;\n        }\n    };\n}\nconst Verbs = union(enum) {\n    compact: struct {\n        host: ?[]const u8 = null,\n        mode: enum { default, special } = .default,\n    },\n    forward: void,\n};\ntest \"full help\" {\n    const Options = struct {\n        boolflag: bool = false,\n    };\n}\n";
    expect(
        named("zig", text),
        &[
            ("parseInternal", "fn"),
            ("parseInternal::Pair", "type"),
            ("parseInternal::Pair::name", "field"),
            ("Parser", "fn"),
            ("Parser::arena", "field"),
            ("Parser::deinit", "fn"),
            ("Verbs", "type"),
            ("Verbs::compact", "field"),
            ("Verbs::compact::host", "field"),
            ("Verbs::compact::mode", "field"),
            ("Verbs::compact::mode::default", "const"),
            ("Verbs::compact::mode::special", "const"),
            ("Verbs::forward", "field"),
            ("test \"full help\"", "fn"),
            ("test \"full help\"::Options", "type"),
            ("test \"full help\"::Options::boolflag", "field"),
        ],
    );
    let found = symbols("zig", text);
    assert_eq!(found[1].lines, [3, 5]);
    assert_eq!(found[7].lines, [19, 22]);
}
