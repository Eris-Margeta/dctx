#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Linux syscall proof that a warm targeted read stays within its I/O contract."""
import argparse
import collections
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from parity import ROOT, Context, isolated_env

# Explicit contract: scoped read uses an ephemeral session. A warm read opens
# zero persistent cache files, including ledger, locks, policy and objects.
EXPECTED_CACHE_OPENS = frozenset()
# CLI line-range preparation, snapshot verification and page emission each
# currently open the selected file once. This bound is independent of tree size.
MAX_TARGET_OPENS = 3


def open_paths(trace, cwd):
    """Resolve successful AND failed opens, including relative openat dirfds.

    strace -yy annotates directory descriptors; -s 4096 avoids path truncation.
    Unknown/truncated open syntax fails closed rather than silently losing I/O.
    """
    for line in trace.splitlines():
        match = re.search(r"\b(open|openat|openat2)\((.*)", line)
        if not match:
            continue
        kind, args = match.groups()
        base = cwd
        if kind != "open":
            fd, args = args.split(", ", 1)
            if fd != "AT_FDCWD":
                annotated = re.fullmatch(r"(?:\d+|AT_FDCWD)<(/[^>]*)>", fd)
                if annotated is None:
                    raise AssertionError(f"unresolved open dirfd: {line}")
                base = Path(annotated[1])
        quoted = re.match(r'"((?:\\.|[^"\\])*)"', args)
        if quoted is None:
            raise AssertionError(f"unparsed open path: {line}")
        path = Path(json.loads(quoted.group()))
        yield Path(os.path.normpath(path if path.is_absolute() else base / path)), line


def check_trace(trace, repo, cache_roots):
    opens = collections.Counter()
    source_opens = collections.Counter()
    errors = []
    for path, line in open_paths(trace, repo):
        for cache in cache_roots:
            if path.is_relative_to(cache):
                relative = path.relative_to(cache).as_posix()
                if relative not in EXPECTED_CACHE_OPENS:
                    errors.append(f"unexpected cache open: {relative}")
                opens[relative] += 1
        if path.is_relative_to(repo) and path.suffix == ".rs":
            relative = path.relative_to(repo).as_posix()
            source_opens[relative] += 1
            if relative != "src/selected.rs":
                errors.append(f"unrelated source opened: {relative}")
    if not source_opens["src/selected.rs"]:
        errors.append("no target source opens observed; trace did not exercise targeted read")
    if source_opens["src/selected.rs"] > MAX_TARGET_OPENS:
        errors.append(f"selected source opened more than {MAX_TARGET_OPENS} times")
    # A target file may be verified and/or emitted, but unrelated source bytes
    # must never be read, even via inherited descriptors or memory mapping.
    for line in trace.splitlines():
        if re.search(r"\bgetdents64\(", line):
            for annotated in re.finditer(r"\d+<(/[^>]*)>", line):
                path = Path(annotated[1])
                if path.is_relative_to(repo) and path not in {repo, repo / "src"}:
                    errors.append(f"unrelated directory enumerated: {path.relative_to(repo)}")
        if re.search(r"\b(?:read|pread64|mmap)\(", line):
            for annotated in re.finditer(r"\d+<(/[^>]*)>", line):
                path = Path(annotated[1])
                if any(path.is_relative_to(cache) for cache in cache_roots):
                    errors.append("persistent cache content read or mapped")
                if path.is_relative_to(repo) and path.suffix == ".rs" and path != repo / "src/selected.rs":
                    errors.append(f"unrelated source content read: {path.relative_to(repo)}")
    if errors:
        raise AssertionError("; ".join(sorted(set(errors))))
    return opens, source_opens


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    if sys.platform != "linux":
        print(f"SKIP warm-read syscall contract: Linux required (host: {sys.platform})")
        return 0
    strace = shutil.which("strace")
    if strace is None:
        print("SKIP warm-read syscall contract: strace is unavailable")
        return 0
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="dctx-warm-", dir=ROOT / "target") as tmp:
        root = Path(tmp)
        trace_file = root / "warm.strace"
        probe = subprocess.run([strace, "-o", str(trace_file), "--", "/bin/true"], capture_output=True, timeout=10)
        if probe.returncode:
            detail = probe.stderr.decode(errors="replace").strip()
            if "Operation not permitted" in detail or "Permission denied" in detail:
                print(f"SKIP warm-read syscall contract: ptrace is unavailable: {detail}")
                return 0
            raise AssertionError(f"strace probe failed: {detail}")
        repo = root / "repo"
        (repo / "src").mkdir(parents=True)
        selected = b"pub fn selected() -> u32 { 7 }\n"
        (repo / "src/selected.rs").write_bytes(selected)
        for index in range(32):
            folder = repo / f"unrelated/branch-{index}"
            folder.mkdir(parents=True)
            (folder / "other.rs").write_text(f"pub fn other_{index}() {{}}\n" * 128)
        env = isolated_env(root / "state")
        context = Context(binary, repo, env)
        # Populate the persistent whole-project index, then repeat a scoped read.
        # The cache exists and is usable; avoiding it cannot be a missing-cache
        # fallback. Scoped requests must remain independent of its size.
        context.run(["map", "find", "selected", "--json"])
        read = ["map", "read", "m:src/selected.rs", "--lines", "1-1", "--json"]
        cold = context.run(read)
        cache_parent = Path(env["DCTX_CACHE_DIR"])
        namespaces = [path for path in cache_parent.iterdir() if path.is_dir()]
        if len(namespaces) != 1:
            raise AssertionError("expected one engine cache namespace")
        cache = namespaces[0]
        if not (cache / "ledger.v1").is_file() or not any((cache / "objects").iterdir()):
            raise AssertionError("whole-project prime did not populate the persistent cache")
        # Unexpected cache reads cannot hide behind a missing-file fallback.
        for index in range(8):
            (cache / "objects" / f"unrelated-{index}").write_bytes(b"must not be opened")
        result = subprocess.run(
            [strace, "-f", "-qq", "-yy", "-s", "4096", "-e", "trace=open,openat,openat2,read,pread64,mmap,getdents64",
             "-o", str(trace_file), "--", str(binary), *read],
            cwd=repo, env=env, capture_output=True, timeout=30, close_fds=True,
        )
        if result.returncode or result.stderr:
            raise AssertionError(f"warm read failed: exit {result.returncode}: {result.stderr.decode(errors='replace')}")
        if result.stdout != cold or "selected" not in result.stdout.decode():
            raise AssertionError("warm read must preserve the cold read's exact output")
        cache_roots = [cache_parent, Path(env["DCTX_MAP_CACHE"]), Path(env["XDG_CACHE_HOME"])]
        opens, sources = check_trace(trace_file.read_text(), repo, cache_roots)
        print("Warm-read syscall contract PASS: expected cache set is empty; "
              f"{sum(opens.values())} cache opens; {sum(sources.values())} target opens; "
              "zero unrelated source opens/reads across 32 branches")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (AssertionError, ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"Warm-read syscall contract FAIL: {error}", file=sys.stderr)
        sys.exit(1)
