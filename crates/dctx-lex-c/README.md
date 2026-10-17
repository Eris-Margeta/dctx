# dctx-lex-c

Hand-written C and C++ structural extractor for dctx map mode (decision 077). Pure Rust:
`dctx-map-model` and `dctx-hash` only, no `unsafe`, no tree-sitter, no git, no mmap.

`CLexer::extract` returns a `LexedFile` in the same shape as `dctx-lex-go`: a module
node, labeled symbols and containment edges, unresolved references, a precision
label on every fact, `PARSER_ID`, `PARSER_REVISION`, and `body_fingerprint`
(whitespace-normalized SHA-256).

Paths: `.c` and `.h` are C; `.cc`, `.cpp`, `.cxx`, `.hpp` and `.hh` are C++.
The walker accepts the same declaration shapes in both dialects.

Extracted: `#include` edges, `#define` macros, namespaces, functions and methods
(signatures, template headers, out-of-line `Type::name`), `struct` / `class` /
`union` / `enum` / `typedef` / `using`, doc-comment summaries (`///`, `//!`,
`/**`, `/*!`), call sites, and member reads (`a.b`, `p->b`).

Unions, typedefs and using-aliases are `type` symbols; the signature text carries
`union`, `typedef` or `using`. Namespaces and fields have no symbol kind; the
signature starts with `namespace` or the field declaration. Destructors are
spelled `dtor_Name` so the stable id does not contain `~`.

The preprocessor is only a token stream. Macros are never expanded. An `#include`
whose path is itself a macro is abstained. A declaration whose name is a macro,
or a brace body introduced only by a macro, is abstained with `HiddenByMacro`.
Calls to a macro defined in this file are not call edges. Facts inside `#if` /
`#ifdef` / `#else` keep convention precision because the branch is not evaluated.

Malformed input yields partial facts and a `Diagnostic`. Oversized input, bracket
or angle nesting past `Limits`, and too many facts are typed `LexError`s with no
facts. Every node is contained by the file module, ids are unique, and every
line span lies inside the file.
