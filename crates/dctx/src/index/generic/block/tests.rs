//! Tests for the block family: Elixir, Lua, OCaml, R, Julia and Haskell.

use super::super::{handles, symbols};
use crate::index::extract::{extract_structural_refs, language_of};

const TAGS: &[&str] = &["elixir", "lua", "ocaml", "r", "julia", "haskell"];

fn named(tag: &str, text: &str) -> Vec<(String, String)> {
    symbols(tag, text).into_iter().map(|symbol| (symbol.name, symbol.kind)).collect()
}

fn pair(name: &str, kind: &str) -> (String, String) {
    (name.to_string(), kind.to_string())
}

#[test]
fn every_block_language_finds_its_types_and_functions() {
    let elixir = "defmodule Shop.Cart do\n  def add(cart, item) do\n    if item do\n      [item | cart]\n    end\n  end\n\n  defp total(cart), do: length(cart)\nend\n";
    assert_eq!(named("elixir", elixir), [pair("Shop.Cart", "type"), pair("Shop.Cart::add", "fn"), pair("Shop.Cart::total", "fn")]);
    let lua = "local M = {}\nfunction M.start(x)\n  if x then return 1 end\nend\nlocal function helper()\n  return 2\nend\nfunction globalfn() end\nfunction M:run() end\nreturn M\n";
    assert_eq!(named("lua", lua), [pair("M", "var"), pair("M::start", "fn"), pair("helper", "fn"), pair("globalfn", "fn"), pair("M::run", "fn")]);
    let ocaml = "let add x y = x + y\nlet rec fact n = if n = 0 then 1 else n * fact (n - 1)\ntype shape = Circle | Square\nmodule Geo = struct\n  let area r = r *. r\nend\n";
    assert_eq!(
        named("ocaml", ocaml),
        [pair("add", "fn"), pair("fact", "fn"), pair("shape", "type"), pair("Geo", "type"), pair("Geo::area", "fn")]
    );
    let r = "add <- function(x, y) {\n  x + y\n}\nscale = function(v) v * 2\n";
    assert_eq!(named("r", r), [pair("add", "fn"), pair("scale", "fn")]);
    let julia = "function solve(a, b)\n    return a + b\nend\nstruct Point\n    x::Int\nend\nmutable struct Box\nend\n";
    assert_eq!(named("julia", julia), [pair("solve", "fn"), pair("Point", "type"), pair("Point::x", "field"), pair("Box", "type")]);
    let haskell = "module Main where\n\nadd :: Int -> Int -> Int\nadd x y = x + y\n\ndata Shape = Circle | Square\n";
    assert_eq!(named("haskell", haskell), [pair("add", "fn"), pair("Shape", "type")]);
}

#[test]
fn nesting_and_end_lines_follow_the_blocks() {
    let elixir = symbols("elixir", "defmodule A do\n  defmodule B do\n    def f(x), do: x\n  end\n  def g(a,\n        b) do\n    a\n  end\nend\n");
    let found: Vec<(&str, [u32; 2])> = elixir.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
    assert_eq!(found, [("A", [1, 9]), ("A::B", [2, 4]), ("A::B::f", [3, 3]), ("A::g", [5, 8])]);
    let julia = symbols("julia", "module M\nfunction f(xs)\n  xs[end]\nend\nabstract type S end\nend\n");
    let found: Vec<(&str, [u32; 2])> = julia.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
    assert_eq!(found, [("M", [1, 6]), ("M::f", [2, 4]), ("M::S", [5, 5])]);
    let haskell = symbols("haskell", "class Shape a where\n  area :: a -> Double\n\nf :: Int\nf = 1\n  where y = 2\n");
    let found: Vec<(&str, [u32; 2])> = haskell.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect();
    assert_eq!(found, [("Shape", [1, 2]), ("Shape::area", [2, 2]), ("f", [4, 6])]);
    let r = symbols("r", "f <- function(x)\n{\n  g <- function(y) y\n  x\n}\nh <- 1\n");
    assert_eq!(r.iter().map(|symbol| (symbol.name.as_str(), symbol.lines)).collect::<Vec<_>>(), [("f", [1, 5])]);
}

#[test]
fn symbols_carry_the_declaration_line_as_signature_and_summary() {
    let found = symbols("elixir", "defmodule A do\n  def run(x), do: x\nend\n");
    assert_eq!(found[1].signature, "def run(x), do: x");
    assert_eq!(found[1].summary, found[1].signature);
    assert_eq!(found[1].summary_source, "sig");
    assert_eq!(found[1].lines, [2, 2]);
}

#[test]
fn comments_strings_and_locals_hide_declarations() {
    assert_eq!(named("elixir", "# def hidden(x) do\n@doc \"\"\"\ndef fake(x) do\n\"\"\"\ndef shown(x), do: \"def nope\"\n"), [pair("shown", "fn")]);
    assert_eq!(named("lua", "--[[ function hidden() end\n]]\n-- function nope() end\nlocal s = [[function fake() end]]\nfunction shown() end\n"), [pair("shown", "fn")]);
    assert_eq!(named("ocaml", "(* let hidden = 1 (* nested *) let alsohidden = 2 *)\nlet s = \"let fake = 1\"\nlet shown = 1\nlet f x =\n  let local = 2 in\n  local\n"), [pair("s", "fn"), pair("shown", "fn"), pair("f", "fn")]);
    assert_eq!(named("r", "# f <- function() 1\ng <- function() {\n  inner <- function() 2\n}\nx <- list(\n  k = function() 3\n)\n"), [pair("g", "fn")]);
    assert_eq!(named("julia", "#= function hidden() end =#\n# function nope() end\ns = \"function fake() end\"\nfunction shown() end\n"), [pair("shown", "fn")]);
    assert_eq!(named("haskell", "-- hidden :: Int\n{- data Hidden = H -}\nf :: Int\nf = 1\n  where g :: Int\n        g = 2\n"), [pair("f", "fn")]);
}

#[test]
fn denylisted_keywords_are_never_symbols() {
    for word in ["if", "else", "for", "while", "match", "switch", "return", "case", "catch", "when", "do", "try"] {
        for (tag, text) in [
            ("elixir", format!("defmodule {word} do\n  def {word}(x) do\n  end\nend\ndef {word}(y), do: 1\n")),
            ("lua", format!("function {word}() end\nfunction t.{word}() end\nlocal {word} = function() end\n")),
            ("ocaml", format!("let {word} x = 1\ntype {word} = A\nval {word} : int\n")),
            ("r", format!("{word} <- function(x) x\n{word} = function(x) x\n")),
            ("julia", format!("function {word}(x)\nend\nstruct {word}\nend\n")),
            ("haskell", format!("{word} :: Int\ndata {word} = A\n")),
        ] {
            let found = named(tag, &text);
            assert!(found.iter().all(|(name, _)| name != word && !name.ends_with(&format!("::{word}"))), "{tag} {word}: {found:?}");
        }
    }
}

#[test]
fn malformed_input_never_panics_and_stays_bounded() {
    let samples = [
        "",
        "defmodule A do\n  def f(",
        "end end end\n}}}} ))))\n",
        "def f(a,\n  b,\n",
        "function f(\n",
        "function\n",
        "local function\n",
        "--[[ never closed function f() end",
        "= function(\nx <- function(\nx <<- function",
        "let\nlet rec\ntype\ntype 'a\ntype (\nmodule\nmodule type\nmodule X =\n",
        "(* never closed let x = 1",
        "module M = struct\n  let a = 1\n",
        "struct\nend end end\n",
        ":: Int\n,, :: Int\na, :: Int\nclass\ndata\nnewtype =>\ndata Eq a =>\n",
        "{- never closed f :: Int",
        "abstract type\nabstract\nprimitive type\nfunction \u{1F600}(x)\n",
        "\"\"\" never closed heredoc def f(x) do",
        "defmodule \u{e9}\u{1F600} do\r\n  def \u{e9}(x), do: 1\r\nend\r\n",
        "\u{0}\u{1}\r\n\r\n",
        "@\n@type\n@spec\n@type t\ndefstruct\ndefimpl\ndefimpl P, for:\nend",
        "M.x =\nM. = 1\nlocal t = {\nlocal = {\n.x = 1\n",
        "const\nconst x\nf(\nf(x\nf(x)::\nf(x) where\nf(x) where T\nstruct S\n  x::\n  ::T\n",
        "setClass(\nsetClass(\"\nsetClass(\"A\nR6::R6Class(\nx <- setGeneric(\nsetClass(Class =\n",
        "exception\nclass\nclass [\nclass type\nmethod\nval mutable\nclass c = object\n  method\n  val\n",
        "instance\ninstance =>\ndata T = T {\nnewtype N = N { a ::\n  , b\n  } }\n}\ninstance Show T where\n  \n  show\n",
    ];
    for tag in TAGS {
        for sample in samples {
            let _ = symbols(tag, sample);
        }
        let deep = format!("{}{}", "do ".repeat(5000), "end ".repeat(10));
        let _ = symbols(tag, &deep);
        let deep = format!("{}\n{}", "function f(x) ".repeat(3000), "(".repeat(3000));
        let _ = symbols(tag, &deep);
        assert!(symbols(tag, "").is_empty());
    }
}

#[test]
fn extensions_dispatch_to_the_block_family() {
    for (path, tag) in [
        ("a.ex", "elixir"),
        ("a.exs", "elixir"),
        ("a.lua", "lua"),
        ("a.ml", "ocaml"),
        ("a.mli", "ocaml"),
        ("a.r", "r"),
        ("a.R", "r"),
        ("a.jl", "julia"),
        ("a.hs", "haskell"),
    ] {
        assert_eq!(language_of(path), tag, "{path}");
        assert!(handles(tag));
    }
    let (found, refs) = extract_structural_refs("lib/cart.ex", b"defmodule Cart do\n  def add(x), do: x\nend\n");
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["Cart", "Cart::add"]);
    assert!(refs.is_empty());
    assert!(extract_structural_refs("a.hs", &[0xff]).0.is_empty());
}

#[test]
fn elixir_lists_structs_types_specs_and_implementations() {
    let text = "defmodule Shop.Item do\n  @moduledoc \"x\"\n  defstruct [:name, :price]\n  @type t :: %__MODULE__{}\n  @spec total([t]) :: number\n  defmacro is_cheap(p), do: quote(do: unquote(p) < 10)\n  def total(items), do: Enum.sum(items)\n  @callback render() :: String.t()\nend\ndefprotocol Priced do\n  def price(x)\nend\ndefimpl Priced, for: Shop.Item do\n  def price(i), do: i.price\nend\n";
    assert_eq!(
        named("elixir", text),
        [
            pair("Shop.Item", "type"),
            pair("Shop.Item::Item", "type"),
            pair("Shop.Item::t", "type"),
            pair("Shop.Item::is_cheap", "fn"),
            pair("Shop.Item::total", "fn"),
            pair("Shop.Item::render", "fn"),
            pair("Priced", "type"),
            pair("Priced::price", "fn"),
            pair("Priced.Shop.Item", "type"),
            pair("Priced.Shop.Item::price", "fn"),
        ]
    );
}

#[test]
fn lua_lists_table_fields_and_local_tables() {
    let text = "local M = {}\nM.version = \"1.0\"\nM.handler = function(x) return x end\nfunction M.start() return 1 end\nlocal MAX = 5\nlocal cache = {\n  a = 1,\n}\nlocal n = 1\nreturn M\n";
    let found = symbols("lua", text);
    let listed: Vec<(&str, &str, [u32; 2])> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.kind.as_str(), symbol.lines)).collect();
    assert_eq!(
        listed,
        [("M", "var", [1, 1]), ("M::version", "field", [2, 2]), ("M::handler", "fn", [3, 3]), ("M::start", "fn", [4, 4]), ("MAX", "const", [5, 5]), ("cache", "var", [6, 8])]
    );
}

#[test]
fn julia_lists_constants_short_functions_and_struct_fields() {
    let text = "const LIMIT = 10\nabstract type Animal end\nmutable struct Dog <: Animal\n    name::String\n    Dog(n) = new(n)\nend\nmacro twice(ex)\n    :($ex; $ex)\nend\nbark(d::Dog) = println(d.name)\nadd(a, b)::Int = a + b\nbase(x) == 3\nshow(x) => 1\n";
    assert_eq!(
        named("julia", text),
        [
            pair("LIMIT", "const"),
            pair("Animal", "type"),
            pair("Dog", "type"),
            pair("Dog::name", "field"),
            pair("Dog::Dog", "fn"),
            pair("twice", "fn"),
            pair("bark", "fn"),
            pair("add", "fn"),
        ]
    );
}

#[test]
fn r_lists_classes_and_generics() {
    let text = "setClass(\"Person\", representation(name = \"character\"))\nsetGeneric(\"greet\", function(p) standardGeneric(\"greet\"))\nAccount <- R6::R6Class(\"Account\",\n  public = list(deposit = function(x) x)\n)\nPoint <- setRefClass(\"Pt\", fields = list(x = \"numeric\"))\nhelper <- function(x) x\n";
    let found = symbols("r", text);
    let listed: Vec<(&str, &str, [u32; 2])> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.kind.as_str(), symbol.lines)).collect();
    assert_eq!(
        listed,
        [("Person", "type", [1, 1]), ("greet", "fn", [2, 2]), ("Account", "type", [3, 5]), ("Pt", "type", [6, 6]), ("helper", "fn", [7, 7])]
    );
}

#[test]
fn ocaml_lists_exceptions_module_types_classes_and_their_members() {
    let text = "type point = { x : int; y : int }\nexception Bad of string\nmodule type S = sig\n  val run : int -> int\nend\nmodule M : S = struct\n  let run x = x\nend\nclass counter = object\n  val mutable n = 0\n  method incr = n <- n + 1\n  method private reset = ()\nend\n";
    assert_eq!(
        named("ocaml", text),
        [
            pair("point", "type"),
            pair("Bad", "type"),
            pair("S", "type"),
            pair("S::run", "fn"),
            pair("M", "type"),
            pair("M::run", "fn"),
            pair("counter", "type"),
            pair("counter::n", "field"),
            pair("counter::incr", "fn"),
            pair("counter::reset", "fn"),
        ]
    );
}

#[test]
fn haskell_lists_record_fields_and_instance_methods() {
    let text = "module Geo where\n\ntype Name = String\nnewtype Meters = Meters Double\ndata Shape = Circle { radius :: Double } | Square { side :: Double }\ndata Person = Person\n  { name :: String\n  , x, y :: Int\n  } deriving Show\nclass HasArea a where\n  area :: a -> Double\ninstance HasArea Shape where\n  area (Circle r) = r\n  area _ = 0\n    where z = 1\n  perimeter _ = 1\n";
    let found = symbols("haskell", text);
    let listed: Vec<(&str, &str)> = found.iter().map(|symbol| (symbol.name.as_str(), symbol.kind.as_str())).collect();
    assert_eq!(
        listed,
        [
            ("Name", "type"),
            ("Meters", "type"),
            ("Shape", "type"),
            ("Shape::radius", "field"),
            ("Shape::side", "field"),
            ("Person", "type"),
            ("Person::name", "field"),
            ("Person::x", "field"),
            ("Person::y", "field"),
            ("HasArea", "type"),
            ("HasArea::area", "fn"),
            ("HasArea.Shape", "type"),
            ("HasArea.Shape::area", "fn"),
            ("HasArea.Shape::perimeter", "fn"),
        ]
    );
    let area = found.iter().find(|symbol| symbol.name == "HasArea.Shape::area").expect("instance method");
    assert_eq!(area.lines, [13, 15]);
}
