# dctx-lex-rb

Hand-written Ruby structural extractor for dctx map mode (decision 077). Pure Rust:
`dctx-map-model` and `dctx-hash` only, no `unsafe`, no tree-sitter, no git, no mmap.

`RubyLexer::extract` returns a `LexedFile` in the same shape as `dctx-lex-go`: a
module node, labeled symbols and containment edges, unresolved references, a
precision label on every fact, `PARSER_ID`, `PARSER_REVISION`, and
`body_fingerprint` (whitespace-normalized SHA-256).

Paths: `.rb`, `.rake`, `.gemspec`, `Rakefile`, and `Gemfile`.

Extracted: `require` / `require_relative` / `load` string edges, modules and
classes (nesting and `class << self`), methods and singleton methods with
parameters, `attr_accessor` / `attr_reader` / `attr_writer`, constants, aliases,
RDoc and YARD comment summaries, call sites, and dotted member references.

Lexed, so they do not invent declarations: heredocs (plain, squiggly, quoted,
several per line), `%`-literals, interpolated code, regex versus division,
symbols, `=begin`/`=end`, `__END__`, and backslash continuation.

Abstains instead of inventing an edge. `define_method`, `const_set`,
`class_eval`, and any other metaprogramming call is not resolved. A bare
identifier is not a call. A `require` whose argument is not a string literal
emits no import.

Malformed input yields partial facts and a `Diagnostic`. Oversized input,
nesting past `Limits`, and too many facts are typed `LexError`s with no facts.
