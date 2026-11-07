#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
"""Run separate first/warm processes, capture evidence, and delete every catalog.

Only scratch and the supplied report (both inside the worktree) are written.
No cache dropping, fadvise on input trees, subprocess Git calls in input trees,
content sampling, or writes to the measured roots are performed.
"""
import argparse
import ctypes
import hashlib
import mmap
import datetime
import json
import os
from pathlib import Path
import platform
import resource
import subprocess
import time

PROJECT = Path(__file__).resolve().parent
WORKTREE = PROJECT.parent.parent


def command(*args):
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    return {"command": list(args), "status": result.returncode, "output": result.stdout}


def cgroup_memory_limit():
    relative = Path("/proc/self/cgroup").read_text().strip().split(":", 2)[-1].lstrip("/")
    path = Path("/sys/fs/cgroup") / relative / "memory.max"
    return path.read_text().strip() if path.is_file() else None


def machine():
    memory = Path("/proc/meminfo").read_text()
    mounts = {}
    for root in (WORKTREE,):
        if root.exists():
            disk = os.statvfs(root)
            mounts[str(root)] = {"capacity_bytes": disk.f_blocks * disk.f_frsize,
                                 "available_bytes": disk.f_bavail * disk.f_frsize,
                                 "free_bytes": disk.f_bfree * disk.f_frsize}
    return {"utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "git": command("git", "rev-parse", "HEAD"), "platform": platform.platform(),
            "uname": list(platform.uname()), "os_release": Path("/etc/os-release").read_text(),
            "nproc": command("nproc"), "lscpu": command("lscpu"),
            "meminfo": memory, "disk": mounts, "df": command("df", "-B1", str(WORKTREE)),
            "rustc": command("rustc", "-Vv"), "cargo": command("cargo", "-V"),
            "cgroup": Path("/proc/self/cgroup").read_text(),
            "loadavg": Path("/proc/loadavg").read_text(),
            "cgroup_memory_max_bytes": cgroup_memory_limit(),
            "drop_caches_writable": os.access("/proc/sys/vm/drop_caches", os.W_OK)}


def run(binary, args):
    print("RUN", *args, flush=True)
    start = time.monotonic()
    scratch = PROJECT / "scratch"
    stdout_path, stderr_path = scratch / "current.stdout", scratch / "current.stderr"
    with stdout_path.open("w") as out, stderr_path.open("w") as err:
        process = subprocess.Popen([str(binary), *map(str, args)], stdout=out, stderr=err, text=True)
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    stdout, stderr = stdout_path.read_text(), stderr_path.read_text()
    stdout_path.unlink()
    stderr_path.unlink()
    values = dict(line.split("=", 1) for line in stdout.splitlines() if "=" in line)
    result = {"arguments": list(map(str, args)), "status": process.returncode,
              "wall_seconds": time.monotonic() - start, "values": values, "stderr": stderr,
              "rusage_maxrss_kib": usage.ru_maxrss,
              "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime,
              "input_blocks": usage.ru_inblock, "output_blocks": usage.ru_oublock,
              "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest()}
    print(json.dumps(result), flush=True)
    return result


def resident_pages(path):
    """mincore checks residency without faulting file data into page cache."""
    with path.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_COPY) as view:
        page_count = (len(view) + mmap.PAGESIZE - 1) // mmap.PAGESIZE
        vector = (ctypes.c_ubyte * page_count)()
        anchor = ctypes.c_char.from_buffer(view)
        libc = ctypes.CDLL(None, use_errno=True)
        mincore = libc.mincore
        mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_ubyte)]
        mincore.restype = ctypes.c_int
        status = mincore(ctypes.addressof(anchor), len(view), vector)
        del anchor
        if status != 0:
            raise OSError(ctypes.get_errno(), "mincore")
        return {"resident": sum(bool(page & 1) for page in vector), "total": page_count}


def cold_catalog(binary, path):
    before = resident_pages(path)
    with path.open("rb") as stream:
        os.posix_fadvise(stream.fileno(), 0, 0, os.POSIX_FADV_DONTNEED)
    after = resident_pages(path)
    result = run(binary, ["inspect", path])
    result.update(residency_before=before, residency_after_eviction=after,
                  verified_cold=after["resident"] == 0)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--threads", type=int, default=len(os.sched_getaffinity(0)))
    parser.add_argument("--report", type=Path, default=WORKTREE / "docs/results/DCX-CATALOG-01.json")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--synthetic-only", action="store_true")
    mode.add_argument("--skip-synthetic", action="store_true")
    parser.add_argument("roots", nargs="*", type=Path)
    args = parser.parse_args()
    report = args.report.resolve()
    if not report.is_relative_to(WORKTREE):
        parser.error("report must be inside the worktree")
    os.umask(0o022)
    # Bound scratch allocation on unexpected input growth; not a measured target.
    resource.setrlimit(resource.RLIMIT_AS, (8 * 1024**3, 8 * 1024**3))
    scratch = PROJECT / "scratch"
    scratch.mkdir(exist_ok=True)
    output = scratch / f"bench-{os.getpid()}.cat"
    binary = PROJECT / "target/release/catalog-bench"
    data = {"machine": machine(), "method": "first-observed cache unknown; warm following first build/queries; 1000 full-result token queries each",
            "spdx_license_identifier": "Apache-2.0",
            "threads": args.threads, "runs": []}
    roots = [] if args.synthetic_only else (args.roots or [WORKTREE])
    checkpoint = scratch / f"checkpoint-{os.getpid()}.json"
    try:
        for root in roots:
            if not root.is_dir() or not os.access(root, os.R_OK | os.X_OK):
                data["runs"].append({"root": str(root), "unavailable": True})
                continue
            for phase in ("first_observed", "warm"):
                result = run(binary, ["build", root, output, args.threads])
                result.update(root=str(root), phase=phase)
                data["runs"].append(result)
                result["cold_catalog_reopen"] = cold_catalog(binary, output) if output.is_file() else None
                output.unlink(missing_ok=True)
                checkpoint.write_text(json.dumps(data, indent=2) + "\n")
        if not args.skip_synthetic:
            result = run(binary, ["synthetic", output, 1_000_000])
            result.update(root="synthetic:1M metadata records", phase="synthetic")
            result["cold_catalog_reopen"] = cold_catalog(binary, output) if output.is_file() else None
            data["runs"].append(result)
    finally:
        output.unlink(missing_ok=True)
        report.write_text(json.dumps(data, indent=2) + "\n")
        checkpoint.unlink(missing_ok=True)
    return int(any(run.get("status", 0) for run in data["runs"]))


if __name__ == "__main__":
    raise SystemExit(main())
