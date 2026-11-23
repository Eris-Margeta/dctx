#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Refuse (exit 1) unless EVERYTHING GitHub reports for <sha> is green:
#   1. a push-event run of ci.yml exists for the commit;
#   2. the newest run of every workflow on the commit is completed/success
#      (an older run of the same workflow that a newer run replaced does not count);
#   3. every check-run on the commit is completed/success, from any app
#      (failure, cancelled, skipped, neutral, timed_out or still running all refuse);
#   4. every commit status on the commit is success.
# Inside GitHub Actions the calling run itself (GITHUB_RUN_ID) is left out, so release.yml
# does not wait on its own still-running jobs.
# Used by release.yml, ops/npm/publish-dctx-npm.sh and the maintainer push scripts.
# Needs python3; GH_TOKEN is used when set (higher API rate limit), otherwise anonymous.
# Usage: require-green-ci.sh <sha> [owner/repo]
set -euo pipefail
sha=${1:?usage: require-green-ci.sh <sha> [owner/repo]}
repo=${2:-${GITHUB_REPOSITORY:-Eris-Margeta/dctx}}
exec python3 -I - "$sha" "$repo" <<'PY'
import json, os, sys, urllib.request

sha, repo = sys.argv[1], sys.argv[2]
api = os.environ.get("GITHUB_API_URL", "https://api.github.com")
token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or ""


def get(path):
    req = urllib.request.Request(f"{api}/repos/{repo}/{path}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("User-Agent", "dctx-require-green-ci")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.load(resp)


def pages(path, key):
    out, page = [], 1
    while True:
        sep = "&" if "?" in path else "?"
        data = get(f"{path}{sep}per_page=100&page={page}")
        items = data[key]
        out += items
        if len(items) < 100:
            return out
        page += 1


def refuse(msg):
    print(f"REFUSED: {msg}", file=sys.stderr)
    sys.exit(1)


try:
    sha = get(f"commits/{sha}")["sha"]
    own_id = os.environ.get("GITHUB_RUN_ID") if os.environ.get("GITHUB_ACTIONS") == "true" else None
    own_suite = get(f"actions/runs/{own_id}")["check_suite_id"] if own_id else None
    runs = [r for r in pages(f"actions/runs?head_sha={sha}", "workflow_runs")
            if r["head_sha"] == sha and str(r["id"]) != str(own_id)]
    checks = pages(f"commits/{sha}/check-runs?filter=latest", "check_runs")
    status = get(f"commits/{sha}/status")
except Exception as exc:  # network, auth, unknown sha: never treat as green
    refuse(f"cannot read GitHub checks for {sha}: {exc}")

if not any(r["path"].endswith("/ci.yml") and r["event"] == "push" for r in runs):
    refuse(f"no push-event CI run found for {sha}")

latest = {}
for r in sorted(runs, key=lambda r: (r["created_at"], r["id"])):
    latest[r["path"]] = r
bad = [f'{r["name"]}={r["status"]}/{r["conclusion"]}' for r in latest.values()
       if r["status"] != "completed" or r["conclusion"] != "success"]

replaced = {r["check_suite_id"] for r in runs} - {r["check_suite_id"] for r in latest.values()}
seen = 0
for c in checks:
    suite = c.get("check_suite", {}).get("id")
    if suite == own_suite or suite in replaced:
        continue
    seen += 1
    if c["status"] != "completed" or c["conclusion"] != "success":
        bad.append(f'check "{c["name"]}"={c["status"]}/{c["conclusion"]}')
for s in status.get("statuses", []):
    if s["state"] != "success":
        bad.append(f'status "{s["context"]}"={s["state"]}')

if not seen:
    refuse(f"no check-runs found for {sha}")
if bad:
    refuse(f"{sha} is not fully green: " + "; ".join(bad))
print(f"ALL GREEN for {sha}: {len(latest)} workflows, {seen} check-runs, "
      f"{len(status.get('statuses', []))} statuses")
PY
