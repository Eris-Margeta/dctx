#!/usr/bin/env python3
"""Run the real dctx binary on the sample project and record every output the website shows.

    DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py            # rewrite data/demo.json
    DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py --out FILE # write elsewhere (check.py)
    DCTX_BIN=target/release/dctx-cli python3 tools/website/capture.py --llms     # rewrite data/llms.json

--llms runs the same binary over a copy of the documents behind llms-small.txt, llms.txt and
llms-full.txt (routes.llms_tiers) and records its exact output, the sha256 of every input document and
the last commit that touched them (git; --keep-git FILE reuses that commit from FILE, for shallow clones).

Every command runs in a fresh, empty environment: HOME, XDG_CONFIG_HOME and DCTX_CACHE_DIR are
new temporary directories, DCTX_AUTO_CLEAN=0, LC_ALL=C.UTF-8, and the working directory is a copy
of tools/website/sample/ named `kiosk`. Wall time is never stored. Three values are not
deterministic and are scrubbed, each in one documented place (SCRUBS below); nothing else is edited.
Python standard library only.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
SAMPLE = HERE / "sample"
SAMPLE_MTIME = 1767225600  # 2026-01-01T00:00:00Z, see capture()
OUT = HERE / "data" / "demo.json"
LLMS_OUT = HERE / "data" / "llms.json"
SENTINEL = "example-not-a-secret"  # the value of the sample's .env; it must never reach an output
VERBS = ["map", "find", "neighbors", "explain", "read", "skeleton", "grep", "hubs", "path", "export"]
GREET = "s:src/util.rs#greet"
MCP_IN = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "dctx-website", "version": "1"}}},
    {"jsonrpc": "2.0", "method": "notifications/initialized"},
    {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
    {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "find", "arguments": {"query": "greet"}}},
]

# (id, group, argv after `dctx`, note). Order is the order of the examples page.
COMMANDS: list[tuple[str, str, list[str], str]] = [
    ("version", "pack", ["--version"], ""),
    ("pack", "pack", ["pack", "."], "The whole project as one block. `.env` is listed by name and never read; `target/` (ignored by .gitignore) and the binary `assets/logo.png` are left out."),
    ("pack-stamps", "pack", ["pack", ".", "--tree-stamps=on"], "The 1.0.0 tree lines with MOD and SIZE. MOD is a modification time in milliseconds and differs on every checkout, so its digits are replaced with … here."),
    ("pack-ext-rs", "pack", ["pack", ".", "--ext", "rs"], "Only the Rust files."),
    ("pack-debug-ignored", "pack", ["pack", ".", "--debug-ignored"], "Ignored paths are listed with [IGNORED] and never read: the content of target/ does not appear."),
    ("pack-for", "pack", ["pack", ".", "--for", "greet store", "--budget", "800"], "Task context for an agent: find, neighbors and read of the names in the text, cut to the estimated output-token budget."),
    ("map-map", "map", ["map", "map", "src/util.rs", "--depth", "2"], "Outline of one file."),
    ("find", "map", ["map", "find", "greet"], ""),
    ("read", "map", ["map", "read", GREET], "Read the symbol metadata and verified source bytes; --json puts the source in source_utf8."),
    ("neighbors-in", "map", ["map", "neighbors", GREET, "--edge", "calls", "--dir", "in"], "Callers, with the call-site line."),
    ("explain", "map", ["map", "explain", GREET], ""),
    ("skeleton", "map", ["map", "skeleton", "src/store.rs"], "Declaration signatures, no bodies, in source order."),
    ("grep", "map", ["map", "grep", "example", "-i"], "Exhaustive literal search. The `.env` value contains `example` too; it is never read, so it is not a hit."),
    ("hubs", "map", ["map", "hubs", "--limit", "10"], ""),
    ("path", "map", ["map", "path", "s:src/main.rs#main", "s:src/store.rs#save"], ""),
    ("find-json", "json", ["map", "find", "greet", "--json"], ""),
    ("neighbors-in-json", "json", ["map", "neighbors", GREET, "--edge", "calls", "--dir", "in", "--json"], ""),
    ("read-json", "json", ["map", "read", GREET, "--json"], "The verified source bytes of one node."),
    ("find-json-meta", "json", ["map", "find", "greet", "--json", "--meta"], "With --meta the answer also carries the snapshot digest, the generation and the index age. The age changes every second, which is why the default leaves these fields out; its digits are replaced with … here."),
    ("export-mermaid", "graph", ["map", "export", "--format", "mermaid"], "Paste into any Mermaid renderer."),
    ("export-json", "graph", ["map", "export", "--format", "json"], ""),
    ("mcp", "mcp", ["mcp", "."], "Four JSON-RPC lines on stdin: initialize, notifications/initialized, tools/list and a tools/call of find."),
    ("error-gone", "errors", ["map", "read", "m:nope.rs"], "An id that is not in the index: the `gone` page on stdout, exit 1, two lines on stderr."),
    ("error-env", "errors", ["map", "read", "m:.env"], "A sensitive file is not in the index at all."),
    ("error-usage", "errors", ["map", "find", ""], "An empty query is a usage error, exit 2."),
    ("languages-md", "reference", ["languages", "--markdown"], ""),
    ("help", "reference", ["--help"], ""),
    ("map-help", "reference", ["map", "--help"], ""),
    *[(f"help-{verb}", "reference", ["map", verb, "--help"], "") for verb in VERBS],
    ("languages-help", "reference", ["languages", "--help"], ""),
    ("template-default", "templates", ["template", "build", "default.dctx_template"], "Compiles the built-in default template; prints the plan size and digest. Nothing is written without --output."),
    ("template-stamped", "templates", ["template", "build", "stamped.dctx_template"], ""),
]

SCRUBS = {
    "version": (r"\((?:[0-9a-f]{7,40}|unknown)\)", "(<git sha>)"),  # the commit the binary was built from
    "pack-stamps": (r"MOD:\d+", "MOD:…"),                              # file modification times
    "find-json-meta": (r'"age_s":\d+', '"age_s":…'),                   # seconds since the index was built
}


def est_tokens(nbytes: int) -> int:
    return math.ceil(2 * nbytes / 7)


def run(binary: str, argv: list[str], cwd: Path, env: dict, stdin: str | None = None) -> tuple[str, str, int]:
    proc = subprocess.run([binary, *argv], cwd=cwd, env=env, input=(stdin or "").encode(),
                          capture_output=True, timeout=60)
    return proc.stdout.decode("utf-8"), proc.stderr.decode("utf-8"), proc.returncode


def tree_paths(pack_stdout: str) -> list[str]:
    return re.findall(r"^\s*\[F\] (\S+) \(ID:", pack_stdout, re.M)


def capture(binary: str, version: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="dctx-capture-") as tmp:
        tmp_path = Path(tmp)
        for name in ("home", "config", "cache"):
            (tmp_path / name).mkdir()
        place = tmp_path / "kiosk"
        shutil.copytree(SAMPLE, place)
        # The snapshot digest (find --meta) covers modification times; a checkout or an archive sets
        # them to its own clock, so every copied path gets one fixed time (2026-01-01T00:00:00Z).
        for path in [*place.rglob("*"), place]:
            os.utime(path, (SAMPLE_MTIME, SAMPLE_MTIME), follow_symlinks=False)
        env = {"PATH": "/usr/bin:/bin", "HOME": str(tmp_path / "home"), "XDG_CONFIG_HOME": str(tmp_path / "config"),
               "DCTX_CACHE_DIR": str(tmp_path / "cache"), "DCTX_AUTO_CLEAN": "0", "LC_ALL": "C.UTF-8", "COLUMNS": "100"}
        stdout, _, code = run(binary, ["--version"], place, env)
        if code or not stdout.startswith(f"dctx {version} "):
            raise SystemExit(f"capture: {binary} --version printed {stdout.strip()!r}; expected 'dctx {version} (…)'")
        templates = tmp_path / "templates"
        shutil.copytree(ROOT / "crates/dctx-template/templates", templates)
        records = []
        for rid, group, argv, note in COMMANDS:
            cwd = templates if group == "templates" else place
            stdin = "".join(json.dumps(m, separators=(",", ":")) + "\n" for m in MCP_IN) if rid == "mcp" else None
            out, err, code = run(binary, argv, cwd, env, stdin)
            if rid in SCRUBS:
                pattern, repl = SCRUBS[rid]
                out, n = re.subn(pattern, repl, out)
                if not n:
                    raise SystemExit(f"capture: scrub for {rid} matched nothing; the output format changed")
            for text in (out, err):
                if SENTINEL in text:
                    raise SystemExit(f"capture: {rid} printed the .env value; the privacy claim is false")
                if "kiosk build output" in text:
                    raise SystemExit(f"capture: {rid} printed the content of an ignored file")
                if tmp in text:
                    raise SystemExit(f"capture: {rid} printed a temporary path")
            nbytes = len(out.encode("utf-8"))
            record = {"id": rid, "group": group, "argv": ["dctx", *argv], "stdout": out, "stderr": err, "exit": code,
                      "bytes": nbytes, "est_tokens": est_tokens(nbytes)}
            if note:
                record["note"] = note
            if rid in SCRUBS:
                record["scrubbed"] = SCRUBS[rid][1]
            if stdin:
                record["stdin"] = stdin
            records.append(record)
        # File status for the hero scene, derived from two real runs, not asserted by hand.
        by_id = {r["id"]: r for r in records}
        listed = set(tree_paths(by_id["pack"]["stdout"]))
        with_binary, _, _ = run(binary, ["pack", ".", "--binary"], place, env)
        listed_binary = set(tree_paths(with_binary))
        omitted = set(re.findall(r'PATH="([^"]+)" LINES="0">\n\[Content omitted', by_id["pack"]["stdout"]))
        files = []
        for path in sorted(p.relative_to(SAMPLE).as_posix() for p in SAMPLE.rglob("*") if p.is_file()):
            if path in omitted:
                status = "listed-not-read"
            elif path in listed:
                status = "packed"
            elif path in listed_binary:
                status = "binary"
            else:
                status = "ignored"
            files.append({"path": path, "bytes": (SAMPLE / path).stat().st_size, "status": status})
        mcp_lines = [json.loads(line) for line in by_id["mcp"]["stdout"].splitlines() if line.strip()]
        tools = next(m["result"]["tools"] for m in mcp_lines if m.get("id") == 2)
        footer = re.search(r"^# end body_tokens=(\d+) budget=(\d+) dropped=(\d+)$", by_id["pack-for"]["stdout"], re.M)
        pack_line = re.search(r"pack bytes=(\d+) files=(\d+)", by_id["pack"]["stdout"])
        return {
            "schema": 1,
            "version": version,
            "env": {k: v for k, v in env.items() if k in ("DCTX_AUTO_CLEAN", "LC_ALL", "COLUMNS")},
            "place": "kiosk",
            "sample": {"files": files},
            "summary": {
                "files_on_disk": len(files),
                "files_listed": len(listed),
                "files_read": len(listed) - len(omitted),
                "pack_bytes": int(pack_line.group(1)), "pack_files": int(pack_line.group(2)),
                "for_body_tokens": int(footer.group(1)), "for_budget": int(footer.group(2)), "for_dropped": int(footer.group(3)),
                "mcp_tools": [t["name"] for t in tools],
            },
            "records": records,
        }


def _git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True, text=True).stdout.strip()


def docs_commit(files: list[str]) -> dict:
    """The last commit that touched any of the documents, and its commit date. Never the clock."""
    sha, date = _git("log", "-1", "--format=%H %cI", "--", *files).split()
    return {"docs_commit": sha, "docs_date": date}


def _pack_page(stdout: str, files: list[str]) -> str:
    """The pack page without the trailing `pack bytes=N files=M ...` status line."""
    m = re.search(r"pack bytes=(\d+) files=(\d+) [^\n]*\n?\Z", stdout)
    if not m:
        raise SystemExit("capture --llms: no `pack bytes=` status line; the pack output format changed")
    if int(m.group(2)) != len(files):
        raise SystemExit(f"capture --llms: pack read {m.group(2)} files, expected {len(files)}: {files}")
    page = stdout.encode("utf-8")[:int(m.group(1))].decode("utf-8")
    if not page.startswith("[DCTX_LLM_CONTEXT_SNAPSHOT_V1]") or not stdout[len(page):].startswith("pack bytes="):
        raise SystemExit("capture --llms: pack page and status line do not split cleanly")
    return page.rstrip("\n") + "\n"


def pack_body(page: str, rel: str) -> str | None:
    """The content of one file inside a pack page: the LINES="N" lines after its start marker (the end
    marker alone is ambiguous when a document quotes a pack)."""
    m = re.search(r'^<FILE_CONTENT_START ID="[^"]+" PATH="' + re.escape(rel) + r'"[^>]* LINES="(\d+)">\n', page, re.M)
    if not m:
        return None
    lines = page[m.end():].split("\n")[:int(m.group(1))]
    return "\n".join(lines).rstrip("\n")


def capture_llms(binary: str, version: str, keep_git: dict | None = None) -> dict:
    from routes import llms_tiers
    tiers = llms_tiers()
    corpus = sorted({f for *_, files in tiers for f in files})
    sources = {}
    with tempfile.TemporaryDirectory(prefix="dctx-llms-") as tmp:
        tmp_path = Path(tmp)
        for name in ("home", "config", "cache"):
            (tmp_path / name).mkdir()
        place = tmp_path / "dctx"
        for rel in corpus:
            data = (ROOT / rel).read_bytes()
            sources[rel] = hashlib.sha256(data).hexdigest()
            (place / rel).parent.mkdir(parents=True, exist_ok=True)
            (place / rel).write_bytes(data)
        for path in [*place.rglob("*"), place]:
            os.utime(path, (SAMPLE_MTIME, SAMPLE_MTIME), follow_symlinks=False)
        env = {"PATH": "/usr/bin:/bin", "HOME": str(tmp_path / "home"), "XDG_CONFIG_HOME": str(tmp_path / "config"),
               "DCTX_CACHE_DIR": str(tmp_path / "cache"), "DCTX_AUTO_CLEAN": "0", "LC_ALL": "C.UTF-8", "COLUMNS": "100"}
        stdout, _, code = run(binary, ["--version"], place, env)
        if code or not stdout.startswith(f"dctx {version} "):
            raise SystemExit(f"capture --llms: {binary} --version printed {stdout.strip()!r}; expected 'dctx {version} (…)'")

        def render(mode: str, files: list[str]) -> tuple[str, list[list[str]]]:
            if mode == "pack":
                argv = ["pack", "."] + [x for f in files for x in ("--include", "/" + f)]
                out, err, code = run(binary, argv, place, env)
                if code:
                    raise SystemExit(f"capture --llms: dctx {' '.join(argv)} exited {code}: {err}")
                return _pack_page(out, files), [["dctx", *argv]]
            parts, argvs = [], []
            for f in files:
                argv = ["map", "map", f, "--depth", "2"]
                out, err, code = run(binary, argv, place, env)
                if code:
                    raise SystemExit(f"capture --llms: dctx {' '.join(argv)} exited {code}: {err}")
                parts.append(out.rstrip("\n") + "\n")
                argvs.append(["dctx", *argv])
            return "".join(parts), argvs

        records = []
        for name, mode, budget, files in tiers:
            chosen, dropped = [], []
            for f in files:
                text, _ = render(mode, chosen + [f])
                if est_tokens(len(text.encode("utf-8"))) <= budget:
                    chosen.append(f)
                else:
                    dropped.append(f)
            text, argvs = render(mode, chosen)
            if tmp in text or SENTINEL in text:
                raise SystemExit(f"capture --llms: {name} output contains a temporary path or the sample secret")
            nbytes = len(text.encode("utf-8"))
            record = {"file": name, "mode": mode, "budget": budget, "files": chosen, "dropped": dropped,
                      "commands": argvs, "bytes": nbytes, "est_tokens": est_tokens(nbytes)}
            if mode == "pack":
                record["altered"] = [f for f in chosen if pack_body(text, f) != (ROOT / f).read_text(encoding="utf-8").rstrip("\n")]
            records.append({**record, "output": text})
    git = {k: keep_git[k] for k in ("docs_commit", "docs_date")} if keep_git else docs_commit(corpus)
    return {"schema": 1, "version": version, **git, "sources": sources, "tiers": records}


def main(argv: list[str]) -> int:
    out = OUT
    if "--out" in argv:
        out = Path(argv[argv.index("--out") + 1])
    binary = os.environ.get("DCTX_BIN") or str(ROOT / "target/release/dctx-cli")
    if not Path(binary).is_file():
        print(f"capture: no dctx binary at {binary}; build it (cargo build --locked --release -p dctx-cli) or set DCTX_BIN",
              file=sys.stderr)
        return 2
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    if "--llms" in argv:
        if "--out" not in argv:
            out = LLMS_OUT
        keep = json.loads(Path(argv[argv.index("--keep-git") + 1]).read_text()) if "--keep-git" in argv else None
        data = capture_llms(str(Path(binary).resolve()), version, keep)
        summary = ", ".join(f"{t['file']} {len(t['files'])} files {t['est_tokens']}/{t['budget']}" for t in data["tiers"])
    else:
        data = capture(str(Path(binary).resolve()), version)
        summary = f"{len(data['records'])} records"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(data, ensure_ascii=False, indent=1, sort_keys=False) + "\n", encoding="utf-8")
    print(f"capture: {summary} -> {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
