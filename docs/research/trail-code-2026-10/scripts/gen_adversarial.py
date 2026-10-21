#!/usr/bin/env python3
"""Generate the synthetic adversarial corpus.

usage: gen_adversarial.py OUT_DIR OUTSIDE_DIR [--no-git]

Every planted item carries a unique marker so leaks can be detected by a plain
byte search over tool outputs, indexes and caches. Deterministic: no randomness
except a fixed-seed PRNG for the binary blob.
"""
import os
import random
import subprocess
import sys

SECRET = "sk-test-DCTXBENCH0000fake0000secret0000value"
MARKERS = {
    "env_secret": SECRET,
    "env_name": "DCTXBENCH_FAKE_SECRET",
    "gitignored_marker": "DCTXBENCH_IGNORED_MARKER_7f3a",
    "gitignored_fn": "dctxbench_ignored_fn",
    "secret_py_value": "sk-test-DCTXBENCHsecretpy",
    "secret_py_fn": "dctxbench_secret_fn",
    "node_modules_fn": "dctxbench_node_modules_fn",
    "binary_marker": "DCTXBENCH_BINARY_MARKER",
    "binary_py_fn": "dctxbench_binary_fn",
    "big_tail_marker": "DCTXBENCH_BIG_TAIL_MARKER",
    "minified_fn": "dctxbench_min_fn",
    "outside_marker": "DCTXBENCH_OUTSIDE_MARKER",
    "outside_fn": "dctxbench_outside_fn",
    "nonutf8_fn": "dctxbench_nonutf8_fn",
    "deep_fn": "dctxbench_deep_fn",
    "crlf_fn": "dctxbench_crlf_fn",
    "space_name_fn": "dctxbench_space_fn",
    "newline_name_fn": "dctxbench_newline_fn",
    "longline_fn": "dctxbench_longline_fn",
    "huge_py_last_fn": "dctxbench_huge_f49999",
}
UNSUPPORTED = {
    "zig": "pub fn dctxbench_zig_fn() void {}\n",
    "ex": "defmodule M do\n  def dctxbench_ex_fn, do: 1\nend\n",
    "hs": "dctxbench_hs_fn :: Int\ndctxbench_hs_fn = 1\n",
    "lua": "function dctxbench_lua_fn() return 1 end\n",
    "scala": "object O { def dctxbench_scala_fn(): Int = 1 }\n",
    "dart": "int dctxbench_dart_fn() => 1;\n",
    "ml": "let dctxbench_ml_fn () = 1\n",
    "erl": "-module(e).\ndctxbench_erl_fn() -> 1.\n",
    "jl": "function dctxbench_jl_fn() 1 end\n",
    "sol": "contract C { function dctxbench_sol_fn() public {} }\n",
    "pl": "sub dctxbench_pl_fn { return 1; }\n",
    "r": "dctxbench_r_fn <- function() 1\n",
    "vue": "<script>\nexport function dctxbench_vue_fn() { return 1 }\n</script>\n",
}


def w(path, data, mode="w"):
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    if isinstance(data, str):
        data = data.encode("utf-8")
    with open(path, "wb") as f:
        f.write(data)


def main():
    out = os.path.abspath(sys.argv[1])
    outside = os.path.abspath(sys.argv[2])
    use_git = "--no-git" not in sys.argv
    os.makedirs(out, exist_ok=False)
    os.makedirs(outside, exist_ok=True)
    j = lambda *p: os.path.join(out, *p)

    # Baseline, well-formed sources with a small call graph.
    w(j("src", "main.py"),
      "from src.util import helper\n\n\n"
      "def dctxbench_lf_target():\n    return helper(1)\n\n\n"
      "def dctxbench_main():\n    return dctxbench_lf_target() + helper(2)\n")
    w(j("src", "util.py"), "def helper(x):\n    return x + 1\n")
    w(j("src", "lib.rs"), "pub fn dctxbench_rs_fn() -> u32 { dctxbench_rs_callee() }\n"
      "fn dctxbench_rs_callee() -> u32 { 1 }\n")
    w(j("src", "app.ts"), "export function dctxbench_ts_fn(): number { return dctxbench_ts_callee(); }\n"
      "function dctxbench_ts_callee(): number { return 1; }\n")

    # Secrets and ignore rules.
    w(j(".env"), f"{MARKERS['env_name']}={SECRET}\nDATABASE_URL=postgres://u:p@h/db\n")
    w(j(".gitignore"), "ignored_dir/\n*.secret.py\n")
    w(j("ignored_dir", "hidden.py"),
      f"def {MARKERS['gitignored_fn']}():\n    return \"{MARKERS['gitignored_marker']}\"\n")
    w(j("ignored_dir", "notes.txt"), MARKERS["gitignored_marker"] + "\n")
    w(j("app.secret.py"),
      f"{MARKERS['secret_py_fn'].upper()} = \"{MARKERS['secret_py_value']}\"\n\n"
      f"def {MARKERS['secret_py_fn']}():\n    return \"{MARKERS['secret_py_value']}\"\n")

    # Vendored dependency directory (not gitignored on purpose).
    w(j("node_modules", "leftpad", "index.js"),
      f"function {MARKERS['node_modules_fn']}(s) {{ return s; }}\nmodule.exports = {MARKERS['node_modules_fn']};\n")

    # Binary content.
    rng = random.Random(1234)
    blob = bytes(rng.getrandbits(8) for _ in range(256 * 1024))
    w(j("assets", "blob.bin"), blob[:1000] + MARKERS["binary_marker"].encode() + blob[1000:])
    w(j("src", "binary_disguised.py"),
      b"\x00\x01\x02\xff\xfe" + f"\ndef {MARKERS['binary_py_fn']}():\n    return 1\n".encode() + blob[:4096])

    # 50 MB text file with a marker at the end.
    line = ("lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor " * 2).strip() + "\n"
    n = (50 * 1024 * 1024) // len(line)
    os.makedirs(j("data"), exist_ok=True)
    with open(j("data", "big.txt"), "w") as f:
        for _ in range(n):
            f.write(line)
        f.write(MARKERS["big_tail_marker"] + "\n")

    # 5 MB single-line minified JS.
    parts = []
    size = 0
    i = 0
    while size < 5 * 1024 * 1024:
        p = f"function a{i}(b){{return b+{i}}};"
        parts.append(p)
        size += len(p)
        i += 1
    parts.append(f"function {MARKERS['minified_fn']}(x){{return a0(x)}};")
    w(j("static", "app.min.js"), "".join(parts))

    # 5 MB generated Python module with a long call chain.
    with open(j("src", "huge_gen.py"), "w") as f:
        f.write("def dctxbench_huge_f0():\n    return 0\n\n")
        for k in range(1, 50000):
            f.write(f"def dctxbench_huge_f{k}():\n    return dctxbench_huge_f{k-1}()\n\n")

    # One 100 KB line inside a Python file.
    w(j("src", "longline.py"),
      "X = '" + ("y" * 100_000) + "'\n\n" + f"def {MARKERS['longline_fn']}():\n    return X\n")

    # Symlink loop plus a symlink escaping the repository.
    os.makedirs(j("loop"), exist_ok=True)
    os.symlink("..", j("loop", "up"))
    os.symlink(".", j("loop", "self"))
    w(os.path.join(outside, "outside.py"),
      f"def {MARKERS['outside_fn']}():\n    return \"{MARKERS['outside_marker']}\"\n")
    w(os.path.join(outside, "marker.txt"), MARKERS["outside_marker"] + "\n")
    os.symlink(outside, j("outside_link"))
    os.symlink(os.path.join(outside, "outside.py"), j("src", "linked_outside.py"))

    # Non-UTF-8 filename, space and newline in names.
    w(os.path.join(out.encode(), b"src", b"caf\xe9.py"),
      f"def {MARKERS['nonutf8_fn']}():\n    return 1\n")
    w(j("src", "we ird name.py"), f"def {MARKERS['space_name_fn']}():\n    return 1\n")
    w(j("src", "new\nline.py"), f"def {MARKERS['newline_name_fn']}():\n    return 1\n")

    # 200-level deep nesting.
    deep = j(*(["deep"] + ["d"] * 200))
    w(os.path.join(deep, "leaf.py"), f"def {MARKERS['deep_fn']}():\n    return 1\n")

    # CRLF line endings.
    w(j("src", "crlf.py"), (f"def {MARKERS['crlf_fn']}():\r\n    return dctxbench_lf_target()\r\n"))

    # Empty, BOM-only and unsupported-language files.
    w(j("src", "empty.py"), "")
    w(j("src", "bom.py"), b"\xef\xbb\xbf")
    for ext, body in UNSUPPORTED.items():
        w(j("src", "unsupported", f"sample.{ext}"), body)

    if use_git:
        env = dict(os.environ, GIT_AUTHOR_NAME="bench", GIT_AUTHOR_EMAIL="bench@example.invalid",
                   GIT_COMMITTER_NAME="bench", GIT_COMMITTER_EMAIL="bench@example.invalid",
                   GIT_AUTHOR_DATE="2026-01-01T00:00:00Z", GIT_COMMITTER_DATE="2026-01-01T00:00:00Z")
        subprocess.run(["git", "init", "-q", "-b", "main", out], check=True, env=env)
        subprocess.run(["git", "-C", out, "add", "-A"], check=True, env=env)
        subprocess.run(["git", "-C", out, "commit", "-q", "-m", "adversarial corpus"], check=True, env=env)

    for k, v in MARKERS.items():
        print(f"{k}\t{v}")


if __name__ == "__main__":
    main()
