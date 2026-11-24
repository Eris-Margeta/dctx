<!-- SPDX-License-Identifier: Apache-2.0 -->
# ADR-019: Import Resolution Exactness

**Status:** Accepted

## Context

An `exact` import is used as evidence that one file depends on another. A false edge can also introduce a binding that sends calls to the wrong symbol with `name` confidence. An omitted edge hides a dependency from neighbors, graph export and impact analysis.

Issues #8 and #9 exposed both errors in ES module imports. The lexer joined a specifier and a binding into one string. Extension stripping worked on a bare path but failed when a binding followed the extension. Splitting on punctuation erased the difference between a sibling, a parent and a package. A whole-project suffix lookup then selected an unrelated file or discarded two candidates even when the importer had exactly one neighbor of that name. Issue #10 exposed a different break in the same pipeline: a lexer can emit a re-export that the facade never consumes. Issue #11 identified a coverage limit at the other endpoint: an exported value without a declaration node cannot be the target of a symbol edge. The [companion audit](audits/AUDIT-20261124-import-resolution.md) records independent examples from the release CLI.

## Decision

1. **Start from the importer and the file inventory.** For a path-relative specifier, join its path to the importer's directory, normalize within the authorized view, then apply the extension and directory-entry rules of the selected language mode. Use the inventory of all included files, including files with no symbols. Never use a suffix search for this lookup. For semantics based on a package, namespace, module tree, load path or working directory, use that context. A source filename alone cannot establish it.
2. **Preserve the reference until resolution.** Carry the literal value, form of import, path anchor, module, selected member or wildcard, local binding, lexical owner and export visibility as distinct facts wherever applicable. Decode literals according to their language. Preserve meaningful punctuation and qualifiers. Where an adapter still needs a string boundary, use one shared, language- and form-aware splitter with round-trip fixtures for **every spelling each lexer emits**.
3. **Resolve the target before binding a name.** Apply the language's lookup order to the eligible candidates. Bind only names actually introduced, in their scope and value/type space. A side-effect import has no local binding. Resolve re-exports through the public surface, including aliases. Keep a known file dependency when its member cannot be represented, if the relation permits a file endpoint. Never invent a symbol to complete the edge.
4. **Let a miss stay unresolved.** A missing file, inaccessible member, unknown search context or tie after semantic filtering yields no asserted target. In particular, a miss never falls back to a weaker match labeled `exact`. Reserve `exact` for an evidenced import target and `name` for an applicable, visible name. Independent call heuristics may use `guess` only where the relation permits it. They cannot upgrade a failed binding. Honor the confidence matrix in `crates/dctx-map-model/src/domain/graph.rs`.
5. **Account for every reference kind.** At each lexer-to-facade projection, consume the kind or explicitly reject it with a test for that producer, profile and reason. Cover `imports`, `reexports`, `calls`, `uses-type`, `implements`, `tests` and `documents` where emitted. An unsupported computed expression can abstain. A catch-all filter or oracle agreement on a raw string does not establish correct final edges.

### Per-language import semantics

| Language or family | Context that establishes a target |
| --- | --- |
| JavaScript, TypeScript, JSX, TSX, MJS, CJS, MTS, CTS and embedded web scripts | Resolve local ESM paths from the containing source file. Honor explicit filenames and the selected runtime/compiler's extension policy. Preserve URL query/fragment identity separately from the file path when supported. Resolve bare packages, package exports, aliases and built-ins in their own domain. CommonJS literal `require` needs an unshadowed loader and CommonJS search rules. Literal dynamic imports can supply a dependency without a synchronous binding. Distinguish default, named, namespace, type-only, side-effect and re-export forms, including supported CommonJS/TS export assignments. |
| Python | Leading dots count package levels. Resolve absolute names against known import roots. For `from p import x`, consider the package's exported attribute and the eligible submodule in Python order. Preserve the difference between `import p.x`, `as` and star import. Use a statically known `__all__` for star bindings, independently of explicit imports. |
| Rust | Resolve `crate`, `self`, repeated `super`, external crates and use-tree leaves from the crate and lexical module. Follow file-module declarations, inline modules and supported `#[path]` overrides. Carry `pub use`, aliases, visibility restrictions and globs across the public boundary. |
| Go | Map the import path through the applicable module/workspace and package directory. Honor the package declaration and any local alias, dot or blank import. A package may contain several files. External and standard packages require positive ownership evidence before linking to an indexed package. |
| C and C++ | Keep the complete header name, quote/angle form and directive. Use the including directory and ordered search roots for quoted includes according to the toolchain. Angle includes require the applicable include roots. Preserve `include_next` search position where supported. C++ using-declarations and namespace directives resolve in the declaration scope. |
| Java | Match declared package, type and accessibility in the relevant source set. Static member and on-demand imports introduce different bindings. |
| Kotlin | Match the declared package and its top-level or member declarations across eligible files. Keep aliases and star imports. JVM interop still requires a visible declaration. |
| C# | Resolve namespace, type alias, global using and using-static forms against the compilation and lexical namespace. A namespace can span files. |
| Ruby | Anchor `require_relative` to the source file. `require` and `load` have separate load-path and extension rules. A missing load path cannot be inferred from the project tree. |
| PHP | Preserve the namespace and class/function/constant import kind and alias. For includes, explicit dot paths use the runtime working directory. A literal relative to `__DIR__` uses the source directory. An unknown working directory or include path limits what can be asserted. |
| Swift | Imports name compiler modules and optionally declarations within them. Use target/module membership and applicable dependencies. Source files within one target share a module. |
| Configuration (JSON, TOML, YAML) | Interpret a path, package dependency or workspace member according to the manifest field. Resolve literal paths with that field's base. A conventional string pattern alone cannot prove an import. |
| Markdown | Resolve local document links from the containing document and fragments within their document. Keep documentation links, code mentions and external URLs distinct. |
| Other structural families and opaque files | Enable import rules by language and form. For example, protobuf needs import roots and Lua needs a known package loader/search path. CSS has no import extractor in this revision. If enabled, resolve local CSS import URLs from the containing stylesheet under the applicable URL rules. Keep unsupported forms as tested abstentions. |

## Consequences

An incomplete view will sometimes show fewer edges, particularly for external packages, headers and dynamic imports. A proven local dependency survives an unrelated file being added, renamed or removed. Package ownership, search roots and symbol exports become explicit resolver inputs. A package with no representable node may require a model change before an exact package edge is available. These rules constrain future fixes and confidence claims. The audit records the behavior of the checkout on which the decision was made.

## Lesson

The lexer and resolver each had passing examples on their own side of the boundary. A test for a bare path with an extension and a test for a joined member name did not exercise a real lexer emitting both in the same reference. A single uniquely named module also allowed a global lookup to mimic language-correct resolution.

Run each lexer's real output through the production projection and resolver. Assert the source, edge kind, target and confidence, plus the absence of a competing edge. Vary the importer, extension, binding, scope and decoy placement one at a time. Include no-hit, multiple-hit, empty-module and re-export cases. Repeat the fixtures through an independent oracle where available and include a release-CLI walk so that inventory omissions are visible.
