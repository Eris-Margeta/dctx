#!/bin/sh
# tlc.sh <spec.tla> <config.cfg>
# Run TLC once. TLC_TIMEOUT seconds, default 300.
# Exit 124 on timeout. The limit is a checker bound, not the protocol's P8 signal.
# Java is the VM's default JRE. The jar is a local tla2tools.jar or a pinned download.
set -u
here=$(cd "$(dirname "$0")" && pwd)

abs() {
  (cd "$(dirname "$1")" && printf '%s/%s\n' "$(pwd)" "$(basename "$1")")
}

jar_ok() {
  [ -s "$1" ] || return 1
  python3 - "$1" <<'PY'
import sys, zipfile
try:
    zipfile.ZipFile(sys.argv[1]).getinfo("tlc2/TLC.class")
except Exception:
    sys.exit(1)
PY
}

resolve_jar() {
  if [ -n "${TLA2TOOLS_JAR:-}" ] && jar_ok "${TLA2TOOLS_JAR}"; then
    printf '%s\n' "${TLA2TOOLS_JAR}"
    return 0
  fi
  for c in \
    "$here/tools/tla2tools.jar" \
    /opt/tla/tla2tools.jar \
    /usr/share/java/tla2tools.jar
  do
    if jar_ok "$c"; then
      printf '%s\n' "$c"
      return 0
    fi
  done
  dest="${TLA2TOOLS_CACHE:-/var/tmp/dctx-tla2tools.jar}"
  if ! jar_ok "$dest"; then
    rm -f "$dest"
    python3 - "$dest" <<'PY'
import socket, sys, urllib.request
socket.setdefaulttimeout(60)
dest = sys.argv[1]
urls = [
    "https://repo1.maven.org/maven2/org/lamport/tla2tools/1.8.0/tla2tools-1.8.0.jar",
    "https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar",
    "https://github.com/tlaplus/tlaplus/releases/download/v1.7.1/tla2tools.jar",
]
errors = []
for url in urls:
    try:
        urllib.request.urlretrieve(url, dest)
        sys.exit(0)
    except Exception as exc:
        errors.append("%s (%s)" % (url, exc))
sys.stderr.write("tla2tools download failed: %s\n" % "; ".join(errors))
sys.exit(1)
PY
  fi
  if jar_ok "$dest"; then
    printf '%s\n' "$dest"
    return 0
  fi
  echo "tla2tools.jar not usable at $dest" >&2
  return 1
}

spec=$(abs "$1")
cfg=$(abs "$2")
t=${TLC_TIMEOUT:-300}
jar=$(resolve_jar) || exit 1
meta=$(mktemp -d "${TMPDIR:-/tmp}/tlc.XXXXXX")
trap 'rm -rf "$meta"' EXIT
cd "$(dirname "$spec")"
if command -v java >/dev/null 2>&1; then
  JAVA=java
else
  echo "java not found" >&2
  exit 127
fi
# python3 is on the dev image; perl is not assumed. Exit 124 on the checker bound.
python3 - "$t" "$JAVA" -Xmx2g -XX:+UseParallelGC -cp "$jar" tlc2.TLC \
  -config "$cfg" -workers "${TLC_WORKERS:-2}" -metadir "$meta" -lncheck final \
  -deadlock "$(basename "$spec")" <<'PY'
import os, signal, sys
limit = int(sys.argv[1])
cmd = sys.argv[2:]
pid = os.fork()
if pid == 0:
    os.execvp(cmd[0], cmd)
    raise SystemExit(127)

def stop(signum, frame):
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass
    signal.signal(signal.SIGALRM, lambda *_a: None)
    signal.alarm(2)
    try:
        os.waitpid(pid, 0)
    except OSError:
        pass
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass
    raise SystemExit(124)

signal.signal(signal.SIGALRM, stop)
signal.alarm(limit)
_, status = os.waitpid(pid, 0)
signal.alarm(0)
if os.WIFEXITED(status):
    raise SystemExit(os.WEXITSTATUS(status))
raise SystemExit(1)
PY
