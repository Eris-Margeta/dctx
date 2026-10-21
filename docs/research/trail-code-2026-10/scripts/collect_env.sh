#!/bin/sh
# Environment and version-pin dump for the benchmark host. usage: collect_env.sh > env.txt
W=${BENCH_ROOT:-/path/to/workdir/dctx-vs-trail}
echo "## date"; date -u +%Y-%m-%dT%H:%M:%SZ
echo "## uname"; uname -a
echo "## os-release"; sed -n '1,4p' /etc/os-release
echo "## cpu"; nproc; grep -m1 'model name' /proc/cpuinfo; grep -c processor /proc/cpuinfo
echo "## mem"; free -m | sed -n '1,2p'
echo "## load"; uptime
echo "## node/npm"; node -v; npm -v
echo "## rust (build host toolchain)"; "$HOME/.cargo/bin/rustc" -V; "$HOME/.cargo/bin/cargo" -V
echo "## dctx binary"; ls -l "$W/bin/dctx"; sha256sum "$W/bin/dctx"; ldd "$W/bin/dctx"; "$W/bin/dctx" --version
echo "## trail"; "$W/trail/node_modules/.bin/trail" --version
echo "## trail npm tree"; (cd "$W/trail" && npm ls --all 2>/dev/null)
echo "## trail install size bytes"; du -sb "$W/trail/node_modules" | cut -f1
echo "## corpora (pristine) commits"
for d in "$W"/pristine/*; do printf '%s %s %s\n' "$(basename "$d")" "$(git -c safe.directory='*' -C "$d" rev-parse HEAD 2>/dev/null)" "$(git -c safe.directory='*' -C "$d" ls-files | wc -l) files"; done
echo "## apt packages installed for the benchmark"; dpkg-query -W -f='${Package} ${Version}\n' time strace
echo "## user namespaces"; cat /proc/sys/kernel/unprivileged_userns_clone 2>/dev/null; sysctl -n user.max_user_namespaces 2>/dev/null
