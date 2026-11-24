#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Refuse (exit 1) unless EVERYTHING GitHub reports for <sha> is green:
#   1. the latest main-push run of ci.yml and each workflow configured to run
#      on every main push at this commit is completed/success;
#   2. the newest run of every workflow on the commit is completed/success
#      (an older run of the same workflow that a newer run replaced does not count);
#   3. every check-run on the commit is completed/success, from any app,
#      apart from suites superseded by newer runs or belonging to the caller;
#   4. every commit status on the commit is success.
# Inside GitHub Actions the calling run itself (GITHUB_RUN_ID) is left out, so release.yml
# does not wait on its own still-running jobs.
# Used by release.yml, ops/npm/publish-dctx-npm.sh and the maintainer push scripts.
# Needs python3; GH_TOKEN is used when set (higher API rate limit).
# Usage: require-green-ci.sh <sha> [owner/repo]
set -euo pipefail
if [[ -z ${1:-} ]]; then
    echo 'usage: require-green-ci.sh <sha> [owner/repo]' >&2
    exit 1
fi
sha=$1
repo=${2:-${GITHUB_REPOSITORY:-Eris-Margeta/dctx}}
python=python3

# Provision only the Actions checkout entry point. A copied guard is standalone.
if [[ ${GITHUB_ACTIONS:-} == true && -n ${GITHUB_WORKSPACE:-} &&
      ${BASH_SOURCE[0]} -ef "$GITHUB_WORKSPACE/tools/release/require-green-ci.sh" ]]; then
    lock="$GITHUB_WORKSPACE/tools/release/ci-policy-requirements.txt"
    if [[ ! -r $lock || -z ${RUNNER_TEMP:-} ]]; then
        echo 'REFUSED: CI policy requirements or runner temporary directory unavailable.' >&2
        exit 1
    fi
    venv="$RUNNER_TEMP/dctx-ci-policy"
    python="$venv/bin/python"
    # Serialize initialization and record the exact lock only after pip succeeds.
    # A retry after an interrupted install must verify the wheels again.
    if ! (
        flock -x 9 || exit 1
        if [[ -x $python ]] && cmp -s "$lock" "$venv/installed-requirements"; then
            exit 0
        fi
        rm -f "$venv/installed-requirements" &&
        env -i PATH="$PATH" python3 -I -m venv "$venv" &&
        env -i PATH="$PATH" PIP_CONFIG_FILE=/dev/null "$python" -I -m pip \
            --disable-pip-version-check install --only-binary=:all: --no-deps \
            --require-hashes --force-reinstall -r "$lock" &&
        cp "$lock" "$venv/installed-requirements"
    ) >/dev/null 2>&1 9>"$RUNNER_TEMP/dctx-ci-policy.lock"; then
        echo 'REFUSED: could not provision the pinned CI policy parser.' >&2
        exit 1
    fi
fi

if ! command -v "$python" >/dev/null 2>&1; then
    echo 'REFUSED: activate a Python 3 venv and run python3 -m pip install PyYAML==6.0.2' >&2
    exit 1
fi
exec "$python" -I - "$sha" "$repo" <<'PY'
import base64
import json
import os
import re
import sys
import urllib.request
from urllib.parse import quote

try:
    import yaml
    if getattr(yaml, "__version__", None) != "6.0.2":
        raise ImportError
except Exception:
    sys.exit("REFUSED: activate a Python 3 venv and run python3 -m pip install PyYAML==6.0.2")

sha, repo = sys.argv[1], sys.argv[2]
api = os.environ.get("GITHUB_API_URL", "https://api.github.com")
token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN") or ""
workflows_dir = ".github/workflows"
ci_path = f"{workflows_dir}/ci.yml"


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
        if not isinstance(items, list):
            raise ValueError
        out += items
        if len(items) < 100:
            if "total_count" in data and len(out) != data["total_count"]:
                refuse("incomplete GitHub check results")
            return out
        page += 1


def log_text(value):
    # Escape API text within one log line, including Actions command markers.
    text = json.dumps(str(value), ensure_ascii=True)[1:-1]
    return text.replace("::", r"\x3a\x3a").replace("##[", r"\x23\x23[")


def refuse(msg):
    print(f"REFUSED: {msg}", file=sys.stderr)
    sys.exit(1)


class WorkflowLoader(yaml.SafeLoader):
    # Workflow names, events and globs are strings. Keep YAML null for an
    # empty event mapping, without YAML 1.1's conversion of "on" to True.
    yaml_implicit_resolvers = {
        key: [(tag, regex) for tag, regex in entries if tag == "tag:yaml.org,2002:null"]
        for key, entries in yaml.SafeLoader.yaml_implicit_resolvers.items()
    }

    def construct_mapping(self, node, deep=False):
        pairs = self.construct_pairs(node, deep=deep)
        mapping = dict(pairs)
        if len(mapping) != len(pairs):
            raise ValueError("duplicate workflow key")
        return mapping


def matches_main(pattern):
    # GitHub's * and ** have the same meaning for a branch without slashes.
    # A leading **/ can match zero directories. An embedded slash must match.
    while pattern.startswith("**/"):
        pattern = pattern[3:]
    parts = []
    for part in re.findall(r"\\.|\*+|\[[a-zA-Z0-9-]+\]|.", pattern, re.DOTALL):
        if part.startswith("*"):
            parts.append(".*")
        elif part in ("?", "+") or (part.startswith("[") and len(part) > 1):
            parts.append(part)
        else:
            parts.append(re.escape(part.removeprefix("\\")))
    return re.fullmatch("".join(parts), "main") is not None


def required_on_main(trigger):
    if isinstance(trigger, (str, list)):
        events = [trigger] if isinstance(trigger, str) else trigger
        if not all(isinstance(event, str) and re.fullmatch(r"[\w-]+", event) for event in events):
            raise ValueError("invalid workflow event list")
        return "push" in events
    if not isinstance(trigger, dict):
        raise ValueError("invalid workflow trigger declaration")
    if "push" not in trigger:
        return False
    push = trigger["push"]
    if not isinstance(push, dict):
        if push is None:
            return True
        raise ValueError("invalid push event filters")
    # A path-filtered workflow may be absent depending on the original push's diff.
    # Any runs that exist still go through the checks below.
    if "paths" in push or "paths-ignore" in push:
        return False
    for key in ("branches", "branches-ignore"):
        if key in push and (not isinstance(push[key], list) or
                            not all(isinstance(pattern, str) for pattern in push[key])):
            raise ValueError(f"invalid {key} filter list")
    if "branches" in push:
        applies = False
        for pattern in push["branches"]:
            if matches_main(pattern.removeprefix("!")):
                applies = not pattern.startswith("!")
        return applies
    if "branches-ignore" in push:
        return not any(matches_main(pattern) for pattern in push["branches-ignore"])
    return "tags" not in push and "tags-ignore" not in push


def workflow_policy(sha):
    policy = {}
    entries = get(f"contents/{workflows_dir}?ref={sha}")
    # The Contents API caps directory listings at 1,000 entries.
    if not isinstance(entries, list) or len(entries) >= 1000:
        refuse("cannot establish the complete candidate workflow list")
    for entry in entries:
        path = entry["path"]
        if entry["type"] != "file" or not path.endswith((".yml", ".yaml")):
            continue
        blob = get(f"contents/{quote(path)}?ref={sha}")
        try:
            if blob["encoding"] != "base64":
                raise ValueError
            source = base64.b64decode(blob["content"]).decode("utf-8-sig")
            document = yaml.load(source, Loader=WorkflowLoader)
            applies = required_on_main(document["on"])
        except (yaml.YAMLError, ValueError, KeyError, TypeError, RecursionError):
            refuse(f"cannot read workflow trigger for {log_text(path)}")
        policy[path] = applies
    policy[ci_path] = True
    return policy


def workflow_key(run, policy):
    path = run["path"]
    # Match the longest candidate filename before treating an @ as a ref separator.
    candidate = path
    while candidate:
        if candidate in policy:
            return candidate
        candidate = candidate.rpartition("@")[0]
    # An unlisted workflow still has a stable API identity across run refs.
    return run.get("workflow_id", path)


def success(result):
    return result["status"] == "completed" and result["conclusion"] == "success"


def outcome(result):
    return f'{log_text(result["status"])}/{log_text(result["conclusion"])}'


def evaluate(ref):
    sha = get(f"commits/{quote(ref, safe='')}")["sha"]
    if not re.fullmatch("[0-9a-f]{40}", sha) or (
        re.fullmatch("[0-9a-fA-F]{40}", ref) and ref.lower() != sha
    ):
        refuse("cannot resolve candidate commit")
    policy = workflow_policy(sha)
    required = {path for path, applies in policy.items() if applies}
    own_id = os.environ.get("GITHUB_RUN_ID") if os.environ.get("GITHUB_ACTIONS") == "true" else None
    own_suite = get(f"actions/runs/{own_id}")["check_suite_id"] if own_id else None
    runs = [r for r in pages(f"actions/runs?head_sha={sha}", "workflow_runs")
            if r["head_sha"] == sha and str(r["id"]) != str(own_id)]
    checks = pages(f"commits/{sha}/check-runs?filter=latest", "check_runs")

    latest, main_pushes = {}, {}
    for r in sorted(runs, key=lambda r: (r["created_at"], r["id"], r.get("run_attempt", 1))):
        # GitHub may append @ref to the workflow path. Compare the file across refs.
        path = workflow_key(r, policy)
        latest[path] = r
        if path in required and r["event"] == "push" and r.get("head_branch") == "main":
            main_pushes[path] = r

    bad = []
    for path in sorted(required):
        push = main_pushes.get(path)
        if push is None:
            bad.append(f"missing main push run for {log_text(path)}")
        elif not success(push):
            bad.append(f"main push {log_text(path)}={outcome(push)}")
    for path, r in latest.items():
        if r is not main_pushes.get(path) and not success(r):
            bad.append(f'{log_text(r["name"])}={outcome(r)}')

    retained = {r["check_suite_id"] for r in (*latest.values(), *main_pushes.values())}
    replaced = {r["check_suite_id"] for r in runs} - retained
    seen = 0
    for c in checks:
        suite = (c.get("check_suite") or {}).get("id")
        if suite is not None and (suite == own_suite or suite in replaced):
            continue
        seen += 1
        if not success(c):
            bad.append(f'check "{log_text(c["name"])}"={outcome(c)}')

    # Check the aggregate as well as every returned context. An empty
    # context set legitimately has the aggregate state "pending".
    count = 0
    page = 1
    while True:
        status = get(f"commits/{sha}/status?per_page=100&page={page}")
        total = status["total_count"]
        if page == 1:
            status_count = total
        contexts = status["statuses"]
        if (type(total) is not int or total < 0 or total != status_count
                or not isinstance(contexts, list) or len(contexts) != min(100, total - count)):
            refuse("incomplete combined status")
        if status["state"] != "success" and not (total == 0 and status["state"] == "pending"):
            bad.append(f'combined status={log_text(status["state"])}')
        for context in contexts:
            if context["state"] != "success":
                bad.append(f'status "{log_text(context["context"])}"={log_text(context["state"])}')
        count += len(contexts)
        if count == total:
            break
        page += 1

    if not seen:
        refuse(f"no check-runs found for {log_text(sha)}")
    if bad:
        refuse(f"{log_text(sha)} is not fully green: " + "; ".join(bad))
    print(f"ALL GREEN for {log_text(sha)}: {len(latest)} workflows, {seen} check-runs, "
          f"{log_text(status_count)} statuses")


try:
    evaluate(sha)
except Exception:
    # Transport exceptions can contain request headers, response text or URLs.
    refuse("cannot read GitHub checks for the candidate commit")
PY
