#!/usr/bin/env bash
# Install a self-built grok binary as the daily driver under ~/.grok/
# (replaces stock bin/grok + bin/agent symlinks without deleting stock downloads).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GROK_HOME="${GROK_HOME:-$HOME/.grok}"
BIN_DIR="$GROK_HOME/bin"
DOWNLOADS="$GROK_HOME/downloads"
FORK_NAME="grok-fork-macos-aarch64"
FORK_PATH="$DOWNLOADS/$FORK_NAME"
BINARY="$REPO_ROOT/target/release/xai-grok-pager"
PACKAGE="xai-grok-pager-bin"

SKIP_BUILD=0
ROLLBACK=0
DISABLE_AUTO_UPDATE=1

usage() {
  cat <<'EOF'
Usage: ./scripts/install-local.sh [options]

  Install this repo's release binary as ~/.grok/bin/grok (and agent).

Options:
  --skip-build       Use existing target/release/xai-grok-pager (do not cargo build)
  --rollback         Point bin/ at the newest stock download (not the fork)
  --keep-auto-update Do not set auto_update = false in ~/.grok/config.toml
  -h, --help         Show this help

Environment:
  GROK_HOME          Install root (default: ~/.grok)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --rollback) ROLLBACK=1; shift ;;
    --keep-auto-update) DISABLE_AUTO_UPDATE=0; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }

ensure_layout() {
  mkdir -p "$BIN_DIR" "$DOWNLOADS"
}

newest_stock_binary() {
  # Prefer versioned stock names; ignore our fork install name.
  local candidate=""
  # shellcheck disable=SC2012
  candidate="$(
    ls -1t "$DOWNLOADS"/grok-*-macos-aarch64 2>/dev/null \
      | grep -v 'fork' \
      | head -1 \
      || true
  )"
  if [[ -z "$candidate" && -x "$DOWNLOADS/grok-macos-aarch64" ]]; then
    candidate="$DOWNLOADS/grok-macos-aarch64"
  fi
  printf '%s' "$candidate"
}

link_bin() {
  local target_rel="$1"
  ln -sfn "$target_rel" "$BIN_DIR/grok"
  ln -sfn "$target_rel" "$BIN_DIR/agent"
  echo "linked $BIN_DIR/grok -> $target_rel"
  echo "linked $BIN_DIR/agent -> $target_rel"
}

disable_auto_update() {
  local cfg="$GROK_HOME/config.toml"
  if [[ ! -f "$cfg" ]]; then
    mkdir -p "$GROK_HOME"
    cat >"$cfg" <<'EOF'
[cli]
auto_update = false
EOF
    echo "created $cfg with auto_update = false"
    return
  fi

  if grep -qE '^[[:space:]]*auto_update[[:space:]]*=' "$cfg"; then
    # macOS/BSD sed needs -i ''
    if sed --version >/dev/null 2>&1; then
      sed -i 's/^[[:space:]]*auto_update[[:space:]]*=.*/auto_update = false/' "$cfg"
    else
      sed -i '' 's/^[[:space:]]*auto_update[[:space:]]*=.*/auto_update = false/' "$cfg"
    fi
    echo "set auto_update = false in $cfg"
    return
  fi

  if grep -qE '^[[:space:]]*\[cli\]' "$cfg"; then
    if sed --version >/dev/null 2>&1; then
      sed -i '/^[[:space:]]*\[cli\]/a\
auto_update = false
' "$cfg"
    else
      # Insert after first [cli] on BSD sed
      awk '
        BEGIN { done=0 }
        /^\[cli\]/ && !done { print; print "auto_update = false"; done=1; next }
        { print }
        END { if (!done) print "\n[cli]\nauto_update = false" }
      ' "$cfg" >"$cfg.tmp" && mv "$cfg.tmp" "$cfg"
    fi
    echo "added auto_update = false under [cli] in $cfg"
  else
    printf '\n[cli]\nauto_update = false\n' >>"$cfg"
    echo "appended [cli] auto_update = false to $cfg"
  fi
}

do_rollback() {
  ensure_layout
  local stock
  stock="$(newest_stock_binary)"
  [[ -n "$stock" && -x "$stock" ]] || die "no stock binary found under $DOWNLOADS"
  local base
  base="$(basename "$stock")"
  link_bin "../downloads/$base"
  echo "rolled back to stock: $base"
  "$BIN_DIR/grok" --version || true
}

do_install() {
  ensure_layout

  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    command -v cargo >/dev/null || die "cargo not found"
    if ! command -v dotslash >/dev/null; then
      echo "warning: dotslash not on PATH (bin/protoc needs it; install: cargo install dotslash)" >&2
    fi
    echo "building release ($PACKAGE)…"
    (cd "$REPO_ROOT" && cargo build -p "$PACKAGE" --release)
  fi

  [[ -x "$BINARY" ]] || die "missing binary: $BINARY (build first or omit --skip-build)"

  cp -f "$BINARY" "$FORK_PATH"
  chmod +x "$FORK_PATH"
  echo "installed $FORK_PATH"

  link_bin "../downloads/$FORK_NAME"

  if [[ "$DISABLE_AUTO_UPDATE" -eq 1 ]]; then
    disable_auto_update
  fi

  echo
  "$BIN_DIR/grok" --version
  echo
  echo "Done. Open a new shell/session so existing grok processes pick this up."
}

if [[ "$ROLLBACK" -eq 1 ]]; then
  do_rollback
else
  do_install
fi
