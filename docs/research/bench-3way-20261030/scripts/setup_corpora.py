#!/usr/bin/env python3
"""Clone/assemble all benchmark corpora under $B3_ROOT/corpora at pinned SHAs. Idempotent. Network needed."""
import json, os, shutil, subprocess, sys

R = os.environ.get("B3_ROOT", "/path/to/workdir/bench/dctx3")
CORP = os.path.join(R, "corpora")
CFG = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "corpora.json")))
GENV = dict(os.environ, GIT_TERMINAL_PROMPT="0", GIT_AUTHOR_NAME="bench", GIT_AUTHOR_EMAIL="bench@example.invalid",
            GIT_COMMITTER_NAME="bench", GIT_COMMITTER_EMAIL="bench@example.invalid",
            GIT_AUTHOR_DATE="2026-01-01T00:00:00+0000", GIT_COMMITTER_DATE="2026-01-01T00:00:00+0000")


def sh(cmd, cwd=None):
    p = subprocess.run(cmd, cwd=cwd, env=GENV, capture_output=True, text=True)
    if p.returncode:
        raise RuntimeError(f"{cmd}: {p.stderr[-500:]}")
    return p.stdout.strip()


def fetch(url, sha, dest, keep_git):
    if os.path.isdir(dest):
        shutil.rmtree(dest)
    os.makedirs(dest)
    sh(["git", "init", "-q"], dest)
    sh(["git", "remote", "add", "origin", url], dest)
    sh(["git", "fetch", "-q", "--depth", "1", "origin", sha], dest)
    sh(["git", "checkout", "-q", "FETCH_HEAD"], dest)
    head = sh(["git", "rev-parse", "HEAD"], dest)
    assert head == sha, (head, sha)
    if not keep_git:
        shutil.rmtree(os.path.join(dest, ".git"))
    return head


def commit_all(d):
    sh(["git", "init", "-q"], d)
    sh(["git", "add", "-A"], d)
    sh(["git", "commit", "-q", "-m", "corpus"], d)
    return sh(["git", "rev-parse", "HEAD"], d)


def write(p, s):
    os.makedirs(os.path.dirname(p), exist_ok=True)
    open(p, "w").write(s)


def canary(name, tracked):
    d = os.path.join(CORP, name)
    if os.path.isdir(d):
        shutil.rmtree(d)
    write(d + "/a.py", "def helper(x):\n    return x + 1\n\n\ndef caller(y):\n    return helper(y) * 2\n")
    write(d + "/README.md", "# Canary fixture\n\nSmall repo with env files holding canary strings.\n")
    write(d + "/.env", "API_KEY=CANARY_ENV_7f3a9c21d0\nDB_PASSWORD=CANARY_ENVPW_44e1b7\n")
    write(d + "/.env.local", "LOCAL_TOKEN=CANARY_ENVLOCAL_b81d44e0a3\n")
    write(d + "/settings.env", "SECRET=CANARY_SETTINGS_5c0e92aa17\n")
    write(d + "/config/prod.env", "PROD_SECRET=CANARY_PROD_91ab33c0f2\n")
    if not tracked:
        write(d + "/.gitignore", ".env\n.env.*\n*.env\n")
    return commit_all(d)


def main():
    os.makedirs(CORP, exist_ok=True)
    out = {}
    for name, c in CFG["gold_corpora"].items():
        d = os.path.join(CORP, name)
        if os.path.isdir(d) and sh(["git", "rev-parse", "HEAD"], d) == c["sha"]:
            out[name] = c["sha"]
            continue
        out[name] = fetch(c["url"], c["sha"], d, True)
        print("fetched", name, out[name], flush=True)
    mixed = os.path.join(CORP, "mixed")
    if not os.path.isdir(os.path.join(mixed, ".git")):
        if os.path.isdir(mixed):
            shutil.rmtree(mixed)
        for lang, c in CFG["mixed_parts"].items():
            tmp = os.path.join(R, "tmp", "part-" + lang)
            fetch(c["url"], c["sha"], tmp, False)
            shutil.copytree(tmp, os.path.join(mixed, lang), symlinks=False, ignore=shutil.ignore_patterns(".git*"))
            shutil.rmtree(tmp)
            print("mixed part", lang, flush=True)
        commit_all(mixed)
    out["mixed"] = sh(["git", "rev-parse", "HEAD"], mixed)
    docs = os.path.join(CORP, "docs")
    if not os.path.isdir(os.path.join(docs, ".git")):
        if os.path.isdir(docs):
            shutil.rmtree(docs)
        exts = tuple(CFG["docs_exts"])
        for nm, c in CFG["docs_parts"].items():
            tmp = os.path.join(R, "tmp", "part-" + nm)
            fetch(c["url"], c["sha"], tmp, False)
            for dp, dns, fns in os.walk(tmp):
                for fn in fns:
                    if fn.lower().endswith(exts):
                        s = os.path.join(dp, fn)
                        t = os.path.join(docs, nm, os.path.relpath(s, tmp))
                        os.makedirs(os.path.dirname(t), exist_ok=True)
                        shutil.copy2(s, t)
            shutil.rmtree(tmp)
            print("docs part", nm, flush=True)
        commit_all(docs)
    out["docs"] = sh(["git", "rev-parse", "HEAD"], docs)
    out["canary-tracked"] = canary("canary-tracked", True)
    out["canary-ignored"] = canary("canary-ignored", False)
    json.dump(out, open(os.path.join(R, "corpora-shas.json"), "w"), indent=1, sort_keys=True)
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
