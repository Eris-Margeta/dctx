#!/usr/bin/env bash
# spdx_check.sh [root]: licence gate for the workspace at <root> (default: the repository root).
# Fails unless every workspace member's Cargo.toml declares `license = "Apache-2.0"` in [package], or inherits it with
# `license.workspace = true` (or `license = { workspace = true }`) while [workspace.package] sets `license = "Apache-2.0"`.
# Also fails unless LICENSE (the Apache License, Version 2.0 text) and a non-empty NOTICE exist at the root.
# Desktop application TS/JS/CSS/HTML sources must carry Apache-2.0 and Eris Margeta headers.
# Test trees, generated output and third-party dependencies are outside this source-header check.
# Members come from [workspace] members/exclude of the root Cargo.toml; a root [package] is checked as well. No cargo needed.
set -eu
root=${1:-"$(cd "$(dirname "$0")/.." && pwd)"}
cd "$root"
problems=0
bad() { echo "spdx_check: $*" >&2; problems=$((problems + 1)); }

# toml_array <file> <section> <key>: the quoted strings of a (possibly multi-line) string array; comments are dropped.
toml_array() {
  awk -v sec="[$2]" -v key="$3" '
    { line = $0; sub(/#.*/, "", line) }
    !inarr && line ~ /^[[:space:]]*\[/ { cur = line; gsub(/[[:space:]]/, "", cur); next }
    !inarr && cur == sec && line ~ ("^[[:space:]]*" key "[[:space:]]*=") { inarr = 1; sub(/^[^=]*=/, "", line) }
    inarr { s = line; while (match(s, /"[^"]*"/)) { print substr(s, RSTART + 1, RLENGTH - 2); s = substr(s, RSTART + RLENGTH) }; if (line ~ /\]/) inarr = 0 }
  ' "$1"
}

# license_kind <file> <section>: "own" for license = "Apache-2.0", "other" for another literal licence,
# "workspace" for an inherited licence, "none" when the section has no licence key.
license_kind() {
  awk -v sec="[$2]" '
    { line = $0; sub(/#.*/, "", line) }
    line ~ /^[[:space:]]*\[/ { cur = line; gsub(/[[:space:]]/, "", cur); next }
    cur != sec { next }
    line ~ /^[[:space:]]*license[[:space:]]*=[[:space:]]*"Apache-2\.0"[[:space:]]*$/ { own = 1; next }
    line ~ /^[[:space:]]*license\.workspace[[:space:]]*=[[:space:]]*true[[:space:]]*$/ { ws = 1; next }
    line ~ /^[[:space:]]*license[[:space:]]*=[[:space:]]*\{[[:space:]]*workspace[[:space:]]*=[[:space:]]*true[[:space:]]*\}[[:space:]]*$/ { ws = 1; next }
    line ~ /^[[:space:]]*license[[:space:]]*=/ { other = 1 }
    END { if (other) print "other"; else if (own) print "own"; else if (ws) print "workspace"; else print "none" }
  ' "$1"
}

[ -f Cargo.toml ] || { echo "spdx_check: no Cargo.toml in $root" >&2; exit 2; }
ws_license=$(license_kind Cargo.toml workspace.package)
excludes=$(toml_array Cargo.toml workspace exclude)
manifests=()
if grep -q '^[[:space:]]*\[package\][[:space:]]*$' Cargo.toml; then manifests+=("Cargo.toml"); fi
while IFS= read -r pattern; do
  [ -n "$pattern" ] || continue
  matched=0
  for dir in $pattern; do
    [ -d "$dir" ] || continue
    skip=0
    while IFS= read -r ex; do [ -n "$ex" ] && [ "${dir%/}" = "${ex%/}" ] && skip=1; done <<< "$excludes"
    [ "$skip" = 1 ] && continue
    matched=1
    if [ -f "$dir/Cargo.toml" ]; then manifests+=("$dir/Cargo.toml"); else bad "member $dir has no Cargo.toml"; fi
  done
  [ "$matched" = 1 ] || bad "workspace member pattern '$pattern' matches no directory"
done <<< "$(toml_array Cargo.toml workspace members)"

[ "${#manifests[@]}" -gt 0 ] || bad "no crates found in $root/Cargo.toml"
for manifest in ${manifests[@]+"${manifests[@]}"}; do
  case $(license_kind "$manifest" package) in
    own) ;;
    workspace) [ "$ws_license" = own ] || bad "$manifest inherits the workspace licence, but [workspace.package] does not set license = \"Apache-2.0\"" ;;
    other) bad "$manifest: license is not \"Apache-2.0\"" ;;
    *) bad "$manifest: no license field in [package]" ;;
  esac
done

if [ ! -s LICENSE ]; then bad "LICENSE missing or empty"
elif ! grep -q 'Apache License' LICENSE || ! grep -q 'Version 2\.0' LICENSE; then bad "LICENSE is not the Apache License, Version 2.0"; fi
[ -s NOTICE ] || bad "NOTICE missing or empty"

# Restrict traversal to first-party application sources. Keep HTML's doctype first;
# both SPDX fields must appear in the first eight lines in the language's comment form.
desktop=apps/dctx-desktop
sources=()
if [ -d "$desktop/src" ]; then
  while IFS= read -r -d '' source; do sources+=("$source"); done < <(
    find "$desktop/src" -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.mjs' -o -name '*.cjs' -o -name '*.css' -o -name '*.html' \) -print0
  )
fi
for source in "$desktop/index.html" "$desktop/vite.config.ts"; do
  [ ! -f "$source" ] || sources+=("$source")
done
for source in ${sources[@]+"${sources[@]}"}; do
  case "$source" in
    *.html) prefix='<!-- '; suffix=' -->' ;;
    *.css) prefix='[[:space:]]*(/\*|\*) '; suffix='([[:space:]]*\*/)?' ;;
    *) prefix='// '; suffix='' ;;
  esac
  header=$(head -n 8 "$source")
  if ! grep -Eq "^${prefix}SPDX-License-Identifier: Apache-2\.0${suffix}$" <<< "$header"; then
    bad "$source: missing Apache-2.0 SPDX source header"
  fi
  if ! grep -Eq "^${prefix}SPDX-FileCopyrightText: [0-9]{4} Eris Margeta${suffix}$" <<< "$header"; then
    bad "$source: missing Eris Margeta copyright header"
  fi
done

[ "$problems" = 0 ] || { echo "spdx_check FAIL: $problems problem(s)" >&2; exit 1; }
echo "spdx_check ok: ${#manifests[@]} crates, ${#sources[@]} desktop sources, LICENSE and NOTICE present"
