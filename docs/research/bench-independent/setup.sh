#!/bin/sh
# Network step of the independent benchmark (v1 card 6), idempotent. usage: ./setup.sh <selection.json>
# Clones the selected repos at their resolved commits (full history without blobs, for the fix-commit questions),
# installs the pinned language servers and tokenizer, fetches the repos' dependencies for the servers, and installs
# Trail-Code and graphify through the 3-way harness's setup_tools.sh. Everything after this step runs offline.
set -eu
SEL=$(readlink -f "${1:?selection.json}")
HERE=$(cd "$(dirname "$0")" && pwd)
B=${B3_ROOT:?B3_ROOT}
CORP=$B/corpora-indep LSP=$B/lsp VENV=$B/venv-indep
export TIKTOKEN_CACHE_DIR=$B/tiktoken-cache GOPATH=$B/gopath GOMODCACHE=$B/gopath/pkg/mod GIT_TERMINAL_PROMPT=0
mkdir -p "$CORP" "$LSP/bin" "$TIKTOKEN_CACHE_DIR" "$GOPATH" "$B/home"
pin() { python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1]))["pins"]; print(d[sys.argv[2]][sys.argv[3]])' "$SEL" "$1" "$2"; }

# 1. repos at the commit select.sh resolved; a different HEAD is an error.
python3 -I -c 'import json,sys; [print(s["name"], s["url"], s["sha"] or "", s["lang"]) for s in json.load(open(sys.argv[1]))["selection"]]' "$SEL" |
while read -r name url sha lang; do
  [ -n "$sha" ] || { echo "setup: $name has no resolved commit; run select.sh without --no-resolve" >&2; exit 2; }
  d=$CORP/$name
  if [ ! -d "$d/.git" ] || [ "$(git -C "$d" rev-parse HEAD)" != "$sha" ]; then
    rm -rf "$d"; git clone -q --filter=blob:none --no-checkout "$url" "$d"; git -C "$d" checkout -q "$sha"
  fi
  [ "$(git -C "$d" rev-parse HEAD)" = "$sha" ] || { echo "setup: $name HEAD is not $sha" >&2; exit 2; }
  git -C "$d" fetch -q --tags origin || true
  case $lang in
    rust) (cd "$d" && cargo fetch --locked 2>/dev/null || cargo fetch) ;;
    go) (cd "$d" && GOTOOLCHAIN=local go mod download) ;;
    python) : ;;
  esac
  echo "repo $name $sha"
done

# 2. language servers, pinned. rust-analyzer's sha256 is recorded on first download and checked afterwards.
RA=$(pin rust-analyzer release); RAA=$(pin rust-analyzer asset)
if [ ! -x "$LSP/bin/rust-analyzer" ]; then
  curl -fsSL "https://github.com/rust-lang/rust-analyzer/releases/download/$RA/$RAA" | gunzip > "$LSP/bin/rust-analyzer"
  chmod +x "$LSP/bin/rust-analyzer"; sha256sum "$LSP/bin/rust-analyzer" | cut -d' ' -f1 > "$LSP/rust-analyzer.sha256"
fi
[ "$(sha256sum "$LSP/bin/rust-analyzer" | cut -d' ' -f1)" = "$(cat "$LSP/rust-analyzer.sha256")" ] || { echo "setup: rust-analyzer changed" >&2; exit 2; }
PV=$(pin pyright version)
[ "$("$LSP/node_modules/.bin/pyright" --version 2>/dev/null || true)" = "pyright $PV" ] ||
  HOME=$B/home npm install --prefix "$LSP" --no-audit --no-fund --save-exact "pyright@$PV"
GV=$(pin gopls version)
"$LSP/bin/gopls" version 2>/dev/null | grep -q "$GV" || GOBIN=$LSP/bin go install "$(pin gopls module)@$GV"

# 3. one tokenizer for every tool: tiktoken + the cl100k BPE file, hash-checked by tokens.Counter.
[ -x "$VENV/bin/python" ] || python3 -m venv "$VENV"
TV=$(pin tokenizer version)
"$VENV/bin/python" -c "import importlib.metadata as m; assert m.version('tiktoken') == '$TV'" 2>/dev/null ||
  "$VENV/bin/pip" install --no-input -q "tiktoken==$TV"
"$VENV/bin/python" -c "import tiktoken; tiktoken.get_encoding('$(pin tokenizer name)')"
"$VENV/bin/python" -I -c 'import json,sys; sys.path.insert(0, sys.argv[2]); import tokens; tokens.Counter(json.load(open(sys.argv[1]))["pins"]["tokenizer"]); print("tokenizer ok")' "$SEL" "$HERE"

# 4. the two comparison tools at the 3-way versions.
B3_ROOT=$B sh "$HERE/../bench-3way-20261030/scripts/setup_tools.sh"
echo "SETUP OK"
