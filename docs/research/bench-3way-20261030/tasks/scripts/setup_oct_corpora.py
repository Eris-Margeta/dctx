#!/usr/bin/env python3
"""Assemble the seven October corpora under $B3_ROOT/corpora-oct at the October pins. Network needed (not for the
local exports). Idempotent for the public ones.

flask, ripgrep, pocketbase  -> symlink to corpora/ (same SHAs as October, set up by setup_corpora.py)
django  -> tag 6.1.2 (2212534bae39...), shallow fetch
trailcode -> trailhq/Trail-Code at b1211efd3eef0efa121b93800e5dd8e547aead77 (0.23.1; the 3-way corpora/trailcode is the
             0.28.0 release commit, a different tree, so the October tree is cloned separately)
dctx    -> `git archive 17dff0b454c24f43848abad1c88ea982e02731f7` of a local dctx checkout (env DCTX_REPO), re-committed
core    -> `git archive f5f2f48b` of the private Rust repository checkout (env CORE_REPO), re-committed.
           Private: results for it are published as numbers only.
The exported trees are committed with `git add -A -f` so tracked-file counts equal October (1,749 and 3,190).
"""
import os
import subprocess
import sys

R = os.environ.get("B3_ROOT", "/path/to/workdir/bench/dctx3")
C = os.path.join(R, "corpora-oct")
E = dict(os.environ, GIT_TERMINAL_PROMPT="0", GIT_AUTHOR_NAME="bench", GIT_AUTHOR_EMAIL="bench@example.invalid",
         GIT_COMMITTER_NAME="bench", GIT_COMMITTER_EMAIL="bench@example.invalid",
         GIT_AUTHOR_DATE="2026-01-01T00:00:00+0000", GIT_COMMITTER_DATE="2026-01-01T00:00:00+0000")


def sh(cmd, cwd=None, **kw):
    return subprocess.run(cmd, cwd=cwd, env=E, check=True, **kw)


def export(repo, sha, dest):
    os.makedirs(dest)
    a = subprocess.Popen(["git", "-C", repo, "archive", sha], stdout=subprocess.PIPE)
    sh(["tar", "-x", "-C", dest], stdin=a.stdout)
    sh(["git", "init", "-q"], dest)
    sh(["git", "add", "-A", "-f"], dest)
    sh(["git", "commit", "-q", "-m", "corpus"], dest)


os.makedirs(C, exist_ok=True)
for n in ("flask", "ripgrep", "pocketbase"):
    if not os.path.lexists(os.path.join(C, n)):
        os.symlink(os.path.join(R, "corpora", n), os.path.join(C, n))
if not os.path.isdir(os.path.join(C, "django")):
    d = os.path.join(C, "django")
    os.makedirs(d)
    sh(["git", "init", "-q"], d)
    sh(["git", "remote", "add", "origin", "https://github.com/django/django.git"], d)
    sh(["git", "fetch", "-q", "--depth", "1", "origin", "refs/tags/6.1.2"], d)
    sh(["git", "checkout", "-q", "FETCH_HEAD"], d)
if not os.path.isdir(os.path.join(C, "trailcode")):
    d = os.path.join(C, "trailcode")
    sh(["git", "clone", "-q", "https://github.com/trailhq/Trail-Code.git", d])
    sh(["git", "checkout", "-q", "b1211efd3eef0efa121b93800e5dd8e547aead77"], d)
for name, env, sha in (("dctx", "DCTX_REPO", "17dff0b454c24f43848abad1c88ea982e02731f7"), ("core", "CORE_REPO", "f5f2f48b")):
    if not os.path.isdir(os.path.join(C, name)):
        if os.environ.get(env):
            export(os.environ[env], sha, os.path.join(C, name))
        else:
            print(f"skip {name}: set {env} to a local checkout containing {sha}", file=sys.stderr)
