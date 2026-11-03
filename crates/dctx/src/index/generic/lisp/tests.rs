// SPDX-License-Identifier: Apache-2.0
//! Tests for the lisp family: Clojure, Common Lisp, Scheme, Emacs Lisp and Nix.

use super::super::{handles, symbols};
use crate::index::extract::{extract_structural_refs, language_of};

const TAGS: &[&str] = &["clojure", "lisp", "scheme", "elisp", "nix"];

fn named(tag: &str, text: &str) -> Vec<(String, String)> {
    symbols(tag, text).into_iter().map(|symbol| (symbol.name, symbol.kind)).collect()
}

fn pair(name: &str, kind: &str) -> (String, String) {
    (name.to_string(), kind.to_string())
}

#[test]
fn every_lisp_finds_its_definitions() {
    let clj = "(ns shop.core)\n(defn add [a b]\n  (+ a b))\n(defmacro unless [c & body] `(if (not ~c) (do ~@body)))\n(def rate 4)\n(def ^:private twice (fn [x] (* 2 x)))\n(defprotocol Shape\n  (area [s])\n  (grow [s k]))\n(defrecord Box [w h])\n";
    assert_eq!(
        named("clojure", clj),
        [
            pair("shop.core", "type"),
            pair("add", "fn"),
            pair("unless", "fn"),
            pair("rate", "var"),
            pair("twice", "fn"),
            pair("Shape", "type"),
            pair("Shape::area", "fn"),
            pair("Shape::grow", "fn"),
            pair("Box", "type"),
        ]
    );
    let lisp = "(defun square (x)\n  (* x x))\n(defmacro my-when (c &body b) `(if ,c (progn ,@b)))\n(defclass point () ((x :initarg :x)))\n(defstruct (pt (:conc-name p-)) x y)\n(defvar *count* 0)\n(defconstant +max+ 10)\n(defgeneric area (shape))\n#| (defun hidden () 1) |#\n";
    assert_eq!(
        named("lisp", lisp),
        [pair("square", "fn"), pair("my-when", "fn"), pair("point", "type"), pair("pt", "type"), pair("*count*", "var"), pair("+max+", "const"), pair("area", "fn")]
    );
    let scm = "(define (cube x) (* x x x))\n(define pi 3.14)\n(define sq (lambda (x) (* x x)))\n(define ((curried a) b) a)\n(define-record-type point (make-point x y) point? (x px))\n(define-syntax swap! (syntax-rules () ((_ a b) 1)))\n";
    assert_eq!(
        named("scheme", scm),
        [pair("cube", "fn"), pair("pi", "var"), pair("sq", "fn"), pair("curried", "fn"), pair("point", "type"), pair("swap!", "fn")]
    );
    let el = "(defun my-mode-hook ()\n  (setq x 1))\n(defvar my-var 1)\n(defcustom my-opt 1 \"doc\")\n(defvar c ?\\()\n(defun after () 1)\n";
    assert_eq!(named("elisp", el), [pair("my-mode-hook", "fn"), pair("my-var", "var"), pair("my-opt", "var"), pair("c", "var"), pair("after", "fn")]);
}

#[test]
fn nix_lists_the_attributes_of_the_top_level_set() {
    let text = "{ pkgs, lib ? 1, ... }:\nlet\n  inner = { x = 1; };\nin\n{\n  hello = pkgs.writeShellScriptBin \"hello\" \"echo hi\";\n  mkThing = name: { inherit name; };\n  pat = { a, b }: a + b;\n  inherit (pkgs) git;\n  nested = { deep = 1; };\n  withs = with pkgs; [ git ];\n  letv = let a = 1; in a;\n  \"quoted\" = 1;\n  script = ''\n    fake = bar;\n  '';\n  # comment = 1;\n  last = 2;\n}\n";
    assert_eq!(
        named("nix", text),
        [
            pair("hello", "var"),
            pair("mkThing", "fn"),
            pair("pat", "fn"),
            pair("nested", "var"),
            pair("withs", "var"),
            pair("letv", "var"),
            pair("script", "var"),
            pair("last", "var"),
        ]
    );
    let found = symbols("nix", text);
    let script = found.iter().find(|symbol| symbol.name == "script").map(|symbol| symbol.lines);
    assert_eq!(script, Some([14, 16]));
    assert_eq!(named("nix", "{\n  a = 1;\n}\n"), [pair("a", "var")]);
    assert_eq!(named("nix", "rec { f = x: x; g = lib.f; }"), [pair("f", "fn"), pair("g", "var")]);
    assert!(named("nix", "{ pkgs }: pkgs.hello").is_empty());
}

#[test]
fn symbols_carry_lines_signature_and_the_sig_summary() {
    let text = "(ns a)\n(defn add [a b]\n  (+ a b))\n";
    let found = symbols("clojure", text);
    assert_eq!(found[1].lines, [2, 3]);
    assert_eq!(found[1].signature, "(defn add [a b]");
    assert_eq!(found[1].summary, found[1].signature);
    assert_eq!(found[1].summary_source, "sig");
    assert_eq!(found[1].span, [7, 33]);
}

#[test]
fn comments_strings_and_nested_forms_hide_declarations() {
    assert_eq!(named("clojure", "; (defn hidden [] 1)\n(def s \"(defn fake [] 1)\")\n(comment (defn nope [] 1))\n(defn shown [] \\( 1)\n"), [pair("s", "var"), pair("shown", "fn")]);
    assert_eq!(named("lisp", "#| (defun a () 1) #| (defun b () 1) |# (defun c () 1) |#\n(defun \"s\" ())\n(defun shown ()\n  \"doc (defun fake ()\n  still doc\"\n  1)\n"), [pair("shown", "fn")]);
    assert_eq!(named("scheme", "(let ((x 1)) (define inner 2))\n(define shown 1)\n"), [pair("shown", "var")]);
    assert_eq!(named("nix", "{\n  # hidden = 1;\n  s = \"x = 1; ${y}\";\n  t = /* c = 2; */ 3;\n}\n"), [pair("s", "var"), pair("t", "var")]);
}

#[test]
fn denylisted_keywords_are_never_symbols() {
    for word in ["if", "else", "for", "while", "match", "switch", "return", "case", "catch", "when", "do", "try"] {
        for (tag, text) in [
            ("clojure", format!("(defn {word} [x] x)\n(def {word} 1)\n(defprotocol {word} ({word} [x]))\n")),
            ("lisp", format!("(defun {word} (x) x)\n(defvar {word} 1)\n(defstruct ({word}) a)\n")),
            ("scheme", format!("(define ({word} x) x)\n(define {word} 1)\n")),
            ("elisp", format!("(defun {word} (x) x)\n(defvar {word} 1)\n")),
            ("nix", format!("{{\n  {word} = 1;\n  g = {word}: x;\n}}\n")),
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
        "(",
        ")))) ((( ]]] }}}",
        "(defn",
        "(defn add [a b]\n  (+ a",
        "(defprotocol P\n  (m [x]",
        "(defprotocol\n(defprotocol P (",
        "(def ^",
        "(def ^{:a",
        "(def #^ x 1)",
        "(define (",
        "(define ((((",
        "(defstruct (",
        "\"never closed (defn a [] 1)",
        "#| never closed (defun a ())",
        "#\\",
        "\\",
        "?",
        "(defvar ?\\",
        "(defun \u{e9}\u{1F600} (x) x)\r\n(defvar \u{1F600} ?\u{e9})\r\n",
        "{\n  a = ''never closed",
        "{ a = \"${",
        "{ a = b: ",
        "{ a",
        "{ a = { b",
        "{ a = let b = 1;",
        "{ a = with x;",
        "{ pkgs }:",
        "{ } @ x:",
        "let",
        "in { a = 1; }",
        "a: { b = 1; }",
        "/* never closed",
        "\u{0}\u{1}\r\n\r\n",
    ];
    for tag in TAGS {
        for sample in samples {
            let _ = symbols(tag, sample);
        }
        let deep = format!("{}{}", "(defn f ".repeat(5000), ")".repeat(10));
        let _ = symbols(tag, &deep);
        let deep = format!("{}\n{}", "{ a = ".repeat(3000), "(".repeat(3000));
        let _ = symbols(tag, &deep);
        assert!(symbols(tag, "").is_empty());
    }
}

#[test]
fn extensions_dispatch_to_the_lisp_family() {
    for (path, tag) in [
        ("a.clj", "clojure"),
        ("a.cljs", "clojure"),
        ("a.cljc", "clojure"),
        ("a.edn", "clojure"),
        ("a.lisp", "lisp"),
        ("a.lsp", "lisp"),
        ("a.cl", "lisp"),
        ("a.scm", "scheme"),
        ("a.ss", "scheme"),
        ("a.el", "elisp"),
        ("a.nix", "nix"),
        ("a.NIX", "nix"),
    ] {
        assert_eq!(language_of(path), tag, "{path}");
        assert!(handles(tag));
    }
    let (found, refs) = extract_structural_refs("src/a.clj", b"(defn helper [] 1)\n(defn run [] (helper))\n");
    assert_eq!(found.iter().map(|symbol| symbol.name.as_str()).collect::<Vec<_>>(), ["helper", "run"]);
    assert_eq!(refs.iter().map(|r| (r.src.as_str(), r.name.as_str())).collect::<Vec<_>>(), [("s:src/a.clj#run", "helper")]);
    assert!(extract_structural_refs("a.el", &[0xff]).0.is_empty());
    assert!(extract_structural_refs("a.nix", b"{ a = 1; }\n").1.is_empty());
}
