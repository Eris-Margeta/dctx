#!/bin/sh
# DX-A2 model check.
# Every <Name>.cfg next to CacheLifecycle.tla must pass TLC with no error.
# expect-fail/*.cfg must each violate the property named on its EXPECT line.
# A TLC_TIMEOUT (default 300s, applied by tlc.sh) is inconclusive and fails.
# That limit is not P8: progress is weak fairness inside CacheLifecycle.tla.
# check.sh [Name ...] checks only those pass configs; mutants still all run.
set -u
here=$(cd "$(dirname "$0")" && pwd)
status=0
checked=0
for cfg in "$here"/*.cfg; do
  [ -f "$cfg" ] || continue
  if [ $# -gt 0 ]; then
    name=$(basename "$cfg" .cfg)
    want=0
    for n in "$@"; do
      [ "$n" = "$name" ] && want=1
    done
    [ "$want" -eq 1 ] || continue
  fi
  checked=$((checked + 1))
  spec="${cfg%.cfg}.tla"
  if [ ! -f "$spec" ]; then
    spec="$here/CacheLifecycle.tla"
  fi
  [ -f "$spec" ] || { echo "MISSING spec for ${cfg#$here/}"; status=1; continue; }
  start=$(date +%s)
  out=$(sh "$here/tlc.sh" "$spec" "$cfg")
  code=$?
  secs=$(( $(date +%s) - start ))
  if [ "$code" -eq 124 ]; then
    echo "INCONCLUSIVE ${cfg#$here/} (timeout after ${secs}s)"
    status=1
  elif printf '%s\n' "$out" | grep -q "Model checking completed. No error has been found."; then
    states=$(printf '%s\n' "$out" | grep -E 'distinct states found' | tail -1 | sed 's/.*generated, //')
    printf 'OK   %s  %ss  %s\n' "${cfg#$here/}" "$secs" "$states"
  else
    printf 'FAIL %s\n%s\n' "${cfg#$here/}" "$(printf '%s\n' "$out" | tail -30)"
    status=1
  fi
done
[ "$checked" -gt 0 ] || { echo "NO CONFIG matched: $*"; status=1; }
sh "$here/expect-fail.sh" || status=1
exit "$status"
