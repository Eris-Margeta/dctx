#!/usr/bin/env zsh
# Build and run the current CLI against a disposable source tree.
set -eu
set -o pipefail

repo_root=${0:A:h:h}
demo_root=$(mktemp -d "${TMPDIR:-/tmp}/dctx-quickstart.XXXXXXXX")
trap 'rm -rf -- "$demo_root"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$demo_root/project" "$demo_root/config"

export XDG_CONFIG_HOME="$demo_root/config"
export DCTX_CACHE_DIR="$demo_root/cache"
export DCTX_AUTO_CLEAN=0

cat > "$demo_root/project/main.rs" <<'RUST'
pub fn greeting() -> &'static str {
    "Hello from dctx!"
}

fn main() {
    println!("{}", greeting());
}
RUST

dctx_command=(cargo run --quiet --locked --manifest-path "$repo_root/Cargo.toml" -p dctx-cli --bin dctx-cli --)
"${dctx_command[@]}" --version
"${dctx_command[@]}" pack "$demo_root/project" --ext rs --output "$demo_root/pack.txt"
[[ "$(<"$demo_root/pack.txt")" == *'pub fn greeting()'* ]]
"${dctx_command[@]}" map map main.rs --place "$demo_root/project" --depth 2
"${dctx_command[@]}" map find greeting --place "$demo_root/project" --limit 5 > "$demo_root/find.txt"
[[ "$(<"$demo_root/find.txt")" == *greeting* ]]
cat "$demo_root/find.txt"
"${dctx_command[@]}" map read m:main.rs --place "$demo_root/project" --json > "$demo_root/read.txt"
cat "$demo_root/read.txt"
[[ "$(<"$demo_root/read.txt")" == *'Hello from dctx!'* ]]
print 'Quickstart passed: pack, outline, find and source read.'
