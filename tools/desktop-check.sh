#!/bin/sh
# // What: check the dctx-desktop crate from the repo root without a built frontend.
# // Why: Tauri's build script requires frontendDist ../dist (apps/dctx-desktop/dist), which is gitignored and not built, so cargo check stops before any Rust error.
# // Spec: DX-E4cb. Depends on cargo, and on Linux on pkg-config webkit2gtk-4.1. The desktop check gate runs this.
# // Lock: allow path dependency resolution during the check; restore Cargo.lock on exit. CI refreshes the committed lock after landing.
set -eu

# This file lives in tools/. The check must run at the repo root.
script=$0
case $script in
  */*) ;;
  *) script=./$script ;;
esac
script_dir=$(CDPATH= cd -- "$(dirname -- "$script")" && pwd)
cd -- "$script_dir/.."
root=$(pwd)

# Tauri on Linux links webkit2gtk. Without the 4.1 pkg-config module the
# check cannot pass, so skip instead of failing the gate.
if [ "$(uname)" = "Linux" ]; then
  if ! command -v pkg-config >/dev/null 2>&1 || ! pkg-config --exists webkit2gtk-4.1; then
    printf '%s\n' 'SKIP: no webkit2gtk'
    exit 0
  fi
fi

dist=$root/apps/dctx-desktop/dist
created=0
lock_backup=
lock_saved=0

cleanup() {
  rc=$?
  trap - EXIT
  if [ "$lock_saved" = 1 ]; then
    if cp -- "$lock_backup" "$root/Cargo.lock"; then
      rm -f -- "$lock_backup"
    else
      printf 'Failed to restore Cargo.lock; backup: %s\n' "$lock_backup" >&2
      rc=1
    fi
  elif [ -n "$lock_backup" ]; then
    rm -f -- "$lock_backup"
  fi
  if [ "$created" = 1 ]; then
    case $dist in
      */apps/dctx-desktop/dist)
        rm -rf -- "$dist" || true
        ;;
    esac
  fi
  exit "$rc"
}
trap cleanup EXIT

# The packet may add path dependencies, but only CI owns the committed lock.
lock_backup=$(mktemp "${TMPDIR:-/tmp}/dctx-desktop-check-lock.XXXXXX")
cp -- "$root/Cargo.lock" "$lock_backup"
lock_saved=1

# Only a dist this script created is removed. An existing frontend build stays.
if [ ! -e "$dist" ]; then
  mkdir -- "$dist"
  created=1
  printf '%s\n' '<!DOCTYPE html><title>dctx desktop check placeholder</title>' >"$dist/index.html"
fi

# Keep the worktree free of a target tree. Cargo may still fetch into its own cache.
CARGO_TARGET_DIR="${TMPDIR:-/tmp}/dctx-desktop-check-target"
export CARGO_TARGET_DIR

cargo check -p dctx-desktop
