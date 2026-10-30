#!/bin/sh
# Task-level rerun (70 agent questions, October protocol): dctx vs Trail-Code vs graphify (code-only).
# usage: ./run-tasks.sh <dctx-binary> [run_tasks.py args, e.g. --corpora flask,ripgrep --tools dctx,trail]
# Needs: tools installed by scripts/setup_tools.sh; corpora-oct/ (pinned October corpora, see tasks/setup_oct_corpora.py).
# Writes results/tasks-<timestamp>/{tasks.jsonl,tasks_aggregate.json,tasks_headline.md,outputs/}.
set -eu
[ $# -ge 1 ] || { echo "usage: $0 <dctx-binary> [run_tasks.py args]" >&2; exit 2; }
DCTX=$1; shift
[ -x "$DCTX" ] || { echo "not executable: $DCTX" >&2; exit 2; }
export B3_ROOT=${B3_ROOT:-$(cd "$(dirname "$0")" && pwd)}
export B3_DCTX=$(readlink -f "$DCTX")
export B3_RESULTS="$B3_ROOT/results/tasks-$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$B3_RESULTS" "$B3_ROOT/tmp" "$B3_ROOT/cache"
cd "$B3_ROOT/scripts"
sh setup_tools.sh >"$B3_RESULTS/setup_tools.log" 2>&1      # idempotent; network only if a tool is missing
python3 - <<PY >"$B3_RESULTS/versions.txt"
import subprocess,os
r=os.environ["B3_ROOT"]
for c in ([os.environ["B3_DCTX"],"--version"],[r+"/trail/node_modules/.bin/trail","-v"],[r+"/venv/bin/graphify","--version"]):
    print(subprocess.run(c,capture_output=True,text=True,env={"DO_NOT_TRACK":"1","PATH":"/usr/bin:/bin"}).stdout.strip())
PY
python3 run_tasks.py "$@" 2>&1 | tee "$B3_RESULTS/run.log"
python3 tasks_score.py "$B3_RESULTS" --sanitize
echo "RESULTS: $B3_RESULTS"
