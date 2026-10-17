# dctx-lex-py

Hand-written Python 3 structural extractor for dctx map mode (decision 077, PARSERS-v1, SPEC-MAP-MODE 5–6.2).

Production dependencies are `dctx-map-model` and `dctx-hash` only. There is no tree-sitter, no `unsafe`, and no process or git access. `PARSER_ID` is `dctx-lex-py`. `PARSER_REVISION` bumps when the same bytes can yield different facts. Body fingerprints are SHA-256 of the whitespace-collapsed span, via `dctx-hash`.

## Facts

Every node, containment edge and raw reference carries a precision label:

| Label | When |
|---|---|
| `structural` | A declaration or a dotted call/import in balanced, terminated syntax |
| `convention` | A module-level constant, recognised only because its name is `ALL_CAPS` |
| `partial` | The fact's span ends after the first diagnostic |

Extracted:

- one module node per file (`.py`, `.pyw`, `.pyi`)
- classes, functions, async functions and methods, with collapsed signatures and docstring first sentences
- `import` and `from` imports, including relative dots, aliases and `*`
- module-level `ALL_CAPS` assignments (`NAME = …`, `NAME: T = …`, `NAME: T`)
- dotted call references (`name`, `obj.method`)

Containment edges are `contains` / `exact`. Imports and calls stay raw references (`imports`, `calls`); this crate does not resolve them.

## Abstains

The extractor records a diagnostic or skips the span instead of guessing:

- callees that are not a dotted name (`obj[k]()`, `(fn)()`, lambdas as callees)
- `uses-type` edges for annotations and bases
- match-pattern class patterns (guards are scanned)
- augmented assignments (`+=`) as constant bindings
- assignments that are not direct statements of the module
- names that cannot form a stable id

An unterminated non-triple string ends at the newline so later statements still parse. An unterminated triple quote or an unclosed bracket follows Python and swallows the rest of that literal or group. Either way the file is `Partial` and nothing panics.

## Limits

`Limits::CEILING` is 8 MiB, depth 256 and 250 000 facts. Callers may only narrow those ceilings. Oversized input, nesting past the ceiling and too many facts are `LexError` refusals with no facts. Other malformed input returns the facts parsed before the damage, plus a `Diagnostic`.

This packet's file list is a single library file, so domain, lexer and walker are inner modules of `src/lib.rs`. Golden sources live in `tests/dctx_lex_py.rs` for the same reason.
