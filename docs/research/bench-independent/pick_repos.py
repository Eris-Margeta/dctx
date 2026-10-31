#!/usr/bin/env python3
"""Seeded repository selection for the independent benchmark (v1 card 6). Python 3 stdlib only.

usage: pick_repos.py [--seed HEX | --seed-ref REF --repo DCTX_GIT_DIR] [--no-resolve] [--out selection.json]

The seed is the full sha of the dctx commit that REF (default v1.0.0) points to, so nobody can choose the repos after
seeing results: the tag is cut first, the repos follow from it. For each language in repos.json the pick is
sha256("<seed>:<lang>") mod len(candidates). Tags are resolved to commits with `git ls-remote` (network) unless
--no-resolve; the clone step then refuses any other commit.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LANGS = ("rust", "python", "go")


def load(path):
    """The candidate list, exclusions and pins (repos.json)."""
    with open(path) as f:
        return json.load(f)


def check_excluded(cfg):
    """Raise ValueError when a candidate is a repository the dctx team measured or tuned on."""
    bad = {u.lower().rstrip("/") for u in cfg["excluded"]["urls"]}
    for lang in LANGS:
        for c in cfg["candidates"][lang]:
            if c["url"].lower().rstrip("/") in bad:
                raise ValueError(f"candidate {c['name']} is an excluded corpus: {c['url']}")


def pick(cfg, seed):
    """One candidate per language, by the published rule. `seed` must be a full 40 or 64 hex commit id."""
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", seed or ""):
        raise ValueError(f"seed must be a full commit sha, got {seed!r}")
    check_excluded(cfg)
    out = []
    for lang in LANGS:
        cands = cfg["candidates"][lang]
        i = int(hashlib.sha256(f"{seed}:{lang}".encode()).hexdigest(), 16) % len(cands)
        out.append({"lang": lang, "index": i, "name": cands[i]["name"], "url": cands[i]["url"], "tag": cands[i]["tag"]})
    return out


def resolve_tag(url, tag):
    """Commit sha the tag points to (the peeled ^{} line wins over the tag object)."""
    p = subprocess.run(["git", "ls-remote", "--tags", url, f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"],
                       capture_output=True, text=True, env=dict(os.environ, GIT_TERMINAL_PROMPT="0"), timeout=120)
    if p.returncode:
        raise RuntimeError(f"ls-remote {url} {tag}: {p.stderr.strip()[-300:]}")
    refs = {ref: sha for sha, ref in (line.split("\t", 1) for line in p.stdout.splitlines() if "\t" in line)}
    sha = refs.get(f"refs/tags/{tag}^{{}}") or refs.get(f"refs/tags/{tag}")
    if not sha:
        raise RuntimeError(f"tag {tag} not found at {url}; the candidate list is broken, fix repos.json before the v1.0.0 tag")
    return sha


def seed_from_ref(repo, ref):
    """Full commit sha of REF in the dctx repository."""
    p = subprocess.run(["git", "-C", repo, "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"], capture_output=True, text=True)
    if p.returncode:
        raise RuntimeError(f"{ref} does not exist in {repo}: tag v1.0.0 first, or pass --seed and say why in the results")
    return p.stdout.strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seed")
    ap.add_argument("--seed-ref", default="v1.0.0")
    ap.add_argument("--repo", default=os.path.abspath(os.path.join(HERE, "..", "..", "..")))
    ap.add_argument("--config", default=os.path.join(HERE, "repos.json"))
    ap.add_argument("--no-resolve", action="store_true")
    ap.add_argument("--out", default=os.path.join(HERE, "selection.json"))
    a = ap.parse_args()
    cfg = load(a.config)
    seed, how = (a.seed, "explicit --seed") if a.seed else (seed_from_ref(a.repo, a.seed_ref), f"git rev-parse {a.seed_ref}")
    sel = pick(cfg, seed)
    for s in sel:
        s["sha"] = None if a.no_resolve else resolve_tag(s["url"], s["tag"])
    doc = {"seed": seed, "seed_source": how, "rule": cfg["seed_rule"], "selection": sel, "pins": cfg["pins"]}
    with open(a.out, "w") as f:
        json.dump(doc, f, indent=1)
    for s in sel:
        print(f"{s['lang']:7} {s['name']:12} {s['tag']:10} {s['sha'] or '(unresolved)'}")
    print("SELECTION", a.out)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError) as e:
        print(f"select: {e}", file=sys.stderr)
        sys.exit(2)
