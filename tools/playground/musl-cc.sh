#!/bin/sh
# A musl C compiler for cc-rs built from zig, for hosts without musl-gcc:
#   CC_x86_64_unknown_linux_musl=tools/playground/musl-cc.sh
# cc-rs passes a clang-style --target that zig spells differently, so it is dropped.
for a in "$@"; do shift; case "$a" in --target=*) ;; *) set -- "$@" "$a";; esac; done
exec zig cc -target x86_64-linux-musl "$@"
