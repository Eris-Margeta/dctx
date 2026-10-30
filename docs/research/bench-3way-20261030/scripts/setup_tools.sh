#!/bin/sh
# Idempotent. Installs the two comparison tools into the scratch dir only (no sudo, nothing system-wide).
set -eu
B=${B3_ROOT:-/path/to/workdir/bench/dctx3}
TRAIL_VERSION=0.28.0
GRAPHIFY_VERSION=0.9.97
mkdir -p "$B/home" "$B/npm-cache" "$B/pip-cache" "$B/trail" "$B/tmp"
export HOME="$B/home" DO_NOT_TRACK=1 npm_config_cache="$B/npm-cache" npm_config_update_notifier=false npm_config_fund=false \
  npm_config_audit=false PIP_CACHE_DIR="$B/pip-cache" PIP_DISABLE_PIP_VERSION_CHECK=1
if [ "$("$B/trail/node_modules/.bin/trail" -v 2>/dev/null || true)" != "$TRAIL_VERSION" ]; then
  (cd "$B/trail" && nice -n 10 taskset -c 24-31 npm install --prefix "$B/trail" --save-exact "@trailhq/trail-code@$TRAIL_VERSION")
fi
if [ ! -x "$B/venv/bin/graphify" ] || [ "$("$B/venv/bin/graphify" --version 2>/dev/null)" != "graphify $GRAPHIFY_VERSION" ]; then
  python3 -m venv "$B/venv"
  nice -n 10 taskset -c 24-31 "$B/venv/bin/pip" install --no-input "graphifyy==$GRAPHIFY_VERSION"
fi
"$B/trail/node_modules/.bin/trail" -v; "$B/venv/bin/graphify" --version
