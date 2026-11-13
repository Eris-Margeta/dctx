#!/bin/sh
# End-to-end smoke test of a running playground (local binary or container) with the real dctx.
#   tools/playground/smoke.sh http://127.0.0.1:8080
# Needs curl and python3. Checks: health, samples, the recorded `map find greet` bytes, pasted
# files, the no-JS form, 24 refusals (400), oversize (413), the rate
# limit (429), and that `.env` never reaches an answer.
set -eu
base=${1:-http://127.0.0.1:8080}
here=$(cd "$(dirname "$0")" && pwd)
demo="$here/../website/data/demo.json"
pass=0
fail=0
ok() { pass=$((pass + 1)); echo "ok   $*"; }
bad() { fail=$((fail + 1)); echo "FAIL $*"; }
post() { curl -s -o /dev/null -w '%{http_code}' -H 'Content-Type: application/json' --data-binary "$2" "$base$1"; }

health=$(curl -sf "$base/api/v1/health") && ok "health: $health" || bad "health"
case "$health" in *'"ok":true,"dctx":"1.1.0"'*) ok "health reports dctx 1.1.0" ;; *) bad "health version" ;; esac
samples=$(curl -sf "$base/api/v1/samples") && ok "samples: $samples" || bad "samples"

# The recorded example and the live run must be the same bytes.
tmp=$(mktemp)
curl -sf -H 'Content-Type: application/json' --data-binary '{"cmd":["map","find","greet"],"sample":"kiosk"}' "$base/api/v1/run" > "$tmp"
python3 - "$demo" "$tmp" <<'PY' && ok "map find greet = recorded example (find)" || bad "map find greet differs from demo.json"
import json, sys
record = next(r for r in json.load(open(sys.argv[1]))["records"] if r["id"] == "find")
live = json.load(open(sys.argv[2]))
sys.exit(0 if live["stdout"] == record["stdout"] and live["exit"] == record["exit"] and live["bytes"] == record["bytes"] else 1)
PY
for id in find-json read-json pack-for grep; do
  python3 - "$demo" "$base" "$id" <<'PY' && ok "$id = recorded example" || bad "$id differs from demo.json"
import json, sys, urllib.request
record = next(r for r in json.load(open(sys.argv[1]))["records"] if r["id"] == sys.argv[3])
body = json.dumps({"cmd": record["argv"][1:], "sample": "kiosk"}).encode()
req = urllib.request.Request(sys.argv[2] + "/api/v1/run", body, {"Content-Type": "application/json", "X-Forwarded-For": "198.51.100." + str(len(sys.argv[3]))})
live = json.load(urllib.request.urlopen(req))
sys.exit(0 if live["stdout"] == record["stdout"] and live["exit"] == record["exit"] else 1)
PY
done

if [ "${SMOKE_ALL:-0}" = 1 ]; then
  # Every recorded example (not the three with scrubbed values, not help pages), one address each:
  # only for a server that trusts X-Forwarded-For directly (local runs), since it sends many runs.
  python3 - "$demo" "$base" <<'PY' && ok "every recorded example: same bytes live" || bad "a recorded example differs live"
import json, sys, urllib.request
records = [r for r in json.load(open(sys.argv[1]))["records"]
           if r["group"] in ("pack", "map", "json", "graph", "errors") and "scrubbed" not in r]
bad = []
for i, r in enumerate(records):
    body = json.dumps({"cmd": r["argv"][1:], "sample": "kiosk"}).encode()
    req = urllib.request.Request(sys.argv[2] + "/api/v1/run", body, {"Content-Type": "application/json", "X-Forwarded-For": f"203.0.113.{i + 1}"})
    try:
        live = json.load(urllib.request.urlopen(req))
    except OSError as error:
        bad.append(f"{r['id']} ({error})")
        continue
    if (live["stdout"], live["stderr"], live["exit"]) != (r["stdout"], r["stderr"], r["exit"]):
        bad.append(r["id"])
print(f"{len(records) - len(bad)} of {len(records)} identical", "differ: " + ", ".join(bad) if bad else "")
sys.exit(1 if bad else 0)
PY
fi

files=$(curl -sf -H 'Content-Type: application/json' -H 'X-Forwarded-For: 198.51.100.20' --data-binary '{"cmd":["map","skeleton","src/a.rs"],"files":[{"path":"src/a.rs","content":"pub fn alpha() {}\nfn beta() { alpha() }\n"}]}' "$base/api/v1/run")
case "$files" in *'s:src/a.rs#alpha'*'"exit":0'*) ok "pasted files: skeleton" ;; *) bad "pasted files: $files" ;; esac

form=$(curl -s -H 'X-Forwarded-For: 198.51.100.22' --data-urlencode 'cmd=map find greet' --data 'source=sample&sample=kiosk' "$base/api/v1/run-form")
case "$form" in *'<h1>dctx map find greet</h1>'*'s:src/util.rs#greet'*) ok "form fallback (no JavaScript)" ;; *) bad "form: $form" ;; esac

grep_env=$(curl -sf -H 'Content-Type: application/json' -H 'X-Forwarded-For: 198.51.100.23' --data-binary '{"cmd":["map","grep","not-a-secret"],"sample":"kiosk"}' "$base/api/v1/run")
case "$grep_env" in *example-not-a-secret*) bad ".env content reached an answer" ;; *) ok ".env value never in an answer" ;; esac

refused=0
for cmd in '["cache","status"]' '["cache","purge","--all","--yes"]' '["template","build","x"]' '["plugin","list"]' '["git","status"]' \
  '["."]' '[".","-d","-c"]' '["pack","/etc"]' '["pack",".","--output","o.txt"]' '["pack",".","--clipboard"]' \
  '["map","find","greet","--place","/"]' '["map","read","/etc/passwd"]' '["map","read","m:../x"]' '["map","read","s:../../x"]' \
  '["map","neighbors","s:a","--depth","9"]' '["map","map","a","--depth","4"]' '["map","find","x","--limit","41"]' \
  '["map","export","--output","g.mmd"]' '["map","read","s:a","--rev","HEAD"]' '["map","skeleton","a.rs","--profile"]' \
  '["map","hubs","--limit","61"]' '["languages","--yaml"]' '["sh","-c","id"]' '[]'; do
  code=$(post /api/v1/run "{\"cmd\":$cmd}")
  if [ "$code" = 400 ]; then refused=$((refused + 1)); else bad "refusal $cmd gave $code"; fi
done
[ "$refused" = 25 ] && ok "25 of 25 bad commands refused with 400" || bad "only $refused of 25 refused"

big=$(python3 -c 'print("{\"cmd\":[\"languages\"],\"pad\":\"" + "x" * 600000 + "\"}")')
code=$(printf '%s' "$big" | curl -s -o /dev/null -w '%{http_code}' -H 'Content-Type: application/json' --data-binary @- "$base/api/v1/run")
[ "$code" = 413 ] && ok "oversize body: 413" || bad "oversize body gave $code"

limited=0
for i in 1 2 3 4 5 6 7 8; do
  code=$(post /api/v1/run "{\"cmd\":[\"map\",\"find\",\"rate$i\"]}")
  [ "$code" = 429 ] && limited=$((limited + 1))
done
[ "$limited" -gt 0 ] && ok "rate limit: $limited of 8 rapid runs got 429" || bad "rate limit never answered 429"

metrics=$(curl -sf "$base/api/v1/metrics.json") && ok "metrics: $metrics" || bad "metrics"
rm -f "$tmp"
echo "smoke: $pass passed, $fail failed"
[ "$fail" = 0 ]
