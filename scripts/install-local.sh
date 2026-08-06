#!/usr/bin/env bash
# Install the fork *launcher* as the daily driver under ~/.grok/
# (replaces stock bin/grok + bin/agent without deleting stock downloads).
#
# CRITICAL: ~/.grok/bin/grok must be the launch wrapper (scripts/grok), NOT a
# static binary. The wrapper fetches origin + upstream and rebuilds at most once
# per launch. Pointing bin/grok at a frozen binary silently kills auto-update.
#
# Always maintains ~/.grok/bin/grok-official — escape hatch to stock even when
# bin/grok is the fork. Never overwritten by fork install.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GROK_HOME="${GROK_HOME:-$HOME/.grok}"
BIN_DIR="$GROK_HOME/bin"
DOWNLOADS="$GROK_HOME/downloads"
FORK_NAME="grok-fork-macos-aarch64"
FORK_PATH="$DOWNLOADS/$FORK_NAME"
BINARY="$REPO_ROOT/target/release/xai-grok-pager"
PACKAGE="xai-grok-pager-bin"
LAUNCHER_SRC="$REPO_ROOT/scripts/grok"
OFFICIAL_SCRIPT_SRC="$REPO_ROOT/scripts/grok-official"
STAMP="$REPO_ROOT/.fork-built-at"

SKIP_BUILD=0
ROLLBACK=0
ENSURE_OFFICIAL=0
STATIC_BINARY=0
DISABLE_AUTO_UPDATE=1

usage() {
  cat <<'EOF'
Usage: ./scripts/install-local.sh [options]

  Wire the fork launcher as ~/.grok/bin/grok (and agent) so every open:
    - fetches origin (your GitHub fork) + upstream (xai-org)
    - fast-forwards / rebases when possible
    - rebuilds the release binary at most once if HEAD moved
  Always refreshes ~/.grok/bin/grok-official → stock escape hatch.

Options:
  --skip-build       Do not cargo build (first launch will build if needed)
  --static-binary    DANGEROUS/legacy: install a frozen binary as bin/grok
                     (disables launch-time git sync — not recommended)
  --rollback         Point bin/grok at the newest stock download (not the fork)
  --ensure-official  Only install/update grok-official (no launcher / no build)
  --keep-auto-update Do not set auto_update = false in ~/.grok/config.toml
  -h, --help         Show this help

Environment:
  GROK_HOME          Install root (default: ~/.grok)

Escape hatch (always stock, even when grok is the fork):
  grok-official --version
  grok-official
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --static-binary) STATIC_BINARY=1; shift ;;
    --rollback) ROLLBACK=1; shift ;;
    --ensure-official) ENSURE_OFFICIAL=1; shift ;;
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
  # Prefer official installer's canonical name, then versioned; never *fork*.
  if [[ -x "$DOWNLOADS/grok-macos-aarch64" ]]; then
    printf '%s' "$DOWNLOADS/grok-macos-aarch64"
    return 0
  fi
  if [[ -x "$DOWNLOADS/grok-macos-x86_64" ]]; then
    printf '%s' "$DOWNLOADS/grok-macos-x86_64"
    return 0
  fi
  local candidate=""
  # shellcheck disable=SC2012
  candidate="$(
    ls -1t "$DOWNLOADS"/grok-*-macos-* 2>/dev/null \
      | grep -vE 'fork|staging' \
      | head -1 \
      || true
  )"
  printf '%s' "$candidate"
}

link_bin_to() {
  local target="$1"
  ln -sfn "$target" "$BIN_DIR/grok"
  ln -sfn "$target" "$BIN_DIR/agent"
  echo "linked $BIN_DIR/grok -> $target"
  echo "linked $BIN_DIR/agent -> $target"
}

# Daily driver = launch wrapper (absolute path into this checkout).
install_launcher() {
  [[ -f "$LAUNCHER_SRC" ]] || die "missing $LAUNCHER_SRC"
  [[ -x "$LAUNCHER_SRC" ]] || chmod +x "$LAUNCHER_SRC"
  # Absolute symlink so it works from any cwd; re-run install if the repo moves.
  link_bin_to "$LAUNCHER_SRC"
  # Optional convenience on ~/.local/bin when present (same wrapper).
  local local_bin="${HOME}/.local/bin"
  if [[ -d "$local_bin" ]]; then
    ln -sfn "$LAUNCHER_SRC" "$local_bin/grok-fork" 2>/dev/null || true
  fi
}

# Install grok-official launcher (always stock). Never points at the fork.
install_official_escape() {
  ensure_layout
  [[ -f "$OFFICIAL_SCRIPT_SRC" ]] || die "missing $OFFICIAL_SCRIPT_SRC"

  # Real script in bin/ (not a symlink into the repo) so it keeps working if
  # the checkout moves; re-copied on every install.
  cp -f "$OFFICIAL_SCRIPT_SRC" "$BIN_DIR/grok-official"
  chmod +x "$BIN_DIR/grok-official"
  # Same binary entry for agent users who type agent-official.
  ln -sfn "grok-official" "$BIN_DIR/agent-official"

  # Optional: also land on ~/.local/bin if that dir exists (extra PATH safety).
  local local_bin="${HOME}/.local/bin"
  if [[ -d "$local_bin" ]]; then
    cp -f "$OFFICIAL_SCRIPT_SRC" "$local_bin/grok-official"
    chmod +x "$local_bin/grok-official"
    ln -sfn "grok-official" "$local_bin/agent-official" 2>/dev/null || true
    echo "also installed $local_bin/grok-official"
  fi

  local stock
  stock="$(newest_stock_binary)"
  if [[ -n "$stock" && -x "$stock" ]]; then
    echo "linked escape hatch: $BIN_DIR/grok-official → stock $(basename "$stock")"
    if "$BIN_DIR/grok-official" --version >/dev/null 2>&1; then
      echo -n "  grok-official --version → "
      "$BIN_DIR/grok-official" --version
    else
      echo "warning: grok-official could not run stock binary at $stock" >&2
    fi
  else
    echo "warning: no stock binary under $DOWNLOADS yet; grok-official installed but will fail until official CLI is present" >&2
  fi
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

seed_build() {
  command -v cargo >/dev/null || die "cargo not found"
  if ! command -v dotslash >/dev/null; then
    echo "warning: dotslash not on PATH (bin/protoc needs it; install: cargo install dotslash)" >&2
  fi
  echo "seeding release build ($PACKAGE) so first launch can skip cargo if HEAD is unchanged…"
  (cd "$REPO_ROOT" && cargo build -p "$PACKAGE" --release)
  [[ -x "$BINARY" ]] || die "missing binary after build: $BINARY"
  git -C "$REPO_ROOT" rev-parse HEAD >"$STAMP" 2>/dev/null || true
  echo "seed build ok → $BINARY (stamp written; launcher will not rebuild until HEAD moves)"
}

# Legacy path: frozen signed binary as daily driver (disables auto-sync).
do_static_binary_install() {
  ensure_layout
  install_official_escape

  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    seed_build
  fi
  [[ -x "$BINARY" ]] || die "missing binary: $BINARY (build first or omit --skip-build)"

  local staging="$FORK_PATH.staging"
  cp -f "$BINARY" "$staging"
  chmod +x "$staging"
  if command -v codesign >/dev/null; then
    codesign -s - --force --timestamp=none "$staging" \
      || die "codesign adhoc re-sign failed for $staging"
    codesign --verify --verbose=2 "$staging" 2>&1 \
      || die "codesign verify failed for $staging"
  else
    echo "warning: codesign not found; binary may be killed by macOS (Invalid Page)" >&2
  fi
  if command -v xattr >/dev/null; then
    xattr -cr "$staging" 2>/dev/null || true
  fi
  if ! "$staging" --version >/dev/null 2>&1; then
    local ec=$?
    rm -f "$staging"
    die "smoke test failed (exit $ec): $staging --version — not linking bin/grok"
  fi
  mv -f "$staging" "$FORK_PATH"
  echo "installed $FORK_PATH (adhoc re-signed)"
  link_bin_to "../downloads/$FORK_NAME"
  echo
  echo "WARNING: --static-binary installed a frozen daily driver."
  echo "  Launch-time origin/upstream sync is OFF until you re-run without --static-binary."
}

do_rollback() {
  ensure_layout
  install_official_escape
  local stock
  stock="$(newest_stock_binary)"
  [[ -n "$stock" && -x "$stock" ]] || die "no stock binary found under $DOWNLOADS"
  local base
  base="$(basename "$stock")"
  # Relative link matches stock installer layout.
  link_bin_to "../downloads/$base"
  echo "rolled back to stock: $base"
  "$BIN_DIR/grok" --version || true
}


verify_launcher_link() {
  local link target
  link="$BIN_DIR/grok"
  [[ -e "$link" ]] || die "missing $link after install"
  if [[ -L "$link" ]]; then
    target="$(readlink "$link")"
  else
    target="$link"
  fi
  case "$target" in
    */scripts/grok) echo "verified: $link -> $target" ;;
    *)
      die "daily driver is not the launch wrapper.
  expected: …/scripts/grok
  got:      $target
  re-run without --static-binary:  $0 --skip-build"
      ;;
  esac
}

do_install() {
  ensure_layout
  # Escape hatch first so it exists even if later steps abort.
  install_official_escape

  if [[ "$STATIC_BINARY" -eq 1 ]]; then
    do_static_binary_install
  else
    install_launcher
    if [[ "$SKIP_BUILD" -eq 0 ]]; then
      seed_build
    else
      echo "skipped seed build; first grok launch will build if needed"
    fi
    verify_launcher_link
  fi

  if [[ "$DISABLE_AUTO_UPDATE" -eq 1 ]]; then
    disable_auto_update
  fi

  echo
  echo "daily driver: $BIN_DIR/grok -> $(readlink "$BIN_DIR/grok" 2>/dev/null || echo '?')"
  if [[ "$STATIC_BINARY" -eq 0 ]]; then
    echo "  (launcher: origin + upstream sync on every open; one rebuild max)"
  fi
  echo -n "fork:     "
  # Skip sync so install doesn't hang on network; still hits the binary path.
  if GROK_SKIP_SYNC=1 GROK_SKIP_REBUILD=1 "$BIN_DIR/grok" --version 2>/dev/null; then
    :
  else
    # Launcher may log to stderr before exec; try binary directly.
    if [[ -x "$BINARY" ]]; then
      "$BINARY" --version 2>/dev/null || echo "(build pending — run grok once)"
    else
      echo "(build pending — run grok once)"
    fi
  fi
  echo -n "official: "
  "$BIN_DIR/grok-official" --version 2>/dev/null || echo '?'
  echo
  echo "Done. If the fork misbehaves:  grok-official"
  echo "Full rollback of default grok: ./scripts/install-local.sh --rollback"
  echo "Re-wire launcher after moving the checkout: ./scripts/install-local.sh --skip-build"
}

if [[ "$ENSURE_OFFICIAL" -eq 1 ]]; then
  install_official_escape
elif [[ "$ROLLBACK" -eq 1 ]]; then
  do_rollback
else
  do_install
fi
