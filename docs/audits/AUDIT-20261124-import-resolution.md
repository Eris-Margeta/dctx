<!-- SPDX-License-Identifier: Apache-2.0 -->
# Import-resolution audit, 2026-11-24

**30 reproduced findings, 0 suspected, 7 proposed fix groups.** Seventeen findings produced a wrong `exact` edge (F1–F17). Four exposed unreachable call bindings (F18–F21), five dropped supported evidence (F22–F26), and four concern missing features (F27–F30). The default release CLI at `bd82ecf497cc` accepted unrelated files as exact dependencies in path-based, package-based and namespace-based languages. Some of those imports also redirected calls at `name` confidence. A separate omission happens before resolution: the production walk can omit a real module from its candidate inventory even though graph export lists its node.

The 65 commands below are independent cold exports, including controls. Each finding groups variations of one failure at the indicated boundary. Severity is the strongest outcome shown: **wrong-exact** for a false exact endpoint, **unreachable** for an unjustified strong call binding, **dropped** for lost evidence in supported forms, and **missing-feature** for a form without extraction or binding support. An empty projection proves absence in this CLI profile. It makes no claim about what another parser mode emits.

## Reproduce

Run in Bash from the checkout with its build environment. The first package is the library requested for the audit. The second builds the CLI that links it. The card-local Cargo registry lacked `schemars`; using the already populated local Cargo home for the two offline builds succeeded with the original target and cache locations. On a checkout with a populated registry, the commands are:

```sh
umask 022
cargo build --offline --locked -p dctx --release
cargo build --offline --locked -p dctx-cli --release
```

Define this helper once in the same shell. Each invocation writes the supplied JSON filename/source pairs to a fresh source directory and sets `DCTX_CACHE_DIR` to its empty sibling. It verifies the subject is present in the export, then prints the outgoing non-containment edges from all nodes in that file as sorted JSON tuples `[source, relation, target, confidence]`. The reporting assertion covers a newline, ESC and a quote. It applies to exported evidence and subprocess errors.

```sh
audit_runner=$(cat <<'PY'
import sys
import json
import os
import subprocess
import tempfile
from pathlib import Path

cli = (Path(os.environ["CARGO_TARGET_DIR"]) / "release" / "dctx-cli").resolve()

def report(record):
    return json.dumps(record, ensure_ascii=True, separators=(",", ":"))

def probe(subject, files):
    with tempfile.TemporaryDirectory(dir=Path.cwd()) as tmp:
        root = Path(tmp) / "src"
        cache = Path(tmp) / "cache"
        root.mkdir()
        cache.mkdir()
        for name, source in files.items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(source.encode("utf-8"))
        run = subprocess.run([cli, "map", "export", "--format", "json"],
                             cwd=root, env=dict(os.environ, DCTX_CACHE_DIR=str(cache)),
                             capture_output=True)
        if run.returncode:
            raise SystemExit(report([run.returncode, run.stderr.decode(errors="replace")]))
        graph = json.loads(run.stdout)
        owners = {n["id"] for n in graph["nodes"] if n["path"] == subject}
        if not owners:
            raise ValueError(report(["missing subject", subject]))
        edges = sorted([e["src"], e["edge"], e["dst"], e["confidence"]]
                       for e in graph["edges"] if e["edge"] != "contains"
                       and e["src"] in owners)
        return edges

bad = 'dir/\n\x1b"file'
line = report([bad])
assert json.loads(line) == [bad] and line.isascii() and len(line.splitlines()) == 1
assert "\x1b" not in line and r'\"' in line

try:
    print(report(probe(sys.argv[1], json.load(sys.stdin))))
except (OSError, ValueError, KeyError) as err:
    raise SystemExit(report([type(err).__name__, str(err)])) from None
PY
)
probe() { python3 -c "$audit_runner" "$1"; }
```

## Coverage and existing issues

Reviewed the facade walk and `refs.rs`, `refs_names.rs`, `refs_tests.rs`, the first-party producers for JS/TS, Rust, Python, Go, Java, Kotlin, C/C++, C#, Swift, Ruby, PHP, configuration and Markdown, and the generic and embedded-web dispatch. The optional tree-sitter oracle has independent import logic for JS/TS and Rust and document references for Markdown. In `crates/dctx-map-parse/src/adapters/mod.rs:94` the other first-party languages delegate to their pure adapter. That agreement cannot independently validate their spelling. The release CLI probes below exercise the facade, not the optional oracle.

Issues #8 and #9 cover the static extension/member and importer-directory failures. The following variants establish their reach; they are excluded from the finding count. The TS, TSX, JSX, MJS, literal CommonJS require and embedded-script cases should select the file under `p/`. The destructured require should bind `work` to `s:p/a.js#hit`. Issue #10 concerns ES re-exports and #11 concerns exported non-function binding nodes. Neither is counted.

```sh
probe 'p/b.ts' <<'JSON'
{
  "p/b.ts": "import './util.ts';\n",
  "util.ts": "export function hit() {}\n",
  "p/util.ts": "export function hit() {}\n"
}
JSON
probe 'p/b.tsx' <<'JSON'
{
  "p/b.tsx": "import './util.tsx';\n",
  "util.tsx": "export function hit() {}\n",
  "p/util.tsx": "export function hit() {}\n"
}
JSON
probe 'p/b.jsx' <<'JSON'
{
  "p/b.jsx": "import './util.jsx';\n",
  "util.jsx": "export function hit() {}\n",
  "p/util.jsx": "export function hit() {}\n"
}
JSON
probe 'p/b.mjs' <<'JSON'
{
  "p/b.mjs": "import './util.mjs';\n",
  "util.mjs": "export function hit() {}\n",
  "p/util.mjs": "export function hit() {}\n"
}
JSON
probe 'p/b.cjs' <<'JSON'
{
  "p/b.cjs": "require('./util.cjs');\n",
  "util.cjs": "function wrong() {}\n",
  "p/util.cjs": "function right() {}\n"
}
JSON
probe 'p/App.vue' <<'JSON'
{
  "p/App.vue": "<script>\nimport './helper.js';\n</script>\n",
  "helper.js": "export function hit() {}\n",
  "p/helper.js": "export function hit() {}\n"
}
JSON
probe 'p/b.cjs' <<'JSON'
{
  "p/b.cjs": "const {hit: work} = require('./a.js');\nfunction run() { work() }\n",
  "p/a.js": "function hit() {}\nexports.hit = hit;\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/b.ts","imports","m:util.ts","exact"]]
[["m:p/b.tsx","imports","m:util.tsx","exact"]]
[["m:p/b.jsx","imports","m:util.jsx","exact"]]
[["m:p/b.mjs","imports","m:util.mjs","exact"]]
[["m:p/b.cjs","imports","m:util.cjs","exact"]]
[["m:p/App.vue","imports","m:helper.js","exact"]]
[]
```

CSS has no language entry in `crates/dctx/src/index/extract/language.rs:9`. A local stylesheet import is therefore a coverage control. The two remaining controls establish that plain Python and extensionless TypeScript member imports can reach a symbol exactly and bind its call by name. The heuristic families do not share an import grammar. Protobuf and Lua are exercised separately below.

```sh
probe 'main.css' <<'JSON'
{
  "main.css": "@import \"./base.css\";\n",
  "base.css": "p { color: red; }\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "a.py": "def hit():\n    pass\n",
  "b.py": "from a import hit\ndef run():\n    hit()\n"
}
JSON
probe 'b.ts' <<'JSON'
{
  "a.ts": "export function hit() {}\n",
  "b.ts": "import {hit} from './a';\nexport function run() { hit(); }\n"
}
JSON
```

**Actual output**

```jsonl
[]
[["m:b.py","imports","s:a.py#hit","exact"],["s:b.py#run","calls","s:a.py#hit","name"]]
[["m:b.ts","imports","s:a.ts#hit","exact"],["s:b.ts#run","calls","s:a.ts#hit","name"]]
```

## F1. Empty modules vanish from import lookup

**JS, shared walk · wrong-exact · G1**

```sh
probe 'p/b.mjs' <<'JSON'
{
  "p/b.mjs": "import './util.mjs';\n",
  "p/util.mjs": "// side effects only\n",
  "q/util.mjs": "export function other() {}\n"
}
JSON
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import './util.mjs';\n",
  "util.mjs": "// empty module\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/b.mjs","imports","m:q/util.mjs","exact"]]
[]
```

**Expected.** Both imports should be exact. Their respective destinations are `m:p/util.mjs` and `m:util.mjs`. Removing the other directory still leaves a dropped dependency in the second case.

**Cause.** `crates/dctx/src/index/walk.rs:414` records a file for linking only if it produced a reference or a child node. `crates/dctx/src/index/refs.rs:160` constructs module lookup from those records. A comment-only file is in the graph but absent from the lookup table.

**Proposed fix.** Build the module inventory from every included module node. Keep any sparse optimization for outgoing reference scans independent.

**Tests to add.** Compare a comment-only, zero-byte and declaration-bearing target, each with and without a same-named decoy. Use the full walk because the `linked` unit helper registers every input.

## F2. An imported member can become a file

**TypeScript, Python · wrong-exact · G1, G2, G3**

```sh
probe 'b.ts' <<'JSON'
{
  "b.ts": "import { hit } from './a';\nexport function run() { hit(); }\n",
  "a.ts": "export function hit() {}\n",
  "a/hit.ts": "export function other() {}\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "p/__init__.py": "def hit():\n    pass\n",
  "p/hit.py": "def other():\n    pass\n",
  "b.py": "from p import hit\ndef run():\n    hit()\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.ts","imports","m:a/hit.ts","exact"],["s:b.ts#run","calls","s:a.ts#hit","guess"]]
[["m:b.py","imports","m:p/hit.py","exact"],["s:b.py#run","calls","s:p/__init__.py#hit","guess"]]
```

**Expected.** For TypeScript the import should reach `s:a.ts#hit` exactly and the call should reach it by name. For Python both should reach `s:p/__init__.py#hit` at those ranks: that attribute already exists when the package is imported.

**Cause.** `crates/dctx-lex-js/src/extract/modules.rs:144` and `crates/dctx-lex-py/src/extract/imports.rs:88` append a binding to the module spelling. `crates/dctx/src/index/refs.rs:317` tries the entire sequence as a module before it tries the member.

**Proposed fix.** Resolve the module operand first. Apply its export/member rules, including Python’s attribute-before-submodule rule, before installing the local binding.

**Tests to add.** Add and remove a populated submodule named after the exported function. Assert that the call keeps its destination and rank.

## F3. Namespace punctuation and reserved words are stripped across languages

**Python · wrong-exact · G1, G3**

```sh
probe 'b.py' <<'JSON'
{
  "b.py": "import static.tools\n",
  "static/__init__.py": "def pkg():\n    pass\n",
  "static/tools.py": "def hit():\n    pass\n",
  "tools.py": "def other():\n    pass\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "p/__init__.py": "def parent():\n    pass\n",
  "p/h.py": "def hit():\n    pass\n",
  "b.py": "import p.h\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "p/__init__.py": "",
  "p/main.py": "def hit():\n    return 1\n",
  "b.py": "from p.main import hit\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.py","imports","m:tools.py","exact"]]
[["m:b.py","imports","m:p/__init__.py","exact"]]
[]
```

**Expected.** The exact targets should be `m:static/tools.py`, `m:p/h.py` and `s:p/main.py#hit`, respectively. The first segment is an ordinary package name, the second example imports a module named `h`, and `main.py` is an ordinary Python module.

**Cause.** `crates/dctx/src/index/refs_names.rs:18` applies one keyword list to all languages. Line 47 strips trailing `.h` even in a dotted import. Line 78 gives `main`, `lib`, `mod`, `index` and `__init__` directory-entry semantics across languages.

**Proposed fix.** Make keyword, extension and entry-file rules specific to the language and syntactic form. Index the full inventory path even where a language supplies a directory alias.

**Tests to add.** Keep all three fixtures. Vary `main` with `index`, `mod` and `lib`. Pair each with an actual Python package initializer and a real C header.

## F4. Relative levels and absolute roots disappear

**Python · wrong-exact · G1, G3**

```sh
probe 'p/b.py' <<'JSON'
{
  "p/__init__.py": "",
  "p/b.py": "from . import util\ndef run():\n    util.hit()\n",
  "util.py": "def hit():\n    return 1\n",
  "p/util.py": "def hit():\n    return 1\n"
}
JSON
probe 'p/sub/b.py' <<'JSON'
{
  "p/__init__.py": "",
  "p/sub/__init__.py": "",
  "p/sub/b.py": "from ..util import hit\n",
  "util.py": "def hit():\n    return 1\n",
  "p/util.py": "def hit():\n    return 1\n"
}
JSON
probe 'p/b.py' <<'JSON'
{
  "p/__init__.py": "",
  "p/b.py": "from .util import hit\n",
  "p/util.py": "def hit():\n    return 1\n",
  "q/util.py": "def hit():\n    return 1\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "b.py": "import util\n",
  "unrelated/util.py": "def hit():\n    return 1\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/b.py","imports","m:util.py","exact"],["s:p/b.py#run","calls","s:util.py#hit","name"]]
[["m:p/sub/b.py","imports","s:util.py#hit","exact"]]
[]
[["m:b.py","imports","m:unrelated/util.py","exact"]]
```

**Expected.** First: exact import of `m:p/util.py`, then name call to `s:p/util.py#hit`. Second and third: exact import of `s:p/util.py#hit`. Last: `[]` with the fixture root as the only project import root. The unrelated directory is not on the search path.

**Cause.** `crates/dctx-lex-py/src/extract/imports.rs:119` reads leading dots; line 88 joins the member. `crates/dctx/src/index/refs_names.rs:24` drops the dots and `crates/dctx/src/index/refs.rs:327` picks a full or unique suffix without the importing package.

**Proposed fix.** Carry the relative level, find the importing package, ascend that number of levels and resolve from known roots. Detect ascents beyond the package.

**Tests to add.** Assert the four outcomes and add a failing above-root ascent. Add a shadow package outside the configured import root without changing any expected edge.

## F5. Rust module declarations and use anchors escape the module tree

**Rust · wrong-exact · G1, G4**

```sh
probe 'p/b.rs' <<'JSON'
{
  "lib.rs": "mod p;\n",
  "p/mod.rs": "pub mod util;\npub mod b;\n",
  "p/b.rs": "use super::util::hit;\npub fn run() { hit(); }\n",
  "p/util.rs": "pub fn hit() {}\n",
  "util.rs": "pub fn hit() {}\n"
}
JSON
probe 'p.rs' <<'JSON'
{
  "lib.rs": "mod p;\n",
  "p.rs": "mod util;\nuse self::util::hit;\nfn run() { hit(); }\n",
  "p/util.rs": "pub fn hit() {}\n",
  "util.rs": "pub fn hit() {}\n"
}
JSON
probe 'p.rs' <<'JSON'
{
  "lib.rs": "mod p;\n",
  "p.rs": "mod util;\n",
  "p/util.rs": "pub fn hit() {}\n",
  "util.rs": "pub fn unrelated() {}\n"
}
JSON
probe 'lib.rs' <<'JSON'
{
  "lib.rs": "#[path = \"actual.rs\"]\nmod item;\n",
  "actual.rs": "pub fn hit() {}\n",
  "item.rs": "pub fn unrelated() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/b.rs","imports","s:util.rs#hit","exact"],["s:p/b.rs#run","calls","s:util.rs#hit","name"]]
[["m:p.rs","imports","m:util.rs","exact"],["m:p.rs","imports","s:util.rs#hit","exact"],["s:p.rs#run","calls","s:util.rs#hit","name"]]
[["m:p.rs","imports","m:util.rs","exact"]]
[["m:lib.rs","imports","m:item.rs","exact"]]
```

**Expected.** The `super` import/call should reach `s:p/util.rs#hit` at `exact`/`name`. For `self`, the module and symbol should reach `m:p/util.rs` and `s:p/util.rs#hit`. The standalone `mod util;` should reach `m:p/util.rs`. The explicit path should reach `m:actual.rs`. The root decoy is not declared in these crates.

**Cause.** `crates/dctx-lex-rust/src/adapters/syntax_refs.rs:150` emits only the module identifier for a file declaration. `crates/dctx/src/index/syn_extract.rs:104` erases the import kind. `crates/dctx/src/index/refs_names.rs:18` discards `self` and `super`; `crates/dctx/src/index/refs.rs:255` receives no module context.

**Proposed fix.** Carry file-module versus use-tree intent, literal path overrides and module ancestry. Follow declarations from the crate root, applying the child-file and inline-module rules.

**Tests to add.** Retain the four fixtures, then change `p.rs` to `p/mod.rs` and add an inline module and a second `super`. Check undeclared decoys never enter the candidate set.

## F6. Quote, angle and filename extension collapse for headers

**C, C++ · wrong-exact · G1, G6**

```sh
probe 'p/a.c' <<'JSON'
{
  "p/a.c": "#include \"item.h\"\n",
  "p/item.h": "#define P 1\n",
  "item.h": "#define Q 2\n"
}
JSON
probe 'p/a.cpp' <<'JSON'
{
  "p/a.cpp": "#include <vector>\n",
  "vendor/vector.cpp": "int unrelated() { return 1; }\n"
}
JSON
probe 'main.c' <<'JSON'
{
  "main.c": "#include \"api.h\"\n",
  "api.h": "#define A 1\n",
  "api.c": "int a;\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/a.c","imports","m:item.h","exact"]]
[["m:p/a.cpp","imports","m:vendor/vector.cpp","exact"]]
[]
```

**Expected.** Quoted search should select `m:p/item.h` exactly. With no configured include root containing `vector`, the project edge for `<vector>` should be absent. The last import should reach `m:api.h` exactly even beside `api.c`.

**Cause.** `crates/dctx-lex-c/src/lexer/preprocessor.rs:195` returns the contents of either delimiter. `crates/dctx-lex-c/src/extract/declarations.rs:90` forwards the same form. `crates/dctx/src/index/refs_names.rs:47` and line 73 discard extensions on each side.

**Proposed fix.** Keep delimiter, directive and complete filename. Search the local and configured include directories in compiler order. Leave external headers unresolved when that order gives no in-view hit.

**Tests to add.** Run identical basenames with quote and angle forms, two ordered include roots, and a header plus an implementation file. Check the negative edge to the `.cpp` decoy.

## F7. A C++ using-directive imports a translation unit

**C++ · wrong-exact · G1, G5, G6**

```sh
probe 'b.cpp' <<'JSON'
{
  "b.cpp": "namespace mylib { int hit() { return 1; } }\nusing namespace mylib;\n",
  "mylib.cpp": "int other() { return 0; }\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.cpp","imports","m:mylib.cpp","exact"]]
```

**Expected.** No file import to `m:mylib.cpp`. The directive makes a namespace declared in `b.cpp` visible for lookup.

**Cause.** `crates/dctx-lex-c/src/extract/declarations.rs:207` emits `using namespace` with the same import kind as an include. `crates/dctx/src/index/refs.rs:317` interprets the namespace identifier as a module filename.

**Proposed fix.** Distinguish namespace directives and using-declarations from includes. Resolve the former to eligible declarations, or explicitly abstain in the file-dependency graph.

**Tests to add.** Introduce, rename and remove an unrelated same-named translation unit. The namespace directive must never create a dependency on it.

## F8. Package imports resolve to Go filenames

**Go · wrong-exact · G1, G5**

```sh
probe 'cmd/main.go' <<'JSON'
{
  "go.mod": "module audit\n\ngo 1.22\n",
  "cmd/main.go": "package main\nimport \"audit/tools\"\nfunc main() { tools.Hit() }\n",
  "tools/impl.go": "package tools\nfunc Hit() {}\n",
  "audit/tools.go": "package audit\nfunc Other() {}\n"
}
JSON
probe 'main.go' <<'JSON'
{
  "go.mod": "module audit\n\ngo 1.22\n",
  "main.go": "package main\nimport \"fmt\"\nfunc main() { fmt.Println(1) }\n",
  "unused/fmt.go": "package unused\nfunc Println(i int) {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:cmd/main.go","imports","m:audit/tools.go","exact"],["s:cmd/main.go#main","calls","s:tools/impl.go#Hit","guess"]]
[["m:main.go","imports","m:unused/fmt.go","exact"],["s:main.go#main","calls","s:unused/fmt.go#Println","name"]]
```

**Expected.** For `audit/tools` the package owns `tools/impl.go`; `main` should call `s:tools/impl.go#Hit` by name. An exact dependency can use the known package or its sole eligible file. For standard `fmt`, neither an import nor a call should target `unused/fmt.go`.

**Cause.** `crates/dctx-lex-go/src/extract/declarations.rs:81` emits the decoded package path and alias. `crates/dctx/src/index/refs.rs:165` indexes file-derived keys; line 317 gives them priority. Neither operation associates go.mod module paths with package directories.

**Proposed fix.** Index packages by module prefix and relative directory, keep the declared package name and bind exported members across eligible files. Abstain for external packages without ownership evidence.

**Tests to add.** Retain both decoys, add a second file in the local package and an explicit alias. Check blank imports have a dependency but no local binding.

## F9. The declared JVM package loses to the source path

**Java, Kotlin · wrong-exact · G1, G5**

```sh
probe 'Main.java' <<'JSON'
{
  "Main.java": "import p.Widget;\nclass Main { Widget w; }\n",
  "p/Widget.java": "package other;\npublic class Widget {}\n",
  "src/p/Widget.java": "package p;\npublic class Widget {}\n"
}
JSON
probe 'Main.kt' <<'JSON'
{
  "Main.kt": "import p.hit as work\nfun run() = work()\n",
  "p/hit.kt": "package other\nfun other() = 1\n",
  "Util.kt": "package p\nfun hit() = 2\n"
}
JSON
probe 'Main.java' <<'JSON'
{
  "Main.java": "import static p.Widget.hit;\nclass Main { int run() { return hit(); } }\n",
  "p/Widget.java": "package p;\npublic class Widget { public static int hit() { return 1; } }\n"
}
JSON
```

**Actual output**

```jsonl
[["m:Main.java","imports","m:p/Widget.java","exact"]]
[["m:Main.kt","imports","m:p/hit.kt","exact"]]
[["s:Main.java#Main::run","calls","s:p/Widget.java#Widget::hit","guess"]]
```

**Expected.** The Java import should reach `s:src/p/Widget.java#Widget` exactly. The competing class declares package `other`. Kotlin should import `s:Util.kt#hit` exactly and call it by name through `work`. For the static import expect `s:p/Widget.java#Widget::hit` at import `exact` and call `name`.

**Cause.** Java and Kotlin record package declarations at `crates/dctx-lex-java/src/extract/declarations.rs:59` and `crates/dctx-lex-kt/src/extract/declarations.rs:214`. `crates/dctx/src/index/lex_dispatch.rs:97` skips the module metadata. `crates/dctx/src/index/refs.rs:317` prefers paths, and line 322 looks for a bare member rather than a member of the imported type.

**Proposed fix.** Build a source-set index of declared packages, types and members. Resolve Java static imports within the selected owner and Kotlin top-level members across files.

**Tests to add.** Move a source without changing its declaration. Include two Kotlin files in one package, an alias and a Java static import with a competing member.

## F10. A C# type alias binds a file in another namespace

**C# · wrong-exact · G1, G5**

```sh
probe 'Main.cs' <<'JSON'
{
  "Main.cs": "using Alias = P.Widget;\nclass MainClass { Alias value; }\n",
  "P/Widget.cs": "namespace Other { class Widget {} }\n",
  "Types.cs": "namespace P { class Widget {} }\n"
}
JSON
```

**Actual output**

```jsonl
[["m:Main.cs","imports","m:P/Widget.cs","exact"]]
```

**Expected.** The exact import target should be `s:Types.cs#P::Widget`; the path-shaped decoy declares `Other.Widget`.

**Cause.** `crates/dctx-lex-cs/src/extract/declarations.rs:33` preserves the alias target. `crates/dctx/src/index/refs.rs:317` looks for a file key before considering a symbol and never indexes full declared namespace identities across the compilation.

**Proposed fix.** Resolve alias right-hand sides in the compilation namespace/type index. Distinguish namespace using, type alias and using-static.

**Tests to add.** Move both definitions, use a file-scoped namespace, and keep a path-identical decoy in a different namespace.

## F11. An SDK module imports a local Swift source file

**Swift · wrong-exact · G1, G5**

```sh
probe 'App.swift' <<'JSON'
{
  "App.swift": "import Foundation\n",
  "Foundation.swift": "func custom() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:App.swift","imports","m:Foundation.swift","exact"]]
```

**Expected.** No exact project import of `m:Foundation.swift`. Compiling the two fixture files in the same application module does not turn that source file into the imported Foundation module.

**Cause.** `crates/dctx-lex-swift/src/extract/declarations.rs:294` emits the dotted module/declaration name. `crates/dctx/src/index/refs.rs:165` and line 317 interpret it as a file stem.

**Proposed fix.** Use compiler or package target membership. Leave a module external/unresolved without a known in-view target.

**Tests to add.** Test an SDK-name decoy and a real local target composed of multiple source files with unrelated names.

## F12. A relative Ruby load escapes its directory

**Ruby · wrong-exact · G1, G6**

```sh
probe 'p/b.rb' <<'JSON'
{
  "p/b.rb": "require_relative 'util'\n",
  "p/util.rb": "X = 1\n",
  "util.rb": "Y = 2\n"
}
JSON
```

**Actual output**

```jsonl
[["m:p/b.rb","imports","m:util.rb","exact"]]
```

**Expected.** The sole exact import should be `m:p/util.rb`.

**Cause.** `crates/dctx-lex-rb/src/extract/references.rs:12` accepts `require_relative`, `require` and `load`; line 29 stores only the literal. `crates/dctx/src/index/refs.rs:327` then prefers the root path.

**Proposed fix.** Carry the loader operation with the path. Anchor `require_relative` to the file and consult configured load paths only for forms that use them.

**Tests to add.** Repeat with `../`, add a same-named file to a load directory and remove the reachable file. A miss must remain unresolved.

## F13. PHP namespace imports and includes share a file lookup

**PHP · wrong-exact · G1, G5, G6**

```sh
probe 'b.php' <<'JSON'
{
  "b.php": "<?php\nuse function P\\hit;\nhit();\n",
  "P/hit.php": "<?php namespace Other;\nfunction hit() {}\n",
  "real.php": "<?php namespace P;\nfunction hit() {}\n"
}
JSON
probe 'app.php' <<'JSON'
{
  "app.php": "<?php require './missing/util.php';\n",
  "other/missing/util.php": "<?php function x() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.php","calls","s:real.php#P::hit","guess"],["m:b.php","imports","m:P/hit.php","exact"]]
[["m:app.php","imports","m:other/missing/util.php","exact"]]
```

**Expected.** For the function import expect `s:real.php#P::hit` at `exact` and the call to it at `name`, assuming the two definitions are loaded. For the explicit dot path, with the process rooted beside `app.php`, expect `[]`. If no working directory is known, there is still no evidence for the other-directory edge.

**Cause.** `crates/dctx-lex-php/src/extract/references.rs:53` advances past `function`/`const`; line 106 emits a name, and line 143 emits a literal include under the same kind. `crates/dctx/src/index/refs.rs:317` applies the file lookup to both.

**Proposed fix.** Preserve namespace imports and their declaration kind separately from file includes. Resolve explicit dot paths against a known working directory and namespace aliases against loaded declarations.

**Tests to add.** Test a grouped function alias beside a same-path class/file decoy. Test a known and an unknown working directory and an include relative to `__DIR__`.

## F14. Literal punctuation changes which JavaScript module wins

**JavaScript ESM · wrong-exact · G1, G2**

```sh
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import './a-b.mjs';\n",
  "a-b.mjs": "export function real() {}\n",
  "a/b.mjs": "export function other() {}\n"
}
JSON
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import './\\u0061.mjs';\n",
  "a.mjs": "export function hit() {}\n"
}
JSON
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import './a.mjs?v=1#tag';\n",
  "a.mjs": "export function hit() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.mjs","imports","m:a/b.mjs","exact"]]
[]
[]
```

**Expected.** The literal hyphen should select `m:a-b.mjs`. The escaped `a` and the relative URL with query and fragment should both have `m:a.mjs` as their file dependency. Module instances with different queries can still have distinct identities.

**Cause.** `crates/dctx-lex-js/src/extract/calls.rs:270` removes quotes without cooking escapes. `crates/dctx/src/index/refs_names.rs:24` splits punctuation into identifiers, so `a-b` becomes the key for `a/b`. Line 47 tests the end of the joined string for an extension, including any URL suffix.

**Proposed fix.** Decode literal strings at the producer and parse local ESM URLs before inventory lookup. Preserve literal filename characters and keep the URL suffix separate from the underlying path.

**Tests to add.** Retain all three cases, add a literal dot in a basename and change the unrelated directory tree without changing the chosen file.

## F15. A Node built-in resolves inside the project

**JavaScript ESM · wrong-exact · G1, G2**

```sh
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import 'node:util';\n",
  "node/util.mjs": "export function other() {}\n"
}
JSON
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "import 'util';\n",
  "unused/util.mjs": "export function unrelated() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.mjs","imports","m:node/util.mjs","exact"]]
[["m:b.mjs","imports","m:unused/util.mjs","exact"]]
```

**Expected.** No project import in either case. In the Node environment `node:util` names a built-in and bare `util` is also a built-in. Neither fixture declares a package that could supply the target.

**Cause.** `crates/dctx/src/index/refs_names.rs:24` discards the colon; `crates/dctx/src/index/refs.rs:327` then accepts a full file key or any unique suffix.

**Proposed fix.** Classify schemes, built-ins, bare packages and local paths before lookup. Record an external dependency where the model supports it.

**Tests to add.** Pair both spellings with a real local `./util` and a scoped package. Test a missing package next to an identically named source file.

## F16. A shadowed CommonJS loader creates an exact dependency

**CommonJS · wrong-exact · G1, G2**

```sh
probe 'b.cjs' <<'JSON'
{
  "b.cjs": "function load(require) { return require('./a.cjs'); }\n",
  "a.cjs": "function unrelated() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.cjs","imports","m:a.cjs","exact"]]
```

**Expected.** No import of `m:a.cjs`. The only call is through the function parameter, whose value is unknown.

**Cause.** `crates/dctx-lex-js/src/extract/calls.rs:53` recognizes any bare callee spelled `require` followed by a literal. The oracle has the same name-only check in `crates/dctx-map-parse/src/adapters/javascript_references.rs:143`. `crates/dctx/src/index/refs.rs:261` promotes a successfully matched convention to exact.

**Proposed fix.** Only classify a literal call as a loader when the built-in binding is available and unshadowed. Preserve convention evidence through projection.

**Tests to add.** Compare the parameter with a genuine loader in the same file and a local function named `require`. Assert only the genuine loader introduces a dependency.

## F17. An unexported ES function is importable by name

**TypeScript · wrong-exact · G1, G2**

```sh
probe 'b.ts' <<'JSON'
{
  "b.ts": "import { hidden } from './a';\nexport function run() { hidden(); }\n",
  "a.ts": "function hidden() {}\nexport function publicFn() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.ts","imports","s:a.ts#hidden","exact"],["s:b.ts#run","calls","s:a.ts#hidden","name"]]
```

**Expected.** No edge to `s:a.ts#hidden` as an import or named call. The module exists, but the requested binding is absent from its exports. A module-level dependency alone is defensible.

**Cause.** `crates/dctx-lex-js/src/extract/functions.rs:100` records declaration visibility. `crates/dctx/src/index/lex_dispatch.rs:118` drops it during projection. `crates/dctx/src/index/refs.rs:322` searches the module’s full symbol table.

**Proposed fix.** Build an export surface from public declarations and supported explicit export forms. Install a binding only for an exported value.

**Tests to add.** Toggle only the `export` keyword. Include a shadowed private value and a public alias. Verify both the import and the confidence of any call.

## F18. Local imports leak across functions and parameters

**Python · unreachable · G1, G3**

```sh
probe 'b.py' <<'JSON'
{
  "a.py": "def hit():\n    return 1\n",
  "c.py": "def hit():\n    return 1\n",
  "b.py": "def left():\n    from a import hit\n    hit()\ndef right():\n    from c import hit\n    hit()\n"
}
JSON
probe 'b.py' <<'JSON'
{
  "b.py": "from a import hit\ndef run(hit):\n    return hit()\n",
  "a.py": "def hit():\n    pass\n"
}
JSON
```

**Actual output**

```jsonl
[["s:b.py#left","calls","s:a.py#hit","name"],["s:b.py#left","imports","s:a.py#hit","exact"],["s:b.py#right","calls","s:a.py#hit","name"],["s:b.py#right","imports","s:c.py#hit","exact"]]
[["m:b.py","imports","s:a.py#hit","exact"],["s:b.py#run","calls","s:a.py#hit","name"]]
```

**Expected.** Both import edges in the first case are correct. The call in `right` must reach `s:c.py#hit` with `name`, while `left` keeps `s:a.py#hit`. In the second case keep the import but omit the call edge: its target is the caller-supplied parameter.

**Cause.** `crates/dctx/src/index/refs.rs:99` stores no scope on a binding. Lines 245 and 380 build a file-wide vector and accept the first match. The parameter never masks that entry.

**Proposed fix.** Key bindings by lexical owner and account for shadowing before resolving a call. Abstain when a parameter has an unknown value.

**Tests to add.** Reverse the order of the functions, use the same alias in each, and call through a same-named parameter in a third function.

## F19. A qualified call acquires name confidence from a filename

**Python, also JS · unreachable · G1**

```sh
probe 'b.py' <<'JSON'
{
  "b.py": "def run(util):\n    return util.hit()\n",
  "unrelated/util.py": "def hit():\n    return 1\n"
}
JSON
probe 'b.mjs' <<'JSON'
{
  "a.mjs": "export function hit() {}\n",
  "b.mjs": "import './a.mjs';\nexport function run(a) { a.hit(); }\n"
}
JSON
```

**Actual output**

```jsonl
[["s:b.py#run","calls","s:unrelated/util.py#hit","name"]]
[["m:b.mjs","imports","m:a.mjs","exact"],["s:b.mjs#run","calls","s:a.mjs#hit","name"]]
```

**Expected.** Omit the call in both cases. Each receiver is a parameter. The JS side-effect import should still reach `m:a.mjs` exactly and bind no name.

**Cause.** `crates/dctx/src/index/refs.rs:268` synthesizes bindings from the first and last segment when no alias exists, including side-effect imports. Even with no import at all, line 403 tries the receiver as a global module key and line 429 labels a single symbol `name`.

**Proposed fix.** Represent binding-free dependencies explicitly. Require a visible module/type binding or receiver evidence for `name`. An unknown parameter supplies neither.

**Tests to add.** Repeat with the receiver and on-disk module renamed independently, and with an actual namespace alias as the positive control.

## F20. A dollar sign changes the called identifier

**JavaScript · unreachable · G1, G2**

```sh
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "export function run() { return $hit(); }\nfunction $hit() {}\nfunction hit() {}\n"
}
JSON
```

**Actual output**

```jsonl
[["s:b.mjs#run","calls","s:b.mjs#hit","name"]]
```

**Expected.** The name call from `s:b.mjs#run` should reach `s:b.mjs#$hit`, the distinct function named in the source.

**Cause.** `crates/dctx/src/index/refs_names.rs:26` removes `$` from call segments, but `crates/dctx/src/index/refs.rs:300` considers it part of a bare identifier. Line 369 finds the other symbol locally.

**Proposed fix.** Use the language’s identifier segmentation for calls and imported members. Keep `$` and other valid identifier characters across lookup.

**Tests to add.** Put both functions in one file, then export and import both with aliases. Assert that each call keeps its spelling.

## F21. Type-only imports become callable runtime bindings

**TypeScript · unreachable · G1, G2**

```sh
probe 'b.ts' <<'JSON'
{
  "a.ts": "export type Item = () => number;\n",
  "b.ts": "import type {Item} from './a';\ndeclare const Item: () => number;\nexport function run() { return Item(); }\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.ts","imports","s:a.ts#Item","exact"],["s:b.ts#run","calls","s:a.ts#Item","name"]]
```

**Expected.** Keep the type dependency on `s:a.ts#Item`. Omit the cross-file call: the call uses a separately declared value, while the imported type alias has no runtime value.

**Cause.** `crates/dctx-lex-js/src/extract/modules.rs:16` advances past the type-only marker and line 86 does the same for individual specifiers. `crates/dctx/src/index/refs.rs:399` treats the imported symbol as a callable by name.

**Proposed fix.** Carry the type/value space of every import and consult only value-space bindings for a runtime call.

**Tests to add.** Repeat with specifier-level `type`, a type and value of the same name, and an ordinary value import as a positive control.

## F22. Public use aliases disappear at the crate boundary

**Rust · dropped · G1, G4**

```sh
probe 'lib.rs' <<'JSON'
{
  "lib.rs": "mod inner;\npub use crate::inner::hit as api;\n",
  "inner.rs": "pub fn hit() {}\n"
}
JSON
probe 'client.rs' <<'JSON'
{
  "lib.rs": "mod inner;\npub use crate::inner::hit as api;\nmod client;\n",
  "inner.rs": "pub fn hit() {}\n",
  "client.rs": "use crate::api;\npub fn run() { api(); }\n"
}
JSON
```

**Actual output**

```jsonl
[["m:lib.rs","imports","m:inner.rs","exact"],["m:lib.rs","imports","s:inner.rs#hit","exact"]]
[]
```

**Expected.** In the first export keep the imports and also expose an exact re-export of `s:inner.rs#hit` under `api`. In the downstream file expect an exact import and a name call to `s:inner.rs#hit` through `crate::api`.

**Cause.** `crates/dctx-lex-rust/src/adapters/syntax_refs.rs:102` records public use leaves and aliases. `crates/dctx/src/index/syn_extract.rs:105` converts all of them to imports. Compare the paired re-export at `crates/dctx-map-parse/src/adapters/rust.rs:249` and the tree-sitter branch at `rust_references.rs:128` in that directory.

**Proposed fix.** Project public uses as re-exports and resolve downstream names through the exported alias with cycle detection and visibility checks.

**Tests to add.** Compare `pub use`, private `use` and restricted visibility. Assert the downstream import and call as well as the barrel’s own edge.

## F23. A named default declaration has no default binding

**TypeScript · dropped · G1, G2**

```sh
probe 'b.ts' <<'JSON'
{
  "b.ts": "import work from './a';\nexport function run() { work(); }\n",
  "a.ts": "export default function hit() {}\n"
}
JSON
probe 'b.ts' <<'JSON'
{
  "b.ts": "import work from './a';\nexport function run() { work(); }\n",
  "a.ts": "export default function() {}\n"
}
JSON
```

**Actual output**

```jsonl
[]
[["m:b.ts","imports","s:a.ts#default","exact"],["s:b.ts#run","calls","s:a.ts#default","name"]]
```

**Expected.** The named form should import `s:a.ts#hit` exactly and call it by name. The anonymous positive control already resolves to `s:a.ts#default`. Neither importer has a filename extension, which isolates this from #8.

**Cause.** `crates/dctx-lex-js/src/extract/functions.rs:50` keeps the local name when present; `crates/dctx-lex-js/src/extract/modules.rs:35` requests `default`. `crates/dctx/src/index/refs.rs:322` compares the strings without an export-name mapping.

**Proposed fix.** Record `default` as an export name mapped to the declaration, independently of its local identifier.

**Tests to add.** Compare named and anonymous default functions and classes. Use an importer alias unrelated to either source name.

## F24. Wildcard import loses the explicit Python export set

**Python · dropped · G1, G3**

```sh
probe 'b.py' <<'JSON'
{
  "a.py": "__all__ = [\"hit\"]\ndef hit():\n    return 1\n",
  "c.py": "def hit():\n    return 1\n",
  "b.py": "from a import *\ndef run():\n    hit()\n"
}
JSON
```

**Actual output**

```jsonl
[["m:b.py","imports","m:a.py","exact"]]
```

**Expected.** Keep the exact import of `m:a.py` and add the `name` call from `s:b.py#run` to `s:a.py#hit`. The unimported competitor in `c.py` is irrelevant.

**Cause.** `crates/dctx-lex-py/src/extract/imports.rs:63` emits `a::*`. `crates/dctx/src/index/refs_names.rs:55` removes the star. `crates/dctx/src/index/refs.rs:268` installs a module binding instead of its exported names. The existing `__all__` scan in `crates/dctx/src/index/export_list.rs:17` feeds outline ordering (`walk.rs:386`), not bindings.

**Proposed fix.** Keep a wildcard form and consume a statically proven export list. Distinguish an explicitly empty list from an unknown list.

**Tests to add.** Check included and excluded names with competing declarations in a third module, empty `__all__`, and a dynamically constructed list that must abstain.

## F25. Configuration path references stop before linking

**TypeScript project configuration (JSON) · dropped · G1, G7**

```sh
probe 'tsconfig.json' <<'JSON'
{
  "tsconfig.json": "{\"extends\":\"./base.json\"}\n",
  "base.json": "{\"compilerOptions\":{}}\n"
}
JSON
```

**Actual output**

```jsonl
[]
```

**Expected.** With this literal `extends` field, expect an exact dependency from `s:tsconfig.json#extends` to `m:base.json`. Both endpoints are present.

**Cause.** `crates/dctx-lex-cfg/src/path.rs:581` recognizes an obvious relative path and line 825 emits `Imports` with the key as owner. `crates/dctx/src/index/extract.rs:68` returns an empty raw reference vector on the successful config path.

**Proposed fix.** Project config references with their key owners. Confirm manifest-specific semantics before linking paths; ordinary strings and dependency names need different treatment.

**Tests to add.** Pair `extends` with an arbitrary field holding the same string, a nested config with a decoy base and a missing base.

## F26. Local Markdown links lose their documentation relation

**Markdown · dropped · G1, G7**

```sh
probe 'guide.md' <<'JSON'
{
  "guide.md": "# Guide\n\n[Notes](notes.md)\n",
  "notes.md": "# Notes\n"
}
JSON
```

**Actual output**

```jsonl
[]
```

**Expected.** Expect `["d:guide.md#Guide","documents","m:notes.md","name"]` for the local link.

**Cause.** `crates/dctx/src/index/extract/source.rs:60` iterates headings only; `crates/dctx/src/index/extract.rs:97` supplies no raw refs for Markdown. Compare `crates/dctx-map-parse/src/adapters/markdown.rs:52` and line 232 for the pure and tree-sitter document paths. `crates/dctx/src/index/refs.rs:40` would also filter `Documents`.

**Proposed fix.** Project supported link destinations with the heading owner, distinguish them from code mentions, then resolve relative to the document. Use the confidence permitted for `documents`.

**Tests to add.** Move the document to a subdirectory and provide a root-level decoy, a fragment and an external URL. Assert the relation kind as well as the path.

## F27. Literal dynamic imports have no dependency

**JavaScript ESM · missing-feature · G1, G2**

```sh
probe 'b.mjs' <<'JSON'
{
  "b.mjs": "export function load() { return import('./a.mjs'); }\n",
  "a.mjs": "export function hit() {}\n"
}
JSON
```

**Actual output**

```jsonl
[]
```

**Expected.** A dependency from the loading site in `b.mjs` to `m:a.mjs` is statically knowable. The literal supplies no local synchronous binding.

**Cause.** `crates/dctx-lex-js/src/extract/statements.rs:92` excludes call syntax from the import statement path. `crates/dctx-lex-js/src/extract/calls.rs:53` handles literal `require` only. The optional JS oracle’s call-expression arm in `crates/dctx-map-parse/src/adapters/javascript_references.rs:14` has the same coverage limit.

**Proposed fix.** Emit a distinct literal dynamic import fact, anchor it to the importer and keep the expression’s owner. Explicitly abstain on computed specifiers.

**Tests to add.** Compare a literal, a variable and a literal inside a function in another directory. No variant should create a namespace binding merely by loading.

## F28. TS and CommonJS assignment forms lose the exported value

**TypeScript, CommonJS · missing-feature · G1, G2**

```sh
probe 'b.ts' <<'JSON'
{
  "b.ts": "import work = require('./a');\nexport function run() { return work.hit(); }\n",
  "a.ts": "export function hit() {}\n"
}
JSON
probe 'b.ts' <<'JSON'
{
  "b.ts": "import work from './a';\nexport function run() { work(); }\n",
  "a.ts": "function hit() {}\nexport = hit;\n",
  "tsconfig.json": "{\"compilerOptions\":{\"module\":\"commonjs\",\"esModuleInterop\":true}}\n"
}
JSON
probe 'b.cjs' <<'JSON'
{
  "b.cjs": "const run = require('./a.cjs');\nfunction use() { run(); }\n",
  "a.cjs": "function hit() {}\nmodule.exports = hit;\n"
}
JSON
```

**Actual output**

```jsonl
[]
[]
[["m:b.cjs","imports","m:a.cjs","exact"]]
```

**Expected.** The import-equals consumer should load `m:a.ts` and call `s:a.ts#hit` by name. With the explicit interop configuration, the default consumer should bind/call `s:a.ts#hit`. The CommonJS dependency already exists, but `s:b.cjs#use` should call `s:a.cjs#hit` by name through the statically assigned export.

**Cause.** `crates/dctx-lex-js/src/extract/modules.rs:32` skips import-equals, and `crates/dctx-lex-js/src/extract/statements.rs:151` skips export-equals. `crates/dctx-lex-js/src/extract/calls.rs:245` binds a plain require to the module. `crates/dctx/src/index/refs.rs:399` accepts only non-module bindings as bare callees. The outline export-name scan (`crates/dctx/src/index/export_list.rs:13`) supplies no callable target.

**Proposed fix.** Model literal import-equals and unconditional statically identifiable export assignments, with the configured interop semantics. Keep runtime/computed assignments unresolved.

**Tests to add.** Test each producer/consumer independently, then chain them. Include a conditional reassignment and a module that exports an object instead of a function.

## F29. The heuristic protobuf index skips literal imports

**Protocol Buffers · missing-feature · G1, G7**

```sh
probe 'b.proto' <<'JSON'
{
  "b.proto": "syntax = \"proto3\";\nimport \"types.proto\";\nmessage Caller { Item item = 1; }\n",
  "types.proto": "syntax = \"proto3\";\nmessage Item {}\n"
}
JSON
```

**Actual output**

```jsonl
[]
```

**Expected.** Under an import root equal to the fixture root, expect an exact import from `m:b.proto` to `m:types.proto`. If import roots are unknown, expose that limitation.

**Cause.** `crates/dctx/src/index/generic/mod.rs:71` dispatches references to the heuristic families. `crates/dctx/src/index/generic/calls_infra.rs:207` scans supported calls and never emits a protobuf import.

**Proposed fix.** Add literal protobuf import facts and resolve using ordered protobuf import roots. Keep normal, public and weak forms distinct.

**Tests to add.** Use two roots containing the same spelled path, a public import, and a missing path. Pair with a string literal that is not an import directive.

## F30. Lua literal loads have no dependency even with a fixed search path

**Lua · missing-feature · G1, G7**

```sh
probe 'b.lua' <<'JSON'
{
  "b.lua": "package.path = \"./?.lua\"\nlocal m = require(\"a\")\nreturn m\n",
  "a.lua": "local function hit() end\nreturn {hit = hit}\n"
}
JSON
```

**Actual output**

```jsonl
[]
```

**Expected.** With the fixture directory as working directory, an unmodified standard Lua loader and no preloaded module named `a`, the stated search template selects `m:a.lua`. Expect a dependency from `m:b.lua`.

**Cause.** `crates/dctx/src/index/generic/calls_block.rs:275` masks Lua strings and line 343 emits call references within symbols. No top-level dependency with a literal module name reaches the resolver.

**Proposed fix.** Recognize the unshadowed literal loader and resolve only with an established package search template and base. Reject unknown loader hooks.

**Tests to add.** Test the shown fixed template and an unknown runtime template, then a local parameter named `require`.

## Suggested fix groups

These are file ownership sets for future work. Names in braces expand to the enumerated files, and files under `index/imports/` are proposed modules. G1 owns the common contract, registration, integration and the shared regression suite. Agree on that contract first. Then the other groups can implement language rules without editing the same files. A finding tagged with two or more groups needs each listed part before it is closed.

| Group | File set | Responsibility | Overlap with D1124-JSRELIMPORT |
| --- | --- | --- | --- |
| G1 | `crates/dctx/src/index/{refs.rs,refs_names.rs,refs_tests.rs,walk.rs,lex_dispatch.rs,mod.rs,imports/mod.rs}` and `crates/dctx-map-model/src/domain/{graph.rs,mod.rs}` | Inventory, shared structured facts, language dispatch, scoped bindings, permitted edge kinds and confidence, and release-walk regression coverage. Land shared resolver edits after reconciling the JS relative-import card. | **`refs.rs`, `refs_names.rs`, `refs_tests.rs`** |
| G2 | `crates/dctx-lex-js/src/extract/{modules.rs,calls.rs,functions.rs,statements.rs,classes.rs}`, `crates/dctx-lex-js/src/domain/facts.rs`, `crates/dctx-map-parse/src/adapters/{javascript_references.rs,javascript_items.rs}`, `crates/dctx/src/index/imports/javascript.rs` | Script module forms, exports, identifiers, literal decoding and loader evidence. Embedded web scripts consume the same rules. | None |
| G3 | `crates/dctx-lex-py/src/extract/{imports.rs,declarations.rs}`, `crates/dctx-lex-py/src/domain/facts.rs`, `crates/dctx/src/index/{export_list.rs,export_list_tests.rs,imports/python.rs}` | Package levels and roots, wildcard exports, lexical bindings and package-member precedence. G3 owns the shared outline scanner if it needs extension. | None |
| G4 | `crates/dctx-lex-rust/src/adapters/syntax_refs.rs`, `crates/dctx/src/index/{syn_extract.rs,imports/rust.rs}`, `crates/dctx-map-parse/src/adapters/{rust.rs,rust_references.rs}` | Crate/module tree, public-use projection and path overrides. | None |
| G5 | `crates/dctx-lex-{go,java,kt,cs,swift}/src/extract/declarations.rs`, `crates/dctx/src/index/imports/{go.rs,jvm.rs,namespaces.rs,swift.rs}` | Directory-owned Go packages and declaration-owned JVM, C#, Swift, C++ and PHP lookups. G6 supplies the C++ and PHP form tags. | None |
| G6 | `crates/dctx-lex-c/src/{lexer/preprocessor.rs,extract/declarations.rs}`, `crates/dctx-lex-{rb,php}/src/extract/references.rs`, `crates/dctx/src/index/imports/{headers.rs,ruby.rs,php.rs}` | Preserve delimiter/loader/namespace distinctions, search roots and literal file operands. | None |
| G7 | `crates/dctx/src/index/{extract.rs,extract/source.rs,generic/calls_infra.rs,generic/calls_block.rs,imports/config.rs,imports/documents.rs,imports/proto.rs,imports/lua.rs}` | Project existing config and Markdown facts. Add form-specific generic import extraction and tested abstention. | None |

Group-local producer and rule tests can live in the listed Rust modules. Centralize final edge expectations in G1 to avoid concurrent edits to the JS card’s seam tests.
