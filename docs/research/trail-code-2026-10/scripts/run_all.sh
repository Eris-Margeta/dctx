#!/bin/sh
# Full benchmark sequence (run on the benchmark host as the orchestrating user, after setup.md steps).
# Each phase appends JSONL rows under $BENCH_ROOT/results. Phases are independent per corpus and can be
# resumed by re-running a single phase script with the corpora still missing.
set -eu
cd "$(dirname "$0")"
ALL="flask ripgrep pocketbase trailcode dctx django core"
python3 phase_index.py $ALL
python3 select_targets.py ../pristine ../results/targets $ALL
python3 phase_query.py $ALL
python3 phase_accuracy.py $ALL
python3 phase_tasks.py $ALL
python3 phase_fresh.py $ALL
python3 phase_bound.py $ALL
python3 phase_coverage.py $ALL
python3 phase_net.py flask abort
python3 phase_mcp.py flask abort src/flask/helpers.py
python3 phase_init.py flask
python3 phase_adversarial.py
