#!/usr/bin/env python3
"""Callers / callees / outline gold from language servers, not from any benchmarked tool (v1 card 6).

usage: gold_lsp.py SELECTION_JSON CORPORA_DIR GOLD_DIR [--n 20]
Servers (pinned in repos.json, installed by setup.sh under $B3_ROOT/lsp): rust-analyzer, pyright-langserver, gopls.
Targets: functions and methods from textDocument/documentSymbol over the repo's non-test source files, in a seeded
order (sha256 of "<seed>:<repo>"), kept when callHierarchy/incomingCalls returns at least one call site. Gold call
sites are the incomingCalls fromRanges; depth-2 callers and callees come from the same call hierarchy. Each server
runs without network (unshare -n when available, CARGO_NET_OFFLINE, GOPROXY=off).
"""
import argparse
import hashlib
import json
import os
import random
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import lsp  # noqa: E402

KINDS = {5: "class", 6: "method", 9: "constructor", 10: "enum", 11: "interface", 12: "function", 23: "struct"}
CALLABLE = (6, 12)
TESTY = re.compile(r"(^|/)(tests?|testdata|testing|fixtures?|benches|examples?|docs?|vendor|third_party)(/|$)|_test\.go$|"
                   r"(^|/)test_[^/]*\.py$|_tests?\.py$|(^|/)conftest\.py$")
LANG = {"rust": {"exts": (".rs",), "id": "rust"}, "python": {"exts": (".py",), "id": "python"}, "go": {"exts": (".go",), "id": "go"}}
MAX_FILES = 600


def flatten_symbols(result, container=None):
    """DocumentSymbol trees or SymbolInformation lists as flat dicts (lines 1-based, selection position 0-based)."""
    out = []
    for s in result or []:
        if "location" in s:
            r = s["location"]["range"]
            sel, cont = r["start"], s.get("containerName") or None
            out.append({"name": s["name"], "kind": s["kind"], "line": r["start"]["line"] + 1, "end": r["end"]["line"] + 1,
                        "sel_line": sel["line"], "sel_char": sel["character"], "container": cont, "top": cont is None})
            continue
        r, sel = s["range"], s.get("selectionRange", s["range"])["start"]
        out.append({"name": s["name"], "kind": s["kind"], "line": r["start"]["line"] + 1, "end": r["end"]["line"] + 1,
                    "sel_line": sel["line"], "sel_char": sel["character"], "container": container, "top": container is None})
        out.extend(flatten_symbols(s.get("children"), s["name"]))
    return out


def eligible(sym, rel):
    """A callable, non-test, non-trivial name in a non-test file."""
    n = sym["name"]
    return (sym["kind"] in CALLABLE and len(n) >= 4 and n != "main" and not n.lower().startswith("test")
            and not n.startswith("__") and not TESTY.search(rel))


def rel_of(uri, root):
    """Repo-relative path of a URI, or None when it is outside the repo."""
    p = os.path.realpath(lsp.uri_path(uri))
    r = os.path.realpath(root)
    return os.path.relpath(p, r) if p.startswith(r + os.sep) else None


def ask(client, method, params, tries=4):
    """A request with retries: servers answer ContentModified (-32801) while they still index."""
    for i in range(tries):
        try:
            return client.request(method, params)
        except RuntimeError as e:
            if i == tries - 1 or "exited" in str(e):
                raise
            time.sleep(2 * (i + 1))
    return None


def file_symbols(client, root, rel, lang_id):
    uri = client.open(rel, lang_id)
    try:
        return flatten_symbols(ask(client, "textDocument/documentSymbol", {"textDocument": {"uri": uri}}))
    finally:
        client.close(rel)


def prepare(client, rel, root, line0, char0):
    """The call-hierarchy item at a 0-based position, or None."""
    uri = lsp.path_uri(os.path.join(root, rel))
    res = ask(client, "textDocument/prepareCallHierarchy", {"textDocument": {"uri": uri}, "position": {"line": line0, "character": char0}})
    return res[0] if res else None


def incoming_sites(client, item, root):
    """Call sites of `item` inside the repo: file, 1-based line, caller name and caller start line."""
    seen, out = set(), []
    for c in ask(client, "callHierarchy/incomingCalls", {"item": item}) or []:
        f = rel_of(c["from"]["uri"], root)
        if f is None:
            continue
        for r in c.get("fromRanges") or []:
            k = (f, r["start"]["line"] + 1)
            if k not in seen:
                seen.add(k)
                out.append({"file": f, "line": k[1], "caller": c["from"]["name"], "caller_line": c["from"]["range"]["start"]["line"] + 1,
                            "_item": c["from"]})
    out.sort(key=lambda s: (s["file"], s["line"]))
    return out


def callees_of(client, item, root):
    """Functions `item` calls that are defined inside the repo."""
    seen, out = set(), []
    for c in ask(client, "callHierarchy/outgoingCalls", {"item": item}) or []:
        f = rel_of(c["to"]["uri"], root)
        if f is None:
            continue
        e = {"name": c["to"]["name"], "file": f, "line": c["to"]["selectionRange"]["start"]["line"] + 1}
        if (e["name"], f, e["line"]) not in seen:
            seen.add((e["name"], f, e["line"]))
            out.append(e)
    return sorted(out, key=lambda e: (e["file"], e["line"], e["name"]))


def strip(sites):
    return [{k: v for k, v in s.items() if k != "_item"} for s in sites]


def collect(client, root, files, lang_id, seed, n=20, outlines=None, name=None, max_tries=400):
    """Up to n targets with at least one gold call site; fills `outlines` (file -> top-level names) when given."""
    cands = []
    for rel in files:
        try:
            syms = file_symbols(client, root, rel, lang_id)
        except RuntimeError as e:          # one file the server rejects is skipped (and logged), a dead server is fatal
            if "exited" in str(e):
                raise
            print(f"gold: skip {rel}: {e}", file=sys.stderr, flush=True)
            continue
        if outlines is not None:
            outlines[rel] = [s["name"] for s in syms if s["top"]]
        cands += [(rel, s) for s in syms if eligible(s, rel)]
    rng = random.Random(int(hashlib.sha256(f"{seed}:{name or os.path.basename(root)}".encode()).hexdigest(), 16))
    rng.shuffle(cands)
    targets, names = [], set()
    for rel, s in cands[:max_tries]:
        if len(targets) >= n:
            break
        if (s["name"], rel) in names:
            continue
        try:
            t = target_of(client, root, rel, s)
        except RuntimeError as e:
            if "exited" in str(e):
                raise
            print(f"gold: skip {rel}#{s['name']}: {e}", file=sys.stderr, flush=True)
            continue
        if t:
            names.add((s["name"], rel))
            targets.append(t)
    return targets


def target_of(client, root, rel, s):
    """Gold for one candidate symbol, or None when the server finds no call site of it."""
    item = prepare(client, rel, root, s["sel_line"], s["sel_char"])
    if not item or s["name"] not in re.findall(r"\w+", item.get("name", "")):   # e.g. gopls "(*T).Method"
        return None
    sites = incoming_sites(client, item, root)
    if not sites:
        return None
    callers2, seen = [], set()
    for it in [x["_item"] for x in sites][:15]:
        key = (it["uri"], it["range"]["start"]["line"])
        if key not in seen:
            seen.add(key)
            callers2 += incoming_sites(client, it, root)
    return {"def": {"name": s["name"], "file": rel, "line": s["line"], "end": s["end"], "kind": KINDS[s["kind"]], "container": s["container"]},
            "callers": strip(sites), "callers2": strip(callers2)[:60], "callees": callees_of(client, item, root),
            "source": "lsp:callHierarchy/incomingCalls"}


def source_files(root, lang, seed, name):
    p = subprocess.run(["git", "-C", root, "ls-files", "-z"], capture_output=True, check=True)
    fs = sorted(f for f in p.stdout.decode("utf-8", "replace").split("\0") if f.endswith(LANG[lang]["exts"]) and not TESTY.search(f))
    if len(fs) > MAX_FILES:
        rng = random.Random(int(hashlib.sha256(f"{seed}:{name}:files".encode()).hexdigest(), 16))
        fs = sorted(rng.sample(fs, MAX_FILES))
    return fs


def server(lang, base):
    """argv, env, settings and initializationOptions for one pinned server under base/lsp."""
    L = os.path.join(base, "lsp")
    # HOME stays the runner's: cargo/rustup homes and the go build cache hold what setup.sh fetched.
    env = dict(os.environ, CARGO_NET_OFFLINE="true", GOPROXY="off", GOFLAGS="-mod=mod",
               GOTOOLCHAIN="local", GOPATH=os.path.join(base, "gopath"), GOMODCACHE=os.path.join(base, "gopath", "pkg", "mod"),
               PATH=os.pathsep.join([os.path.join(L, "bin"), os.path.join(L, "node_modules", ".bin"), os.environ.get("PATH", "")]))
    if lang == "rust":
        return [os.path.join(L, "bin", "rust-analyzer")], env, {}, {"cargo": {"buildScripts": {"enable": False}}, "procMacro": {"enable": False},
                                                                   "checkOnSave": False, "cachePriming": {"enable": True}}
    if lang == "python":
        return ([os.path.join(L, "node_modules", ".bin", "pyright-langserver"), "--stdio"], env,
                {"python.analysis": {"diagnosticMode": "openFilesOnly", "typeCheckingMode": "off"}, "*": {}}, None)
    return [os.path.join(L, "bin", "gopls")], env, {}, None


def no_net(argv):
    try:
        ok = subprocess.run(["unshare", "-Urn", "true"], capture_output=True, timeout=20).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        ok = False
    return (["unshare", "-Urn"] + argv, True) if ok else (argv, False)


def version_of(argv0, lang):
    flag = {"rust": ["--version"], "python": ["--version"], "go": ["version"]}[lang]
    exe = argv0.replace("pyright-langserver", "pyright")
    try:
        return subprocess.run([exe] + flag, capture_output=True, text=True, timeout=60).stdout.strip()
    except OSError as e:
        return f"unknown ({e})"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("corpora")
    ap.add_argument("gold")
    ap.add_argument("--n", type=int, default=20)
    ap.add_argument("--base", default=os.environ.get("B3_ROOT", "/path/to/workdir/bench/dctx3"))
    a = ap.parse_args()
    sel = json.load(open(a.selection))
    os.makedirs(a.gold, exist_ok=True)
    for s in sel["selection"]:
        root = os.path.join(a.corpora, s["name"])
        argv, env, settings, init = server(s["lang"], a.base)
        version = version_of(argv[0], s["lang"])
        argv, isolated = no_net(argv)
        files = source_files(root, s["lang"], sel["seed"], s["name"])
        t0 = time.monotonic()
        with open(os.path.join(a.gold, f"{s['name']}.server.log"), "wb") as log:
            c = lsp.Client(argv, root, env=env, settings=settings, init_options=init, log=log)
            info = c.initialize()
            ready = c.wait_ready(quiet=10, timeout=1800)
            outlines = {}
            targets = collect(c, root, files, LANG[s["lang"]]["id"], sel["seed"], n=a.n, outlines=outlines, name=s["name"])
            c.shutdown()
        doc = {"repo": s["name"], "lang": s["lang"], "tag": s["tag"], "sha": s["sha"], "seed": sel["seed"],
               "server": {"argv": argv, "version": version, "info": info.get("serverInfo"), "network_isolated": isolated, "ready": ready},
               "files_scanned": len(files), "wall_s": round(time.monotonic() - t0, 1), "targets": targets, "outlines": outlines}
        with open(os.path.join(a.gold, f"{s['name']}.json"), "w") as f:
            json.dump(doc, f, indent=1)
        print(f"gold {s['name']}: {len(files)} files, {len(targets)} targets, {sum(len(t['callers']) for t in targets)} call sites, "
              f"server ready={ready}", flush=True)
        if len(targets) < a.n:
            print(f"gold {s['name']}: WARNING only {len(targets)} of {a.n} targets", flush=True)


if __name__ == "__main__":
    main()
