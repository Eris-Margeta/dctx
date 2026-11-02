//! Call references of the brace family: Scala, Dart, Zig, Solidity, Gleam, Groovy and Perl.
//! Each function symbol's span is scanned on the comment- and string-masked source for `name(` call sites.
//! The references are raw: `refs.rs` resolves them (same file first, then a unique project name) and drops ambiguous ones.
//! Heuristic by design: no types, no receivers; a call is a bare identifier followed by an opening parenthesis, except in
//! Perl, where a bareword with an argument and `&name` also count, never after `->`, `sub`, `$`, `@` or `%`.

use std::collections::{HashMap, HashSet};

use dctx_map_model::{EdgeKind, NodeKind};

use super::{brace, is_denied, mask};
use crate::index::extract::Symbol;
use crate::index::refs::RawRef;
use crate::index::write_stable_id;

/// Words that look like a call (`word (`) but are control flow, a declaration keyword or a language operator.
const NOT_CALLS: &[&str] = &[
    "if", "while", "for", "switch", "match", "return", "catch", "function", "fn", "def", "new", "sizeof", "require", "emit", "assert", "filter",
    "returns", "modifier", "event", "constructor", "import", "extends", "with", "this", "super", "param",
];

/// Words after which the next identifier is a declared name, so `name(` there is a declaration, not a call.
const DECLARES: &[&str] = &["def", "fn", "function", "event", "modifier", "constructor", "class", "object", "trait", "interface", "contract", "library", "type", "enum"];

/// Raw `calls` references of every function symbol of `text`. Anything but a brace-family tag yields none.
///
/// `src` is the caller's stable id exactly as the index writes it (`s:<path>#<name>`, `~N` for a repeated name) and
/// `scope` is the caller's qualified name, so `refs.rs` looks callees up from the caller's own scope outward.
pub(super) fn references(language: &str, path: &str, text: &str, symbols: &[Symbol]) -> Vec<RawRef> {
    let Some(syntax) = brace::syntax_of(language) else {
        return Vec::new();
    };
    let masked = mask(text, &syntax);
    let mut seen: HashMap<&str, u32> = HashMap::new();
    let mut refs = Vec::new();
    // Scala: `new X(..)` builds a value; it calls a def only when the file defines a def named `X`.
    let defined: HashSet<&str> = symbols.iter().filter(|symbol| symbol.kind == "fn").map(|symbol| symbol.name.rsplit("::").next().unwrap_or(&symbol.name)).collect();
    // For PowerShell: build a case-insensitive map from lowercase name to the declared spelling.
    let defined_powershell: Option<HashMap<String, &str>> = if language == "powershell" {
        Some(defined.iter().map(|name| (name.to_lowercase(), *name)).collect())
    } else {
        None
    };
    let skip_new = (language == "scala").then_some(&defined);
    // Perl: only a name that is a sub defined in the file or the project is a call.
    let is_perl = language == "perl";
    for symbol in symbols {
        let count = seen.entry(symbol.name.as_str()).or_insert(0);
        *count += 1;
        let ordinal = if *count == 1 { 0 } else { *count };
        if symbol.kind != "fn" || symbol.graph != NodeKind::Symbol {
            continue;
        }
        let Some(body) = body_of(&masked, symbol.span) else {
            continue;
        };
        let mut src = String::new();
        write_stable_id(&mut src, b's', path, &symbol.name, ordinal);
        let own = symbol.name.rsplit("::").next().unwrap_or(&symbol.name);
        for name in call_names(body, own, skip_new, language, is_perl.then_some(&defined)) {
            // For PowerShell: case-insensitive match against defined functions, keep declared spelling.
            let callee = if language == "powershell" {
                if let Some(map) = &defined_powershell {
                    map.get(&name.to_lowercase()).copied().unwrap_or(name)
                } else {
                    name
                }
            } else {
                name
            };
            refs.extend(RawRef::new(&src, EdgeKind::Calls, callee, &symbol.name, None));
        }
    }
    if language == "powershell" {
        let mut refs_sorted = refs;
        refs_sorted.sort_by(|a, b| a.scope.cmp(&b.scope));
        refs_sorted
    } else {
        refs
    }
}

/// The masked source of one symbol's byte span, or `None` when the span is empty or not on character boundaries.
fn body_of(masked: &str, span: [u64; 2]) -> Option<&str> {
    let start = usize::try_from(span[0]).ok()?;
    let end = usize::try_from(span[1]).ok()?;
    if start >= end {
        return None;
    }
    masked.get(start..end)
}

/// True for a byte that can be part of an identifier. Non-ASCII bytes count, so words never split a character.
/// For PowerShell, `-` is also allowed in names (e.g. `Get-Config`).
fn is_word(byte: u8, language: &str) -> bool {
    let is_base = byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80;
    let is_powershell = language == "powershell";
    is_base || (is_powershell && byte == b'-')
}

/// Called names in `body`, in source order and without repeats.
///
/// A call is an identifier followed by `(`, with one space or tab allowed between. Keywords, declarations, `@builtin(`
/// forms and the function's own name on its declaration line (the first line of `body`) are not calls. With `skip_new`
/// (Scala) a type named right after `new`, dotted path included, is not a call unless it is one of the given def names.
/// With `perl` (Perl) a bareword `name` followed by whitespace and an argument and `&name` are calls too, each only when
/// the name is one of the given sub names, and never after `->`, `sub`, `$`, `@` or `%`.
/// For PowerShell, calls are case-insensitive and can occur without parentheses at command position.
fn call_names<'a>(body: &'a str, own: &str, skip_new: Option<&HashSet<&str>>, language: &str, perl: Option<&HashSet<&str>>) -> Vec<&'a str> {
    if let Some(defined) = perl {
        return brace::perl_call_names(body, own, defined);
    }
    let bytes = body.as_bytes();
    let first_line_end = body.find('\n').unwrap_or(body.len());
    let mut found: Vec<&str> = Vec::new();
    let mut previous: Option<(&str, usize)> = None;
    // End of the last `new` word, and end of the last word of the type path that followed it.
    let mut after_new: Option<usize> = None;
    let mut path_end: Option<usize> = None;
    let mut at = 0usize;
    let is_powershell = language == "powershell";
    while at < bytes.len() {
        if !is_word(bytes[at], language) {
            at += 1;
            continue;
        }
        let end = bytes[at..].iter().position(|byte| !is_word(*byte, language)).map_or(bytes.len(), |offset| at + offset);
        let word = &body[at..end];
        let declared = previous.is_some_and(|(prior, prior_end)| DECLARES.contains(&prior) && body[prior_end..at].trim().is_empty());
        previous = Some((word, end));
        let in_new = after_new.is_some_and(|stop| body[stop..at].trim().is_empty()) || path_end.is_some_and(|stop| &body[stop..at] == ".");
        after_new = (word == "new").then_some(end);
        path_end = in_new.then_some(end);
        let new_type = in_new && skip_new.is_some_and(|defs| !defs.contains(word));
        let open = end + usize::from(matches!(bytes.get(end), Some(b' ' | b'\t')));
        // A call is `(` after the word, OR for PowerShell a word at command position.
        // PowerShell calls are often without parentheses; the callee name is at command position:
        // after a newline (line start with optional indentation), or after `=`, `|`, `(`, `{`, `}`, `;`, `)`,
        // or after an alphabetic word (control flow keyword like `return`, `if`, `else`).
        // Exclude parameters (`-Name`) and variables (`$Name`) which are not calls.
        let is_powershell_param_or_var = is_powershell && (word.starts_with('-') || word.starts_with('$'));
        let calls = if is_powershell && !is_powershell_param_or_var {
            let has_paren = bytes.get(open) == Some(&b'(');
            let at_command_position = {
                // Scan backwards from `at` to find the last non-whitespace character before this word.
                let mut before = at;
                while before > 0 && (bytes[before - 1] == b' ' || bytes[before - 1] == b'\t') {
                    before -= 1;
                }
                if before == 0 {
                    // Start of body: command position.
                    true
                } else {
                    let last = bytes[before - 1];
                    matches!(last, b'\n' | b'=' | b'|' | b'(' | b'{' | b'}' | b';' | b')')
                        || last.is_ascii_alphabetic()
                }
            };
            has_paren || at_command_position
        } else {
            bytes.get(open) == Some(&b'(') && !word.starts_with(|c: char| c.is_ascii_digit())
        };
        let builtin = at > 0 && bytes[at - 1] == b'@';
        let own_declaration = at < first_line_end && word == own;
        if calls && !declared && !new_type && !builtin && !own_declaration && !NOT_CALLS.contains(&word) && !is_denied(word) && !found.contains(&word) {
            found.push(word);
        }
        at = end;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::generic::symbols;

    fn refs_of(tag: &str, path: &str, text: &str) -> Vec<RawRef> {
        references(tag, path, text, &symbols(tag, text))
    }

    /// (caller name, callee) pairs.
    fn pairs(tag: &str, text: &str) -> Vec<(String, String)> {
        refs_of(tag, "a.src", text).into_iter().map(|reference| (reference.scope, reference.name)).collect()
    }

    fn pair(caller: &str, callee: &str) -> (String, String) {
        (caller.to_string(), callee.to_string())
    }

    #[test]
    fn every_brace_language_links_a_caller_to_its_callee() {
        let scala = "object M {\n  def scRun(x: Int): Int = scHelp(x)\n  def scHelp(y: Int): Int = y + 1\n}\n";
        assert_eq!(pairs("scala", scala), [pair("M::scRun", "scHelp")]);
        let dart = "int dHelp(int x) {\n  return x;\n}\nint dRun() {\n  return dHelp(1);\n}\n";
        assert_eq!(pairs("dart", dart), [pair("dRun", "dHelp")]);
        let zig = "fn zHelp() u32 {\n    return 1;\n}\npub fn zRun() u32 {\n    return zHelp();\n}\n";
        assert_eq!(pairs("zig", zig), [pair("zRun", "zHelp")]);
        let sol = "contract T {\n    function solHelp() internal returns (uint) {\n        return 1;\n    }\n    function solRun() public returns (uint) {\n        return solHelp();\n    }\n}\n";
        assert_eq!(pairs("solidity", sol), [pair("T::solRun", "solHelp")]);
        let gleam = "fn gHelp() {\n  1\n}\npub fn gRun() {\n  gHelp()\n}\n";
        assert_eq!(pairs("gleam", gleam), [pair("gRun", "gHelp")]);
        let powershell = "function Get-Config {\n    param([string]$Path)\n    return Read-File -Path $Path\n}\n\nfunction Read-File {\n    param($Path)\n    Get-Content $Path\n}\n\nclass Deployer {\n    [string] Run() {\n        return Get-Config -Path \"x\"\n    }\n}\n";
        // In PowerShell, calls without parentheses at command position are detected:
        // - Deployer::Run calls Get-Config (at command position within the method body)
        // - Get-Config calls Read-File (at command position within its body)
        // - Read-File calls Get-Content (at command position within its body)
        assert_eq!(pairs("powershell", powershell), [pair("Deployer::Run", "Get-Config"), pair("Get-Config", "Read-File"), pair("Read-File", "Get-Content")]);
        let groovy = "class G {\n  def gvHelp(a) {\n    return a\n  }\n  def gvRun(a) {\n    return gvHelp (a)\n  }\n}\n";
        assert_eq!(pairs("groovy", groovy), [pair("G::gvRun", "gvHelp")]);
    }

    /// Perl: a `package` holds its `sub`s and a call is `name(..)`, a bareword with an argument, or `&name`.
    #[test]
    fn perl_packages_qualify_subs_and_calls_link_callers_to_callees() {
        let text = "package Shop::Cart;\nsub total {\n    my ($self) = @_;\n    return add_tax($self->{sum}) + round_it $self->{sum};\n}\nsub add_tax {\n    return 0;\n}\nsub round_it { return int(shift) }\n";
        let found = symbols("perl", text);
        let names: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.kind.as_str())).collect();
        assert_eq!(
            names,
            [("Shop::Cart", "type"), ("Shop::Cart::total", "fn"), ("Shop::Cart::add_tax", "fn"), ("Shop::Cart::round_it", "fn")]
        );
        assert_eq!(
            pairs("perl", text),
            [pair("Shop::Cart::total", "add_tax"), pair("Shop::Cart::total", "round_it")]
        );
    }

    /// Perl: `->` methods and variables are not calls, `&name` is, undefined names are dropped, and POD and comments mask.
    #[test]
    fn perl_methods_amps_and_pod_are_not_calls_and_sub_names_are_filtered() {
        let text = "package P;\n# sub hidden(1)\nsub run {\n    my $c = P->new;\n    print $c->total;\n    &help;\n    my $x = helper(1);\n}\nsub help {\n    1\n}\n=pod\n\nsub in_pod { }\n\n=cut\n\n1;\n";
        let names: Vec<_> = symbols("perl", text).into_iter().map(|symbol| symbol.name).collect();
        assert_eq!(names, ["P", "P::run", "P::help"]);
        // `P->new` and `$c->total` are methods, `helper` is not a sub, so only `&help` is a call.
        assert_eq!(pairs("perl", text), [pair("P::run", "help")]);
    }

    #[test]
    fn the_source_id_is_spelled_as_the_index_writes_it() {
        let text = "object M {\n  def one() = two()\n  def two() = 1\n}\nobject N {\n  def one() = two()\n  def two() = 1\n}\n";
        let found = refs_of("scala", "src/m.scala", text);
        let sources: Vec<_> = found.iter().map(|reference| (reference.src.as_str(), reference.scope.as_str(), reference.name.as_str())).collect();
        assert_eq!(sources, [("s:src/m.scala#M::one", "M::one", "two"), ("s:src/m.scala#N::one", "N::one", "two")]);
        assert!(found.iter().all(|reference| reference.kind == EdgeKind::Calls && reference.alias.is_none()));
        let twice = "def f() = g()\ndef f() = h()\n";
        let found = refs_of("scala", "t.scala", twice);
        assert_eq!(found.iter().map(|reference| reference.src.as_str()).collect::<Vec<_>>(), ["s:t.scala#f", "s:t.scala#f~2"]);
    }

    #[test]
    fn keywords_are_not_calls() {
        let body = "x = if (a) 1 else 2\nwhile (b) {}\nfor (i <- 1) {}\nswitch (c) {}\nmatch (d) {}\nreturn (e)\ncatch (f) {}\nfunction (g) {}\nfn (h) {}\ndef (i) {}\nnew (j)\nsizeof(k)\nrequire(l)\nemit (m)\nassert(n)\nreal (1)\nother(2)\n";
        assert_eq!(call_names(body, "none", None, "none", None), ["real", "other"]);
        let sol = "contract C {\n    function f() public returns (uint) {\n        emit Sent(1);\n        require(ok);\n        return g();\n    }\n}\n";
        // `emit Name(..)` names an event, which is a function symbol: the event is called, `emit` is not.
        assert_eq!(pairs("solidity", sol), [pair("C::f", "Sent"), pair("C::f", "g")]);
    }

    #[test]
    fn a_definition_never_calls_itself_on_its_declaration_line() {
        assert!(pairs("zig", "pub fn zRun() u32 {\n    return 1;\n}\n").is_empty());
        assert!(pairs("dart", "int dRun(int x) {\n  return x;\n}\n").is_empty());
        assert_eq!(pairs("scala", "object M {\n  def run(x: Int): Int = other(run2(x))\n}\n"), [pair("M::run", "other"), pair("M::run", "run2")]);
        // A nested declaration is not a call either.
        assert!(pairs("gleam", "fn outer() {\n  fn inner() {\n    1\n  }\n}\n").is_empty());
    }

    #[test]
    fn masked_comments_and_strings_produce_no_calls() {
        let text = "def run(x: Int): Int = {\n  // hidden(1)\n  /* nested(2) /* inner(3) */ still(4) */\n  val s = \"text(5) \\\" quoted(6)\"\n  \"\"\"triple(7)\n  more(8)\"\"\"\n  shown(9)\n}\n";
        assert_eq!(pairs("scala", text), [pair("run", "shown")]);
        let zig = "fn zRun() void {\n    // zHidden();\n    const s = \"zStr()\";\n    zShown();\n    @import(\"std\");\n}\n";
        assert_eq!(pairs("zig", zig), [pair("zRun", "zShown")]);
    }

    #[test]
    fn types_events_and_odd_input_stay_quiet() {
        assert!(pairs("scala", "class A(x: Int) {\n  val y = make(x)\n}\n").is_empty());
        assert!(refs_of("unknown", "a.src", "def f() = g()").is_empty());
        for sample in ["", "def f(", "fn f() {", "def f() = \u{e9}\u{1F600}(1)\n", "\u{0}\r\n\r\ndef g() = h ()\r\n"] {
            for tag in ["scala", "dart", "zig", "solidity", "gleam", "groovy"] {
                let _ = refs_of(tag, "a.src", sample);
            }
        }
        assert_eq!(pairs("scala", "def g() = h ()\r\n"), [pair("g", "h")]);
    }

    /// The corpus file the benchmark missed: modifiers before every keyword, brace-less expression bodies, a call on the declaration line.
    const FANSI: &str = "package fansi\n\nclass Str private(private val chars: Array[Char], private val colors: Array[Int]) {\n  require(chars.length == colors.length)\n  override def hashCode() = java.util.Arrays.hashCode(chars)\n  def length = chars.length\n  def splitAt(index: Int) = (\n    new Str(chars, colors),\n    new Str(chars, colors)\n  )\n  def firstHalf: Str = splitAt(length / 2)._1\n  def render = {\n    val b = new StringBuilder\n    b.toString\n  }\n}\nobject Str {\n  implicit def implicitApply(raw: CharSequence): Str = apply(raw)\n  def apply(raw: CharSequence): Str = parse(raw)\n  private[this] def parse(raw: CharSequence): Str = ???\n  case class State(bits: Long)\n  sealed trait Attr\n}\n";

    #[test]
    fn real_world_scala_modifiers_and_expression_bodies_give_symbols_and_calls() {
        let found = symbols("scala", FANSI);
        let names: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
        assert_eq!(
            names,
            [
                ("Str", [3, 16]),
                ("Str::chars", [3, 3]),
                ("Str::colors", [3, 3]),
                ("Str::hashCode", [5, 5]),
                ("Str::length", [6, 6]),
                ("Str::splitAt", [7, 10]),
                ("Str::firstHalf", [11, 11]),
                ("Str::render", [12, 15]),
                ("Str", [17, 23]),
                ("Str::implicitApply", [18, 18]),
                ("Str::apply", [19, 19]),
                ("Str::parse", [20, 20]),
                ("Str::State", [21, 21]),
                ("Str::Attr", [22, 22]),
            ]
        );
        // The declaration line is scanned after its `=`; `new Str(..)` builds a value and calls nothing.
        assert_eq!(
            pairs("scala", FANSI),
            [pair("Str::firstHalf", "splitAt"), pair("Str::implicitApply", "apply"), pair("Str::apply", "parse")]
        );
    }

    #[test]
    fn a_def_without_braces_ends_at_the_next_line_not_indented_past_it() {
        let text = "object M {\n  def a =\n    b(1)\n      .c(2)\n\n  // note\n  def d = e(3)\n  def f(x: Int,\n        y: Int) =\n    g(x, y)\n  def h: Int\n}\ndef top = k(\n  1\n)\ndef after = 2\n";
        let found = symbols("scala", text);
        let lines: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
        assert_eq!(lines, [("M", [1, 12]), ("M::a", [2, 4]), ("M::d", [7, 7]), ("M::f", [8, 10]), ("M::h", [11, 11]), ("top", [13, 15]), ("after", [16, 16])]);
        assert_eq!(
            pairs("scala", text),
            [pair("M::a", "b"), pair("M::a", "c"), pair("M::d", "e"), pair("M::f", "g"), pair("top", "k")]
        );
    }

    #[test]
    fn a_scala_header_over_several_lines_still_opens_its_body() {
        let text = "case class Box(\n  a: Int,\n  b: Int = 1\n) extends Base\n  with Other {\n  def get = a\n}\nclass Wide(x: Int)\n  extends Base {\n  def run = get()\n}\n";
        let found = symbols("scala", text);
        let lines: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
        assert_eq!(lines, [("Box", [1, 7]), ("Box::get", [6, 6]), ("Wide", [8, 11]), ("Wide::run", [10, 10])]);
    }

    /// The zig-args shapes the benchmark lost: multi-line parameter lists, `comptime` params, parenthesised return types, `try`, a returned struct and a `test` block.
    const ARGS_ZIG: &str = "const std = @import(\"std\");\n\npub fn parseForCurrent(comptime Spec: type, init: u32) !Result(Spec, null) {\n    var result = try parseInternal(Spec, null, init);\n    return result;\n}\n\npub fn parse(\n    comptime Spec: type,\n    args: u32,\n) !Result(Spec, null) {\n    return parseInternal(Spec, null, args);\n}\n\nfn parseInternal(\n    comptime Generic: type,\n    comptime MaybeVerb: ?type,\n    args: u32,\n) !Result(Generic, MaybeVerb) {\n    _ = try parseInt(u32, \"1\");\n    return undefined;\n}\n\npub fn Result(comptime Generic: type, comptime MaybeVerb: ?type) type {\n    return struct {\n        options: Generic,\n        pub fn deinit(self: @This()) void {\n            _ = self;\n        }\n    };\n}\n\nfn parseInt(comptime T: type, str: []const u8) !T {\n    _ = str;\n    return 0;\n}\n\ntest \"parse int\" {\n    try std.testing.expectEqual(@as(i32, 50), try parseInt(i32, \"50\"));\n}\n";

    #[test]
    fn real_world_zig_multi_line_headers_try_and_test_blocks_keep_their_calls() {
        let found = symbols("zig", ARGS_ZIG);
        let spans: Vec<_> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
        assert_eq!(
            spans,
            [
                ("parseForCurrent", [3, 6]),
                ("parse", [8, 13]),
                ("parseInternal", [15, 22]),
                ("Result", [24, 31]),
                ("Result::options", [26, 26]),
                ("Result::deinit", [27, 29]),
                ("parseInt", [33, 36]),
                ("test \"parse int\"", [38, 40]),
            ]
        );
        let got: Vec<_> = refs_of("zig", "args.zig", ARGS_ZIG).into_iter().map(|reference| (reference.src, reference.name)).collect();
        let pair = |src: &str, name: &str| (src.to_string(), name.to_string());
        assert_eq!(
            got,
            [
                pair("s:args.zig#parseForCurrent", "Result"),
                pair("s:args.zig#parseForCurrent", "parseInternal"),
                pair("s:args.zig#parse", "Result"),
                pair("s:args.zig#parse", "parseInternal"),
                pair("s:args.zig#parseInternal", "Result"),
                pair("s:args.zig#parseInternal", "parseInt"),
                pair("s:args.zig#test \"parse int\"", "expectEqual"),
                pair("s:args.zig#test \"parse int\"", "parseInt"),
            ]
        );
    }

    #[test]
    fn new_names_a_type_not_a_call_unless_a_def_has_that_name() {
        let text = "object M {\n  def make = new Foo(1)\n  def deep = new a.b.Bar(baz(2))\n  def Foo(x: Int) = x\n  def keep = new Other(new Foo(3))\n}\n";
        assert_eq!(pairs("scala", text), [pair("M::make", "Foo"), pair("M::deep", "baz"), pair("M::keep", "Foo")]);
        // Other languages keep their old scan.
        assert_eq!(pairs("groovy", "class G {\n  def a() {\n    return new Foo(1)\n  }\n}\n"), [pair("G::a", "Foo")]);
    }
}
