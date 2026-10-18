#!/bin/sh
# Installer regression for decision 078 / D3-LEGACY-REMOVAL §4 and packet D3-06.
# Uses a fake build, a Git trap and SHA-256 comparisons in a disposable project.
# Depends on POSIX utilities and shasum or sha256sum; used by the packet oracle.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
TEST_ROOT=$(mktemp -d)
trap 'rm -rf "$TEST_ROOT"' 0
trap 'exit 1' HUP INT TERM
PROJECT="$TEST_ROOT/project"
mkdir -p "$PROJECT/tools" "$PROJECT/crates/dctx-core/templates" "$TEST_ROOT/bin"
cp "$SCRIPT_DIR/../install.sh" "$PROJECT/tools/install.sh"
printf 'test-version\n' > "$PROJECT/VERSION"
printf 'template fixture\n' > "$PROJECT/crates/dctx-core/templates/default.txt"
printf '*.fixture-cache\n' > "$PROJECT/crates/dctx-core/templates/global-ignore"
printf '*.fixture-lock\n' > "$PROJECT/crates/dctx-core/templates/global-omitcontent"

export HOME="$TEST_ROOT/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_STATE_HOME="$HOME/.local/state"
export DCTX_INSTALL_DIR="$TEST_ROOT/installed"
export GIT_CALLS="$TEST_ROOT/git-calls"
mkdir -p "$HOME"
printf '# personal patterns\ncustom-global\n' > "$HOME/.gitignore_global"
printf '# project patterns\ncustom-local\n' > "$PROJECT/.gitignore"

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    else
        shasum -a 256 "$1"
    fi
}
GLOBAL_BEFORE=$(sha256 "$HOME/.gitignore_global")
LOCAL_BEFORE=$(sha256 "$PROJECT/.gitignore")

# Only these tools and our shims are available to the installer.
for utility in uname dirname tr cat mkdir cp wc chmod; do
    ln -s "$(command -v "$utility")" "$TEST_ROOT/bin/$utility"
done
cat > "$TEST_ROOT/bin/git" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >> "$GIT_CALLS"
exit 1
EOF
cat > "$TEST_ROOT/bin/cargo" <<'EOF'
#!/bin/sh
set -eu
[ "$*" = 'build --release -p dctx-cli' ]
mkdir -p target/release
printf '#!/bin/sh\nexit 0\n' > target/release/dctx-cli
chmod +x target/release/dctx-cli
EOF
chmod +x "$TEST_ROOT/bin/git" "$TEST_ROOT/bin/cargo"

# Run both fresh-config and existing-config installation paths without network/builds.
PATH="$TEST_ROOT/bin" /bin/sh "$PROJECT/tools/install.sh"
test -x "$DCTX_INSTALL_DIR/dctx"
cmp "$PROJECT/target/release/dctx-cli" "$DCTX_INSTALL_DIR/dctx"
cmp "$PROJECT/crates/dctx-core/templates/default.txt" "$XDG_CONFIG_HOME/dctx/templates/default.txt"
cmp "$PROJECT/crates/dctx-core/templates/global-ignore" "$XDG_CONFIG_HOME/dctx/ignore"
cmp "$PROJECT/crates/dctx-core/templates/global-omitcontent" "$XDG_CONFIG_HOME/dctx/omitcontent"
test -s "$XDG_CONFIG_HOME/dctx/config.toml"
if grep -q 'update_local_git_repo_gitignore_rules\|CFG_KEY_GIT_SAFETY' "$XDG_CONFIG_HOME/dctx/config.toml"; then
    echo 'Installer still generates the obsolete Git safety setting' >&2
    exit 1
fi
CONFIG_BEFORE=$(sha256 "$XDG_CONFIG_HOME/dctx/config.toml")
PATH="$TEST_ROOT/bin" /bin/sh "$PROJECT/tools/install.sh"
test "$CONFIG_BEFORE" = "$(sha256 "$XDG_CONFIG_HOME/dctx/config.toml")"
test ! -e "$GIT_CALLS"
test "$GLOBAL_BEFORE" = "$(sha256 "$HOME/.gitignore_global")"
test "$LOCAL_BEFORE" = "$(sha256 "$PROJECT/.gitignore")"
echo 'PASS: no Git calls; global and project ignore SHA-256 hashes unchanged; binary and assets installed'
