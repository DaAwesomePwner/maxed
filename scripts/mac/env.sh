# Source from bash or zsh: `. scripts/mac/env.sh`
# Puts the pinned Rust (rust-toolchain.toml -> 1.99.0) and CI-pinned Node 22.16.0 on PATH.
# LLVM is keg-only and only needed for cargo-xwin (Windows cross-lint/link).
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
MAXED_NODE_BIN="$(fnm exec --using=22.16.0 node -p 'require("path").dirname(process.execPath)' 2>/dev/null)"
if [ -n "$MAXED_NODE_BIN" ]; then
  export PATH="$MAXED_NODE_BIN:$PATH"
elif ! command -v fnm >/dev/null 2>&1; then
  echo "maxed env: fnm not found; run 'brew install fnm && fnm install 22.16.0'" >&2
else
  echo "maxed env: Node 22.16.0 missing; run 'fnm install 22.16.0'" >&2
fi
export MAXED_XWIN_PATH="/opt/homebrew/opt/llvm/bin"
unset MAXED_NODE_BIN
