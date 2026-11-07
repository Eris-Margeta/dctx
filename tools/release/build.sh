#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# SPDX-FileCopyrightText: 2026 Eris Margeta
# Run from the checkout root. The same recipe is used by release and reproducibility jobs.
set -euo pipefail
umask 022
export PATH="$HOME/.cargo/bin:$PATH"
export RUSTC_WRAPPER=
source tools/release/toolchain.env
target=${1:?usage: build.sh TARGET [OUTPUT_DIRECTORY]}
output=${2:-dist}
export SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
export DCTX_GIT_SHA=${DCTX_GIT_SHA:-$(git rev-parse HEAD)}
export CARGO_INCREMENTAL=0
export TZ=UTC LC_ALL=C
# rustc paths use native separators on Windows. pwd -W supplies the corresponding prefix.
source_root=$PWD
if [[ "$target" == *-windows-* ]]; then source_root=$(pwd -W); fi
cargo_root=${CARGO_HOME:-$HOME/.cargo}
if [[ "$target" == *-windows-* ]]; then cargo_root=$(cygpath -m "$cargo_root"); fi
build_root=${CARGO_TARGET_DIR:-$PWD/target}
if [[ "$target" == *-windows-* ]]; then
  build_root=$(cygpath -am "$build_root")
elif [[ "$build_root" != /* ]]; then
  build_root="$PWD/$build_root"
fi
export RUSTFLAGS="--remap-path-prefix=$source_root=/dctx --remap-path-prefix=$cargo_root=/cargo --remap-path-prefix=$build_root=/target -C debuginfo=0"
if [[ "$target" == *-windows-* ]]; then
  export RUSTFLAGS="$RUSTFLAGS -C link-arg=/Brepro"
  export CFLAGS="${CFLAGS:-} /pathmap:$source_root=/dctx /pathmap:$cargo_root=/cargo /pathmap:$build_root=/target"
else
  export CFLAGS="${CFLAGS:-} -ffile-prefix-map=$source_root=/dctx -fdebug-prefix-map=$source_root=/dctx -ffile-prefix-map=$cargo_root=/cargo -fdebug-prefix-map=$cargo_root=/cargo -ffile-prefix-map=$build_root=/target -fdebug-prefix-map=$build_root=/target"
fi
if [[ "$target" == *-linux-musl ]]; then
  # SQLite still needs native musl C headers/compiler; the final CRT/linker come from Rust.
  cc_variable="CC_${target//-/_}"
  export "$cc_variable=${!cc_variable:-musl-gcc}"
  export RUSTFLAGS="$RUSTFLAGS -C linker=rust-lld -C link-self-contained=yes -C link-arg=--build-id=none"
fi
cargo +"$DCTX_RELEASE_RUST" build --locked --release -p dctx-cli --bin dctx-cli --target "$target"
binary="${CARGO_TARGET_DIR:-target}/$target/release/dctx-cli"
if [[ "$target" == *-windows-* ]]; then binary+=.exe; fi
"$binary" --version
python3 tools/release/supply-chain.py archive --target "$target" --binary "$binary" \
  --out "$output" --epoch "$SOURCE_DATE_EPOCH"
