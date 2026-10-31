#!/bin/sh
# context.sh CARD [PLACE] [BUDGET]: prints a worker card followed by its dctx task context (v1 card 9).
# The context is `dctx pack PLACE --for "<card text>" --budget BUDGET`: map find on the names the card mentions,
# then neighbors and read of the best hits, cut to BUDGET estimated tokens (ceil(2 * UTF-8 bytes / 7)).
# PLACE defaults to the current directory, BUDGET to 3000. DCTX names the binary (default: dctx on PATH).
# Exit: 0 with context; a card whose names match nothing still prints the card and the empty context (dctx exit 1 is
# tolerated); any other dctx failure (2 usage, 3 index) is this script's exit code.
set -eu
card=${1:?usage: context.sh CARD [PLACE] [BUDGET]}
place=${2:-.}
budget=${3:-3000}
dctx=${DCTX:-dctx}
text=$(cat "$card")
printf '%s\n\n## dctx context (dctx pack --for, budget %s estimated tokens)\n\n' "$text" "$budget"
rc=0
"$dctx" pack "$place" --for "$text" --budget "$budget" || rc=$?
[ "$rc" -eq 0 ] || [ "$rc" -eq 1 ] || exit "$rc"
