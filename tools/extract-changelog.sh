#!/bin/sh
# Usage: tools/extract-changelog.sh [version|Unreleased]
# With no argument, extract the latest numbered release (skip Unreleased).
set -eu

if [ "$#" -gt 1 ]; then
  printf '%s\n' 'Usage: extract-changelog.sh [version|Unreleased]' >&2
  exit 2
fi

exec python3 - "$0" "$@" <<'PY'
import os
from pathlib import Path
import re
import sys

changelog = Path(sys.argv[1]).parent.parent / "CHANGELOG.md"
version = os.fsencode(sys.argv[2]) if len(sys.argv) == 3 else None
if version is not None and version.startswith(b"v"):
    version = version[1:]

# Only the bracketed label identifies the release. Links and dates may follow.
heading = re.compile(rb"## \[([0-9][^\]\r\n]*|Unreleased)\]")
selected = False
body = None
try:
    with changelog.open("rb") as lines:
        for line in lines:
            if line.startswith(b"## "):
                selected = False
                match = heading.match(line)
                if match is None:
                    continue
                label = match[1]
                if version is None and label != b"Unreleased":
                    version = label
                if label == version:
                    if body is not None:
                        sys.exit("Error: duplicate release heading in CHANGELOG.md")
                    body = bytearray()
                    selected = True
            elif selected:
                body.extend(line)
except OSError:
    sys.exit("Error: could not read root CHANGELOG.md")

if body is None:
    sys.exit("Error: requested version not found in CHANGELOG.md")
if version != b"Unreleased" and not body.decode("utf-8", "surrogateescape").strip():
    sys.exit("Error: numbered release has no notes in CHANGELOG.md")
sys.stdout.buffer.write(body)
PY
