#!/bin/sh
# Host setup used for the October 2026 run (Debian 13, passwordless sudo for the orchestrating user).
# These are the exact steps that were executed, in order; paths are parameters. Run from the
# orchestrating account on the benchmark host. The dctx source (and any private corpus) is copied in
# with rsync from the workstation beforehand:
#   rsync -a --exclude=/target/ <dctx-src>/ host:$W/stage/dctx/
#   rsync -a --exclude=/target/ --exclude=/corpus/ <private-core>/ host:$W/stage/core/   (then: rm -rf .agent/worktrees)
set -eu
W=${BENCH_ROOT:-/path/to/workdir/dctx-vs-trail}
TRAIL_VERSION=0.23.1

sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q time strace      # removed again at the end
sudo mkdir -p "$W"
sudo useradd -m -d "$W/home" -s /bin/bash dctxbench
sudo chown -R dctxbench:dctxbench "$W"
sudo chmod 755 /path/to/workdir "$W"
sudo -u dctxbench mkdir -p "$W/bin" "$W/trail" "$W/npm-cache" "$W/runs" "$W/dctx-cache" "$W/work/dctx" "$W/work/trail"
sudo mkdir -p "$W/scripts" "$W/results" "$W/runs/tmp" "$W/stage"
sudo chown "$(id -un)": "$W/scripts" "$W/results" "$W/stage"
sudo chmod 1777 "$W/runs/tmp"
# scripts/ (this directory) is copied to $W/scripts; benv must not be writable by the bench user.

# dctx: release build as the orchestrating user (its toolchain), no compiler cache, 8 jobs on CPUs 24-31.
( cd "$W/stage/dctx" && RUSTC_WRAPPER= CARGO_TARGET_DIR="$W/stage/dctx-target" \
    /usr/bin/time -f "wall=%e user=%U sys=%S maxrss_kb=%M" taskset -c 24-31 \
    cargo build --release --locked -p dctx-cli -j 8 )
sudo install -o dctxbench -g dctxbench -m 755 "$W/stage/dctx-target/release/dctx-cli" "$W/bin/dctx"
ldd "$W/bin/dctx"; sha256sum "$W/bin/dctx"

# Trail-Code pinned. NOTE: set DO_NOT_TRACK=1 here as well; the published postinstall records an install
# event (it was not set in the measured run, see REPORT.md).
( cd "$W/trail" && sudo -u dctxbench env HOME="$W/home" npm_config_cache="$W/npm-cache" DO_NOT_TRACK=1 \
    npm_config_update_notifier=false npm_config_fund=false npm_config_audit=false \
    taskset -c 24-31 npm install --prefix "$W/trail" --save-exact "@trailhq/trail-code@$TRAIL_VERSION" )

# Public corpora, shallow clones at pinned tags/commits (network allowed only for this step).
mkdir -p "$W/pristine"
sudo -u dctxbench sh -c "cd '$W/pristine' && \
  git clone -q --depth 1 --branch v0.40.5 https://github.com/pocketbase/pocketbase.git pocketbase && \
  git clone -q --depth 1 --branch 15.2.0 https://github.com/BurntSushi/ripgrep.git ripgrep && \
  git clone -q --depth 1 --branch 3.1.3 https://github.com/pallets/flask.git flask && \
  git clone -q --depth 1 --branch 6.1.2 https://github.com/django/django.git django && \
  git clone -q https://github.com/trailhq/Trail-Code.git trailcode && git -C trailcode checkout -q b1211efd3eef0efa121b93800e5dd8e547aead77"
sudo cp -a "$W/stage/dctx" "$W/pristine/dctx"
[ -d "$W/stage/core" ] && sudo cp -a "$W/stage/core" "$W/pristine/core"
sudo chown -R dctxbench: "$W/pristine"

# Cleanup at the end of a run:
#   sudo userdel -r dctxbench && sudo rm -rf /path/to/workdir && sudo apt-get remove -y time strace
