#!/usr/bin/env python3
"""Shared helpers for the dctx vs Trail-Code benchmark (Python 3 stdlib only).

Runs as the orchestrating user on the benchmark host. Every tool invocation goes
through `benv`, which switches to the unprivileged bench user, clears the
environment, pins CPUs 24-31 and (unless net=True) enters a fresh network
namespace with `unshare -rn`.
"""
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import time
import uuid

W = os.environ.get("BENCH_ROOT", "/path/to/workdir/dctx-vs-trail")
BENV = os.path.join(W, "scripts", "benv")
RES = os.path.join(W, "results")
TMP = os.path.join(W, "runs", "tmp")
PRISTINE = os.path.join(W, "pristine")
WORK = os.path.join(W, "work")
OUT_KEEP = 256 * 1024  # bytes of each raw output kept on disk


def est_tokens(n_bytes):
    """Shared estimator for both tools: ceil(bytes / 3.5)."""
    return math.ceil(n_bytes / 3.5)


def loadavg():
    with open("/proc/loadavg") as f:
        return f.read().split()[0]


def _parse_time(path):
    d = {}
    try:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if ": " not in line:
                    continue
                k, v = line.rsplit(": ", 1)
                d[k.strip()] = v.strip()
    except FileNotFoundError:
        return {}
    out = {}
    try:
        el = d.get("Elapsed (wall clock) time (h:mm:ss or m:ss)")
        if el:
            parts = [float(x) for x in el.split(":")]
            s = 0.0
            for p in parts:
                s = s * 60 + p
            out["wall_s"] = s
        out["user_s"] = float(d.get("User time (seconds)", "nan"))
        out["sys_s"] = float(d.get("System time (seconds)", "nan"))
        out["maxrss_kb"] = int(d.get("Maximum resident set size (kbytes)", "0"))
        out["exit"] = int(d.get("Exit status", "-1"))
    except ValueError:
        pass
    return out


def append_jsonl(name, row):
    os.makedirs(RES, exist_ok=True)
    with open(os.path.join(RES, name), "a") as f:
        f.write(json.dumps(row, sort_keys=True) + "\n")


def run(cmd, cwd, *, tool, corpus, label, rep=0, timeout=900, extra_env=None, net=False,
        log="runs.jsonl", keep=True, stdin=None):
    """Run one tool command in the isolated environment and record a row."""
    os.makedirs(TMP, exist_ok=True)
    tfile = os.path.join(TMP, uuid.uuid4().hex + ".time")
    argv = [BENV] + (["--net"] if net else []) + [
        "/usr/bin/time", "-v", "-o", tfile, "timeout", "-k", "5", str(timeout)] + list(cmd)
    env = dict(os.environ)
    if extra_env:
        env["BENV_EXTRA"] = " ".join(f"{k}={v}" for k, v in extra_env.items())
    else:
        env.pop("BENV_EXTRA", None)
    l0 = loadavg()
    t0 = time.monotonic()
    try:
        p = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, timeout=timeout + 60,
                           input=stdin)
        out, err, rc = p.stdout, p.stderr, p.returncode
    except subprocess.TimeoutExpired as e:
        out, err, rc = e.stdout or b"", e.stderr or b"", -9
    outer = time.monotonic() - t0
    tm = _parse_time(tfile)
    # /usr/bin/time appends "Command exited with non-zero status" to its -o file only; stderr untouched.
    try:
        os.unlink(tfile)
    except OSError:
        pass
    row = {
        "tool": tool, "corpus": corpus, "label": label, "rep": rep,
        "cmd": " ".join(cmd), "cwd": cwd, "rc": rc, "timed_out": rc == 124 or rc == -9,
        "outer_wall_s": round(outer, 4), "load1_before": l0, "load1_after": loadavg(),
        "stdout_bytes": len(out), "stderr_bytes": len(err),
        "stdout_sha256": hashlib.sha256(out).hexdigest(),
        "est_tokens": est_tokens(len(out)),
    }
    row.update(tm)
    if keep:
        d = os.path.join(RES, "outputs", corpus, tool)
        os.makedirs(d, exist_ok=True)
        safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", label)[:120]
        with open(os.path.join(d, f"{safe}.r{rep}.out"), "wb") as f:
            f.write(out[:OUT_KEEP])
        if err:
            with open(os.path.join(d, f"{safe}.r{rep}.err"), "wb") as f:
                f.write(err[:16384])
    append_jsonl(log, row)
    row["_stdout"] = out
    row["_stderr"] = err
    return row


def as_bench(cmd, cwd="/", net=True, timeout=1800):
    """Run a plain shell command as the bench user (setup steps, not measured)."""
    argv = [BENV] + (["--net"] if net else []) + ["sh", "-c", cmd]
    p = subprocess.run(argv, cwd=cwd, capture_output=True, timeout=timeout)
    if p.returncode != 0:
        raise RuntimeError(f"as_bench failed rc={p.returncode}: {cmd}\n{p.stderr.decode(errors='replace')[-2000:]}")
    return p.stdout.decode(errors="replace")


def fresh_copy(corpus, tool, suffix=""):
    dst = os.path.join(WORK, tool, corpus + suffix)
    as_bench(f"rm -rf '{dst}' && mkdir -p '{os.path.dirname(dst)}' && cp -a '{PRISTINE}/{corpus}' '{dst}'")
    return dst


def du_bytes(path):
    out = as_bench(f"[ -e '{path}' ] && du -sb '{path}' | cut -f1 || echo 0")
    return int(out.strip() or 0)


def manifest(path, exclude_git=True):
    """path -> (size, sha256) for every regular file and symlink under path (bench user reads)."""
    script = (
        "import os,sys,hashlib,json\n"
        "root=sys.argv[1]\nres={}\n"
        "for dp,dns,fns in os.walk(root):\n"
        "  if '.git' in dns and %s: dns.remove('.git')\n"
        "  for fn in fns:\n"
        "    p=os.path.join(dp,fn); r=os.path.relpath(p,root)\n"
        "    try:\n"
        "      if os.path.islink(p): res[r]=['link',os.readlink(p)]; continue\n"
        "      h=hashlib.sha256()\n"
        "      with open(p,'rb') as f:\n"
        "        for b in iter(lambda:f.read(1<<20),b''): h.update(b)\n"
        "      res[r]=[os.path.getsize(p),h.hexdigest()]\n"
        "    except OSError as e: res[r]=['err',str(e)]\n"
        "print(json.dumps(res))\n" % ("True" if exclude_git else "False")
    )
    argv = [BENV, "--net", "python3", "-I", "-c", script, path]
    p = subprocess.run(argv, capture_output=True, timeout=1800)
    if p.returncode != 0:
        raise RuntimeError(p.stderr.decode(errors="replace"))
    return json.loads(p.stdout.decode("utf-8", errors="surrogateescape"))


def manifest_diff(before, after):
    added = sorted(k for k in after if k not in before)
    removed = sorted(k for k in before if k not in after)
    changed = sorted(k for k in after if k in before and after[k] != before[k])
    return {"added": added, "removed": removed, "changed": changed}


def median(xs):
    xs = sorted(x for x in xs if x is not None and not (isinstance(x, float) and math.isnan(x)))
    if not xs:
        return None
    n = len(xs)
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


def load_config():
    with open(os.path.join(W, "scripts", "corpora.json")) as f:
        return json.load(f)


def trail_index_dir(d):
    """Trail writes its graph to <repo>/graft, or to <repo>/.trail/cache when the repo already has .trail/."""
    if os.path.isdir(os.path.join(d, "graft", ".graph")):
        return os.path.join(d, "graft")
    if os.path.isdir(os.path.join(d, ".trail", "cache", ".graph")):
        return os.path.join(d, ".trail", "cache")
    return os.path.join(d, "graft")
