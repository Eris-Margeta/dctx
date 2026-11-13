#!/usr/bin/env bash
# release-check.sh: the dctx v1 release gate, one command from the repo root on Linux.
#
#   scripts/release-check.sh                   cargo build/test/clippy, then every gate below on the release dctx-cli
#   scripts/release-check.sh --quick [BIN]     skip cargo; use BIN or $DCTX_BIN as dctx-cli
#   scripts/release-check.sh --scan-only [DIR] only the shipped-code scan, on DIR (default: the repo root)
#
# Gates, in order: cargo-build, cargo-test, cargo-clippy (skipped by --quick), code-scan, determinism,
# unrelated-edit, network. Each prints `PASS <gate>`, `FAIL <gate>: <why>` or `SKIP <gate>: <why>`.
# The last line is `RELEASE-CHECK PASS`, or `RELEASE-CHECK FAIL: <failed gates>` with exit status 1 (2 = usage).
# The Linux build host has no Tauri system libraries, so cargo runs on the workspace without dctx-desktop,
# exactly as CI and docs/STYLE.md do. The committed bench floors are checked by scripts/bench_floors.py.
set -u
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
quick=0; scan_only=0; arg=""
usage() { sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-2}"; }
while [ $# -gt 0 ]; do
  case $1 in
    --quick) quick=1 ;;
    --scan-only) scan_only=1 ;;
    -h|--help) usage 0 ;;
    -*) echo "release-check: unknown option $1" >&2; usage 2 ;;
    *) [ -z "$arg" ] || usage 2; arg=$1 ;;
  esac
  shift
done
FAILED=()
pass() { echo "PASS $1"; }
skip() { echo "SKIP $1: $2"; }
fail() { echo "FAIL $1: $2"; FAILED+=("$1"); }
finish() {
  if [ ${#FAILED[@]} -eq 0 ]; then echo "RELEASE-CHECK PASS"; exit 0; fi
  echo "RELEASE-CHECK FAIL: ${FAILED[*]}"; exit 1
}
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -c1-64; else shasum -a 256 "$1" | cut -c1-64; fi; }

# ---- code-scan: no todo!/unimplemented!/FIXME in shipped Rust, no #[ignore] without an issue link.
# Shipped = *.rs outside tests/, benches/, examples/ and fixture trees, not *_tests.rs / tests.rs / *_test.rs,
# and only the lines before a file's first #[cfg(test)]. An issue link is `issues/<n>` (a tracker URL) or `issue #<n>`.
code_scan() {
  local dir=$1 debt ignores
  local prune=(-name target -o -name .git -o -name node_modules -o -name fixtures -o -name testdata -o -name corpus -o -name .dcxgate)
  debt=$(cd "$dir" && find . \( "${prune[@]}" -o -name tests -o -name benches -o -name examples \) -prune -o \
      -type f -name '*.rs' ! -name '*_tests.rs' ! -name 'tests.rs' ! -name '*_test.rs' -print0 | sort -z |
    xargs -0 awk 'FNR == 1 { t = 0 } /^[[:space:]]*#\[cfg\(test\)\]/ { t = 1 } !t && /todo!|unimplemented!|FIXME/ { print FILENAME ":" FNR ": " $0 }' 2>/dev/null)
  ignores=$(cd "$dir" && find . \( "${prune[@]}" \) -prune -o -type f -name '*.rs' -print0 | sort -z |
    xargs -0 grep -HnE '#\[ignore([] =]|$)|cfg_attr\(.*[(, ]ignore[]), =]' 2>/dev/null | grep -vE 'issues/[0-9]+|issue #[0-9]+')
  if [ -z "$debt" ] && [ -z "$ignores" ]; then pass code-scan; return; fi
  [ -z "$debt" ] || { echo "  todo!/unimplemented!/FIXME in shipped code:"; echo "$debt" | sed 's/^\.\//    /'; }
  [ -z "$ignores" ] || { echo "  #[ignore] without an issue link:"; echo "$ignores" | sed 's/^\.\//    /'; }
  fail code-scan "$(printf '%s' "$debt" | grep -c '^') debt line(s), $(printf '%s' "$ignores" | grep -c '^') unlinked #[ignore]"
}

if [ $scan_only = 1 ]; then code_scan "${arg:-$ROOT}"; finish; fi

# ---- cargo gates (full mode). Tests close inherited descriptors first, as CI does: native confinement rejects
# every descriptor beyond its three protocol pipes.
cd "$ROOT" || exit 2
if [ $quick = 0 ]; then
  [ -z "$arg" ] || usage 2
  CARGO_WS=(--workspace --exclude dctx-desktop --locked)
  if cargo build --release "${CARGO_WS[@]}"; then pass cargo-build; else fail cargo-build "cargo build --release failed"; finish; fi
  if python3 -c 'import subprocess, sys; sys.exit(subprocess.call(sys.argv[1:], close_fds=True))' cargo test "${CARGO_WS[@]}"; then
    pass cargo-test; else fail cargo-test "cargo test --workspace failed"; fi
  if cargo clippy "${CARGO_WS[@]}" --all-targets -- -D warnings -A clippy::field_reassign_with_default -A clippy::assertions_on_constants; then
    pass cargo-clippy; else fail cargo-clippy "cargo clippy -D warnings failed"; fi
  target=$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
  BIN=$target/release/dctx-cli
else
  BIN=${arg:-${DCTX_BIN:-}}
  [ -n "$BIN" ] || { echo "release-check: --quick needs a dctx-cli binary (argument or DCTX_BIN)" >&2; exit 2; }
fi
case $BIN in /*) ;; *) BIN=$PWD/$BIN ;; esac
[ -x "$BIN" ] || { fail binary "not executable: $BIN"; finish; }
echo "binary: $BIN ($("$BIN" --version 2>&1 | head -1))"

code_scan "$ROOT"

# ---- built-in fixture: Python calls across files, a Rust pair, a doc, and one file nothing queries.
WORK=$(mktemp -d "${TMPDIR:-/tmp}/dctx-release.XXXXXX") || exit 2
trap 'rm -rf "$WORK"' EXIT
FX=$WORK/fx; mkdir -p "$FX/app" "$FX/lib" "$FX/rs"
printf 'from lib.util import helper\n\n\ndef main():\n    return helper(2) + helper(3)\n' > "$FX/app/main.py"
printf 'def helper(x):\n    return double(x) + 1\n\n\ndef double(x):\n    return x * 2\n' > "$FX/lib/util.py"
printf 'def unrelated():\n    return 0\n' > "$FX/app/extra.py"
printf 'pub fn parse(s: &str) -> usize {\n    count(s)\n}\n\nfn count(s: &str) -> usize {\n    s.len()\n}\n' > "$FX/rs/core.rs"
printf '# Fixture\n\nA tiny project for the release gate.\n\n## Usage\n\nCall `main`.\n' > "$FX/README.md"
export DCTX_AUTO_CLEAN=0
QUERIES=(map find skeleton neighbors hubs)
query_args() {
  case $1 in
    map) Q=(map map lib/util.py) ;;
    find) Q=(map find helper --limit 10) ;;
    skeleton) Q=(map skeleton lib/util.py) ;;
    neighbors) Q=(map neighbors 's:lib/util.py#helper' --dir in --edge calls) ;;
    hubs) Q=(map hubs) ;;
  esac
}
# answers <cache dir> <out dir>: every query in text and --json; prints one line per failed command.
answers() {
  local q fmt rc; mkdir -p "$2"
  for q in "${QUERIES[@]}"; do
    query_args "$q"
    for fmt in text json; do
      if [ $fmt = json ]; then Q+=(--json); fi
      (cd "$FX" && DCTX_CACHE_DIR=$1 "$BIN" "${Q[@]}") > "$2/$q.$fmt" 2> "$2/$q.$fmt.err"; rc=$?
      [ $rc = 0 ] || echo "$q $fmt exited $rc: $(head -c 300 "$2/$q.$fmt.err")"
      [ $rc != 0 ] || [ -s "$2/$q.$fmt" ] || echo "$q $fmt printed nothing"
    done
  done
}
# differ <dir a> <dir b>: the answer files whose sha256 differs.
differ() { local f; for f in "$1"/*.text "$1"/*.json; do [ "$(sha "$f")" = "$(sha "$2/${f##*/}")" ] || echo "${f##*/}"; done; }

# ---- determinism: index twice in fresh cache dirs; every answer must be byte-identical.
errs=$(answers "$WORK/cache-a" "$WORK/a"; answers "$WORK/cache-b" "$WORK/b")
sanity=""
grep -q 'util\.py#helper' "$WORK/a/find.text" || sanity="$sanity find lacks util.py#helper;"
grep -q 'app/main\.py#main' "$WORK/a/neighbors.text" || sanity="$sanity neighbors lacks app/main.py#main;"
grep -q 'double' "$WORK/a/skeleton.text" || sanity="$sanity skeleton lacks double;"
grep -q 'helper' "$WORK/a/hubs.text" || sanity="$sanity hubs lacks helper;"
diffs=$(differ "$WORK/a" "$WORK/b")
if [ -n "$errs" ]; then echo "$errs" | sed 's/^/  /'; fail determinism "a query failed on the fixture"
elif [ -n "$sanity" ]; then fail determinism "fixture answers are not real:$sanity"
elif [ -n "$diffs" ]; then fail determinism "two fresh indexes answer differently: $(echo $diffs)"
else
  for f in "$WORK"/a/*.text "$WORK"/a/*.json; do echo "  ${f##*/} $(sha "$f" | cut -c1-16)"; done
  pass determinism
fi

# ---- unrelated edit: append a comment to a file no query names; only hubs may change.
printf '# unrelated edit\n' >> "$FX/app/extra.py"
errs=$(answers "$WORK/cache-a" "$WORK/c")
seen=$(cd "$FX" && DCTX_CACHE_DIR=$WORK/cache-a "$BIN" map grep 'unrelated edit' 2>&1)
changed=$(differ "$WORK/a" "$WORK/c" | grep -v '^hubs\.' || true)
if [ -n "$errs" ]; then echo "$errs" | sed 's/^/  /'; fail unrelated-edit "a query failed after the edit"
elif ! printf '%s' "$seen" | grep -q 'extra\.py'; then fail unrelated-edit "the edit was not re-indexed (map grep did not find it)"
elif [ -n "$changed" ]; then fail unrelated-edit "answers other than hubs changed: $(echo $changed)"
else echo "  changed: $(differ "$WORK/a" "$WORK/c" | tr '\n' ' ')(hubs only is allowed)"; pass unrelated-edit
fi

# ---- network: zero AF_INET/AF_INET6 connect/sendto/sendmsg under strace -f for every command, fresh cache each.
# execve is traced too, so an empty log means the command never ran under the tracer.
NET=(
  "--version" "--help" "map --help"
  "map map lib/util.py" "map find helper" "map neighbors s:lib/util.py#helper --dir in --edge calls"
  "map explain s:lib/util.py#helper" "map read s:lib/util.py#helper" "map skeleton lib/util.py"
  "map grep helper" "map hubs" "map path s:app/main.py#main s:lib/util.py#double" "map export --format json"
  "pack ." "cache status --json"
)
if ! command -v strace >/dev/null 2>&1; then
  skip network "strace is not installed (apt install strace), the zero-network gate did not run"
elif ! strace -f -qq -e trace=execve,%network -o /dev/null true 2>/dev/null; then
  skip network "strace cannot trace here (ptrace not permitted), the zero-network gate did not run"
else
  hits=""; n=0; TO=(); if command -v timeout >/dev/null 2>&1; then TO=(timeout 120); fi
  for c in "${NET[@]}"; do
    n=$((n + 1)); log=$WORK/net-$n.log
    read -r -a words <<< "$c"
    (cd "$FX" && DCTX_CACHE_DIR=$WORK/cache-net-$n ${TO[@]+"${TO[@]}"} strace -f -qq -e trace=execve,%network -o "$log" "$BIN" "${words[@]}" > /dev/null 2>&1)
    rc=$?
    if [ ! -s "$log" ]; then echo "  $c: no strace log (rc $rc)"; hits="$hits [$c: not traced]"; continue; fi
    count=$(grep -E '(connect|sendto|sendmsg)\(' "$log" | grep -c 'AF_INET')
    echo "  $c: $count inet connect/send (rc $rc)"
    [ "$count" = 0 ] || hits="$hits [$c: $count]"
  done
  if [ -z "$hits" ]; then pass network; else fail network "network syscalls:$hits"; fi
fi
finish
