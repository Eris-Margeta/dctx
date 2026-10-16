#!/bin/sh
# TLC gate for PL-M / PLUGINS section 7.1: baselines, mutants and reachability.
# Uses the VM's Java/TLC installation.
# Keeps source hashes, complete counterexamples and metadata inside this directory.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$here"

fail() { printf 'FAIL %s\n' "$*" >&2; exit 1; }
[ "$#" -eq 0 ] || fail 'check.sh takes no arguments; it runs the complete gate'
command -v java >/dev/null || fail 'Java is required on the verification VM'
command -v timeout >/dev/null || fail 'GNU timeout is required on the verification VM'
command -v sha256sum >/dev/null || fail 'sha256sum is required on the verification VM'

jar=${TLA2TOOLS_JAR:-}
if [ -z "$jar" ]; then
  for candidate in /opt/tla2tools.jar /opt/tla/tla2tools.jar \
    /opt/tlaplus/tla2tools.jar /usr/local/lib/tla2tools.jar \
    /usr/local/share/tla2tools.jar /usr/local/share/tlaplus/tla2tools.jar \
    /usr/share/java/tla2tools.jar \
    /opt/tla/tla2tools.jar; do
    if [ -f "$candidate" ]; then jar=$candidate; break; fi
  done
fi
[ -n "$jar" ] && [ -f "$jar" ] || fail 'set TLA2TOOLS_JAR to the VM tla2tools.jar (no download is performed)'
case "$jar" in /*) ;; *) fail 'TLA2TOOLS_JAR must be an absolute path' ;; esac
seconds=${TLC_TIMEOUT:-300}
workers=${TLC_WORKERS:-2}
case "$seconds:$workers" in *[!0-9:]*|:*|*:) fail 'timeout/workers must be positive integers' ;; esac
[ "$seconds" -gt 0 ] && [ "$workers" -gt 0 ] || fail 'timeout/workers must be positive'

mkdir -p .runs
run=$(mktemp -d "$here/.runs/run.XXXXXX")
mkdir "$run/meta"
mkdir "$run/tmp"
printf 'EVIDENCE %s\n' "$run"
sha256sum ./*.tla ./*.cfg ./mutations/*.cfg ./witnesses/*.cfg \
  ./suite.tsv ./check.sh "$jar" > "$run/sha256.txt"
java -version > "$run/java.txt" 2>&1
printf 'timeout=%s\nworkers=%s\njar=%s\n' "$seconds" "$workers" "$jar" > "$run/settings.txt"
printf 'config\toutcome\tproperty\texit\tseconds\n' > "$run/results.tsv"

# The manifest is closed: accidentally omitted or duplicate configs fail the gate.
expected=$(awk '!/^#/ && NF {n++} END {print n+0}' suite.tsv)
actual=0
for cfg in ./*.cfg ./mutations/*.cfg ./witnesses/*.cfg; do
  [ -f "$cfg" ] || fail "missing configuration: $cfg"
  actual=$((actual + 1))
done
[ "$expected" -eq "$actual" ] || fail "suite lists $expected configs, tree has $actual"
awk '!/^#/ && NF {if (NF != 4 || seen[$1]++) exit 1}' suite.tsv || fail 'invalid/duplicate suite row'

passed=0
baselines=0
mutants=0
witnesses=0
while read -r cfg module outcome property extra; do
  case "$cfg" in ''|'#'*) continue ;; esac
  [ -z "${extra:-}" ] || fail "extra suite fields: $cfg"
  case "$cfg" in /*|*..*) fail "unsafe suite path: $cfg" ;; esac
  [ -f "$cfg" ] && [ -f "$module.tla" ] || fail "missing input: $cfg / $module"
  name=$(printf '%s' "$cfg" | tr '/' '_')
  log="$run/$name.log"
  mkdir "$run/meta/$name"
  start=$(date +%s)
  code=0
  timeout --kill-after=5 "$seconds" java -Xmx1536m -XX:+UseParallelGC \
    -XX:-UsePerfData -Djava.io.tmpdir="$run/tmp" \
    -cp "$jar" tlc2.TLC -workers "$workers" \
    -metadir "$run/meta/$name" -config "$cfg" "$module.tla" > "$log" 2>&1 || code=$?
  elapsed=$(( $(date +%s) - start ))
  good=false
  case "$outcome" in
    pass)
      if [ "$code" -eq 0 ] && grep -Fq 'Model checking completed. No error has been found.' "$log"; then
        good=true
      fi
      baselines=$((baselines + 1))
      ;;
    mutant|witness)
      if [ "$code" -eq 12 ] && grep -Fq "Invariant $property is violated." "$log" \
          && grep -Eq '^State 2:' "$log"; then
        good=true
      fi
      if [ "$outcome" = mutant ]; then mutants=$((mutants + 1)); else witnesses=$((witnesses + 1)); fi
      ;;
    temporal)
      if [ "$code" -eq 13 ] && grep -Fq 'Temporal properties were violated.' "$log" \
          && grep -Fq 'PROPERTY PP8Progress' "$cfg" \
          && grep -Eq 'Back to state|Stuttering' "$log"; then
        good=true
      fi
      mutants=$((mutants + 1))
      ;;
    *) fail "unknown expected outcome: $outcome" ;;
  esac
  printf '%s\t%s\t%s\t%s\t%s\n' "$cfg" "$outcome" "$property" "$code" "$elapsed" >> "$run/results.tsv"
  if [ "$good" != true ]; then
    cat "$log"
    case "$code" in 124|137) fail "$cfg inconclusive: TLC timeout/resource kill" ;; esac
    fail "$cfg expected $outcome $property; TLC exit $code (evidence $run)"
  fi
  passed=$((passed + 1))
  printf 'OK %s %s %s (%ss)\n' "$outcome" "$cfg" "$property" "$elapsed"
  if [ "$outcome" = pass ]; then
    grep -E 'distinct states found|depth of the complete state graph' "$log"
  fi
done < suite.tsv
[ "$passed" -eq "$expected" ] || fail "only $passed of $expected configurations ran"
printf 'PASS PP1-PP10: %s baselines, %s mutants, %s reachability witnesses; %s total\n' \
  "$baselines" "$mutants" "$witnesses" "$passed"
printf 'Complete TLC logs/counterexamples and source hashes: %s\n' "$run"
