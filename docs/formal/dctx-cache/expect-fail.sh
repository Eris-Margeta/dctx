#!/bin/sh
# expect-fail.sh [dir ...]
# Each cfg must fail on the invariant or property named by "\* EXPECT: <Name>".
# The spec is the cfg's sibling .tla, or the file named by "\* SPEC: <relative>".
# A timeout, parse error, deadlock, or a different violation is not a kill.
set -u
sole_property() {
  awk '
    function token(t) {
      if (t == "PROPERTY" || t == "PROPERTIES") { in_properties = 1; return }
      if (t ~ /^(CONSTANT|CONSTANTS|CONSTRAINT|CONSTRAINTS|ACTION_CONSTRAINT|ACTION_CONSTRAINTS|INIT|NEXT|SPECIFICATION|INVARIANT|INVARIANTS|SYMMETRY|VIEW|ALIAS|POSTCONDITION|CHECK_DEADLOCK|CHECK_DEADLOCK_TRACE|RL_REWARD|_PERIODIC)$/) {
        in_properties = 0; return
      }
      if (in_properties) { count++; property = t }
    }
    function flush() { if (word != "") token(word); word = "" }
    {
      for (i = 1; i <= length($0); i++) {
        ch = substr($0, i, 1); pair = substr($0, i, 2)
        if (depth) {
          if (pair == "(*") { depth++; i++ }
          else if (pair == "*)") { depth--; i++ }
        } else if (quoted) {
          if (ch == "\\") i++
          else if (ch == "\"") { quoted = 0; token("<string>") }
        } else if (pair == "\\*") { flush(); break }
        else if (pair == "(*") { flush(); depth++; i++ }
        else if (ch == "\"") { flush(); quoted = 1 }
        else if (ch ~ /[[:space:]]/) flush()
        else word = word ch
      }
      flush()
    }
    END { if (!depth && !quoted && count == 1) print property }
  ' "$1"
}
here=$(cd "$(dirname "$0")" && pwd)
[ $# -gt 0 ] || set -- "$here/expect-fail"
status=0
n=0
for dir in "$@"; do
  for cfg in $(find "$dir" -name '*.cfg' | sort); do
    n=$((n + 1))
    want=$(sed -n 's/^\\\* EXPECT: *\([A-Za-z0-9_]*\).*/\1/p' "$cfg" | head -1)
    rel=$(sed -n 's/^\\\* SPEC: *\(.*\)$/\1/p' "$cfg" | head -1)
    if [ -n "$rel" ]; then
      spec="$(cd "$(dirname "$cfg")" && cd "$(dirname "$rel")" && pwd)/$(basename "$rel")"
    else
      spec="${cfg%.cfg}.tla"
    fi
    [ -n "$want" ] || { echo "NOEXPECT $cfg"; status=1; continue; }
    [ -f "$spec" ] || { echo "MISSING spec $spec for $cfg"; status=1; continue; }
    out=$(sh "$here/tlc.sh" "$spec" "$cfg")
    code=$?
    if [ "$code" -eq 124 ]; then
      echo "INCONCLUSIVE $cfg (timeout)"
      status=1
    elif printf '%s\n' "$out" | grep -Eq "(Invariant|Temporal property|Action property|Property) $want is violated"; then
      echo "KILLED $want  ${cfg#$here/}"
    elif [ "$code" -eq 13 ] && [ "$(sole_property "$cfg")" = "$want" ] &&
         ! printf '%s\n' "$out" | grep -Eiq '(Invariant .* (is violated|is false)|Deadlock|(Temporal property|Action property|Property) .* is violated)'; then
      echo "KILLED $want  ${cfg#$here/} (sole temporal property, TLC exit 13)"
    else
      echo "SURVIVED $cfg (expected $want)"
      printf '%s\n' "$out" | grep -E "violated|Error|error" | head -8
      status=1
    fi
  done
done
[ "$n" -gt 0 ] || { echo "no expect-fail configs found"; status=1; }
exit "$status"
