#!/bin/sh
# Builds tools/playground/dist/: the Docker build context of the playground image.
# The image needs no compiler: dist/ holds two static musl binaries, the samples and the
# runtime Dockerfile. Run from anywhere; needs cargo with the x86_64-unknown-linux-musl target
# and a musl C compiler for sqlite (musl-gcc, or zig through tools/playground/musl-cc.sh).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
target=x86_64-unknown-linux-musl
dist="$here/dist"
if [ -z "${CC_x86_64_unknown_linux_musl:-}" ]; then
  if command -v musl-gcc >/dev/null 2>&1; then CC_x86_64_unknown_linux_musl=musl-gcc
  elif command -v zig >/dev/null 2>&1; then CC_x86_64_unknown_linux_musl="$here/musl-cc.sh"
  else echo "build-dist: no musl C compiler (install musl-tools or zig)" >&2; exit 2; fi
fi
export CC_x86_64_unknown_linux_musl
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
cargo build --locked --release --target "$target" -p dctx-cli --manifest-path "$root/Cargo.toml"
CARGO_TARGET_DIR="$target_dir" cargo build --locked --release --target "$target" --manifest-path "$here/Cargo.toml"
rm -rf "$dist"
mkdir -p "$dist/bin" "$dist/samples"
cp "$target_dir/$target/release/dctx-cli" "$dist/bin/dctx"
cp "$target_dir/$target/release/dctx-play" "$dist/bin/dctx-play"
cp -R "$root/tools/website/sample" "$dist/samples/kiosk"
cp -R "$root/crates/dctx-lex-go/tests/fixtures" "$dist/samples/lexer-go"
cp -R "$root/crates/dctx-map-parse/tests/fixtures" "$dist/samples/parser-mixed"
printf 'kiosk\tkiosk: the website sample (Rust, TypeScript, Python, TOML, Markdown)\nlexer-go\tGo lexer fixtures (crates/dctx-lex-go/tests/fixtures)\nparser-mixed\tParser test fixtures: C, C#, Go, Java, JavaScript, Kotlin, Markdown, PHP, Python, Ruby, Rust, Swift, TOML, TypeScript, YAML (crates/dctx-map-parse/tests/fixtures)\n' > "$dist/samples/index.tsv"
cp "$here/openapi.json" "$dist/openapi.json"
cp "$here/Dockerfile.runtime" "$dist/Dockerfile"
chmod -R a+rX "$dist"
version=$("$dist/bin/dctx" --version)
printf '%s\n' "$version" > "$dist/VERSION"
(cd "$dist" && find . -type f ! -name SHA256SUMS | LC_ALL=C sort | xargs sha256sum > SHA256SUMS)
echo "build-dist: $version -> $dist ($(du -sh "$dist" | cut -f1))"
