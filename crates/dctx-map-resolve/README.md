# dctx-map-resolve

Pure resolution for map-mode spec §§5.2, 5.3, 6.2 and 6.3. The only production dependencies are `dctx-map-model` and `thiserror`. No parser, filesystem, database, clock or process access is required.

CM-6 supplies a complete snapshot of `Facts`, one per source file:

- `path` is the place-relative file owner; `package` is a stable package identity such as its manifest directory.
- Each `Definition` contains a public model `Node` and a package-relative `qualified_name`. For example, the root module is `""`, a nested module is `"api"`, and a function is `"api::run"`. Inline modules and methods follow the same rule. The model's display name and stable id remain unchanged.
- Each `Reference` supplies its source id, lexical scope, edge kind and either a name or a known target id. Containment requires an id. Test calls and documentation mentions use `Tests` and `Documents` respectively. Adapters expand grouped imports into references; aliases use `binding`, and glob imports use a target ending in `::*`.

`resolve(&files)` builds a `SymbolTable`, validates file ownership and emits sorted, deduplicated model edges plus lossless unresolved facts. Lexical scopes, `crate::`, `self::`, repeated `super::`, import aliases and reexport chains resolve before same-package bare-name fallback. Qualified names with missing prefixes remain unresolved. Cycles terminate, and ambiguous candidates are retained without choosing a winner. Definitions and explicit imports take precedence over glob imports. Documentation may also target a module by its source path. Explicit ids can cross package boundaries; external dependency names need definitions or adapter-provided ids.

Path-proven imports, reexports and implementations are `exact`; name-only matches are `name`. Scoped/imported calls are `name`, and bare-name fallback calls are `guess`. Containment is always `exact`; type uses, tests and documentation are always `name`, following the model's confidence matrix. Repeated edges retain their strongest permitted confidence. Source spellings and lookup diagnostics are preserved in `Unresolved` so the indexer can retry after definitions change.

`detect_moves(&previous_nodes, &current_nodes, revision)` compares complete snapshots. A disappeared id and a newly introduced id with the same nonempty normalized-body fingerprint and node/symbol kinds produce a `Move`. Copies, changed bodies and ambiguous fingerprint groups do not. `Move::identity()` returns the model's `IdentityStatus::Moved`; the query layer then serves the new node. Fingerprint computation belongs to the parser/indexer.

Integration gaps in the landed store: `MapStore` has no move-record read/write API. Its `Unresolved` retains only name and kind, so persisting resolver retry facts also requires source, scope and binding support (or reparsing the owning file). This crate retains those facts and the supplied move revision without editing the store. The scan crate's public `ScannedFile` remains upstream and is converted by the indexer.

Unit coverage includes the exact expected edges for three Rust modules plus a Markdown section, alias/glob/reexport behavior, missing and ambiguous references, package isolation, ownership errors and rename detection. Property tests shuffle files, definitions and references, and permute move snapshots.
