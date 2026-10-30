#!/usr/bin/env python3
"""Shared helpers for the 3-way benchmark (dctx vs Trail-Code vs graphify). Python 3 stdlib only.

Every measured command is executed as:
    nice -n 10 taskset -c 24-31 [unshare -Urn] env -i K=V... CMD...
with a scratch HOME / caches, DO_NOT_TRACK=1 and GRAPHIFY_BACKEND=none. Wall time is taken by the orchestrator around
wait4(); peak RSS is ru_maxrss from wait4 (largest single process) plus, for index runs, a sampled sum of the process
tree (RSS and PSS, 50 ms interval). Token estimate everywhere: ceil(stdout_bytes / 3.5).
"""
import hashlib
import json
import math
import os
import re
import shutil
import signal
import subprocess
import time

R = os.environ.get("B3_ROOT", "/path/to/workdir/bench/dctx3")
RES = os.environ.get("B3_RESULTS", os.path.join(R, "results", "adhoc"))
DCTX = os.environ.get("B3_DCTX", "/path/to/workdir/target/dctx-base/release/dctx-cli")
SCRIPTS = os.path.join(R, "scripts")
CORP = os.path.join(R, "corpora")
WORK = os.path.join(R, "work")
CACHE = os.path.join(R, "cache")
TMP = os.path.join(R, "tmp")
OUT_KEEP = 64 * 1024
CPUS = os.environ.get("B3_CPUS", "24-31")
UNSHARE = None  # decided by detect_unshare()
PHASE = "adhoc"


def est_tokens(n):
    return math.ceil(n / 3.5)


def sha_nohdr(out):
    """sha256 of stdout without a leading '# ...' header line (dctx prints a snapshot id there that changes on any edit)."""
    if out.startswith(b"# "):
        nl = out.find(b"\n")
        out = out[nl + 1:] if nl >= 0 else b""
    return hashlib.sha256(out).hexdigest()


def detect_unshare():
    global UNSHARE
    try:
        p = subprocess.run(["unshare", "-Urn", "true"], capture_output=True, timeout=20)
        UNSHARE = p.returncode == 0
    except Exception:
        UNSHARE = False
    return UNSHARE


def base_env(extra=None, dnt=True, home=None):
    e = {
        "HOME": home or os.path.join(R, "home"),
        "PATH": f"{R}/venv/bin:{R}/trail/node_modules/.bin:/usr/bin:/bin",
        "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8",
        "GRAPHIFY_BACKEND": "none",
        "npm_config_cache": os.path.join(R, "npm-cache"),
        "npm_config_update_notifier": "false",
        "XDG_CACHE_HOME": os.path.join(R, "home", ".cache"),
        "TMPDIR": TMP,
        "NO_COLOR": "1",
    }
    if dnt:
        e["DO_NOT_TRACK"] = "1"
    if extra:
        e.update(extra)
    return e


def wrap(argv, env):
    pre = ["nice", "-n", "10", "taskset", "-c", CPUS]
    if UNSHARE:
        pre += ["unshare", "-Urn"]
    return pre + ["env", "-i"] + [f"{k}={v}" for k, v in env.items()] + list(argv)


def loadavg():
    with open("/proc/loadavg") as f:
        return float(f.read().split()[0])


def _tree_pids(root):
    kids = {}
    for d in os.listdir("/proc"):
        if not d.isdigit():
            continue
        try:
            with open(f"/proc/{d}/stat") as f:
                s = f.read()
            ppid = int(s[s.rindex(")") + 2:].split()[1])
        except (OSError, ValueError, IndexError):
            continue
        kids.setdefault(ppid, []).append(int(d))
    out, stack = [], [root]
    while stack:
        p = stack.pop()
        out.append(p)
        stack.extend(kids.get(p, []))
    return out


def _tree_mem(root):
    rss = pss = 0
    for p in _tree_pids(root):
        try:
            with open(f"/proc/{p}/smaps_rollup") as f:
                for line in f:
                    if line.startswith("Rss:"):
                        rss += int(line.split()[1])
                    elif line.startswith("Pss:"):
                        pss += int(line.split()[1])
        except OSError:
            pass
    return rss, pss


def append_jsonl(name, row):
    os.makedirs(RES, exist_ok=True)
    with open(os.path.join(RES, name), "a") as f:
        f.write(json.dumps(row, sort_keys=True, default=str) + "\n")


def run(argv, cwd, *, tool, corpus, label, rep=0, env_extra=None, dnt=True, home=None, timeout=300, sample=False,
        keep=True, log="runs.jsonl", meta=None, wrapped=True):
    """Run one command in the isolated, pinned environment; log a row; return it (with _out / _err bytes)."""
    os.makedirs(TMP, exist_ok=True)
    env = base_env(env_extra, dnt=dnt, home=home)
    full = wrap(argv, env) if wrapped else list(argv)
    of = os.path.join(TMP, f"o{os.getpid()}.out")
    ef = os.path.join(TMP, f"o{os.getpid()}.err")
    l0 = loadavg()
    with open(of, "wb") as fo, open(ef, "wb") as fe:
        t0 = time.monotonic()
        p = subprocess.Popen(full, cwd=cwd, stdin=subprocess.DEVNULL, stdout=fo, stderr=fe, start_new_session=True)
        peak_rss = peak_pss = 0
        nxt = t0
        timed_out = False
        ru = None
        while True:
            pid, st, ru = os.wait4(p.pid, os.WNOHANG)
            if pid:
                t1 = time.monotonic()
                break
            now = time.monotonic()
            if now - t0 > timeout:
                timed_out = True
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except OSError:
                    pass
            if sample and now >= nxt:
                a, b = _tree_mem(p.pid)
                peak_rss, peak_pss = max(peak_rss, a), max(peak_pss, b)
                nxt = now + 0.05
            time.sleep(0.001 if now - t0 < 2 else 0.01)
    p.returncode = os.waitstatus_to_exitcode(st)
    try:
        os.killpg(p.pid, signal.SIGKILL)  # stray background children
    except OSError:
        pass
    out = open(of, "rb").read()
    err = open(ef, "rb").read()
    row = {
        "phase": PHASE, "tool": tool, "corpus": corpus, "label": label, "rep": rep,
        "cmd": " ".join(argv)[:300], "rc": p.returncode, "timed_out": timed_out,
        "wall_s": round(t1 - t0, 5), "user_s": round(ru.ru_utime, 4), "sys_s": round(ru.ru_stime, 4),
        "maxrss_kb": ru.ru_maxrss, "load1_before": l0, "load1_after": loadavg(),
        "stdout_bytes": len(out), "stderr_bytes": len(err),
        "stdout_sha256": hashlib.sha256(out).hexdigest(), "stdout_sha256_nohdr": sha_nohdr(out),
        "est_tokens": est_tokens(len(out)),
    }
    if sample:
        row["peak_tree_rss_kb"], row["peak_tree_pss_kb"] = peak_rss, peak_pss
    if meta:
        row.update(meta)
    if keep:
        d = os.path.join(RES, "outputs", corpus, tool)
        os.makedirs(d, exist_ok=True)
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", label)[:100]
        with open(os.path.join(d, f"{safe}.r{rep}.out"), "wb") as f:
            f.write(out[:OUT_KEEP])
        if err:
            with open(os.path.join(d, f"{safe}.r{rep}.err"), "wb") as f:
                f.write(err[:8192])
    append_jsonl(log, row)
    row["_out"], row["_err"] = out, err
    return row


def median(xs):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    n = len(xs)
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


def dir_bytes(path):
    tot, n = 0, 0
    for dp, dns, fns in os.walk(path):
        for fn in fns:
            try:
                tot += os.lstat(os.path.join(dp, fn)).st_size
                n += 1
            except OSError:
                pass
    return tot, n


def fresh_copy(tool, corpus, suffix=""):
    dst = os.path.join(WORK, tool, corpus + suffix)
    shutil.rmtree(dst, ignore_errors=True)
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    subprocess.run(["cp", "-a", os.path.join(CORP, corpus), dst], check=True)
    return dst


def git_touched(d):
    """Top-level names git reports as changed/untracked after a tool ran (what the tool wrote into the repo)."""
    p = subprocess.run(["git", "-c", "safe.directory=*", "-C", d, "status", "--porcelain", "--untracked-files=all"],
                       capture_output=True, text=True)
    mod, new = [], set()
    for line in p.stdout.splitlines():
        code, path = line[:2], line[3:]
        if code.strip() == "??":
            new.add(path.split("/")[0] + ("/" if "/" in path else ""))
        else:
            mod.append(path)
    return {"modified_tracked": sorted(mod)[:20], "untracked_top": sorted(new)[:20]}
