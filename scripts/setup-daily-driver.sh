#!/usr/bin/env bash
# One-shot new-machine setup for this personal fork.
# Run from a clone of StephenSHorton/grok-build (any path).
#
# Does:
#   - ensure upstream remote
#   - ensure cargo + dotslash
#   - ./scripts/install-local.sh  (seed build + wire launcher)
#   - verify ~/.grok/bin/grok → scripts/grok (not a frozen binary)
#
# Prerequisites (not installed by this script):
#   - stock Grok CLI already present (~/.grok from https://x.ai/cli)
#   - rustup / cargo
#   - git access to this fork
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

die() { echo "error: $*" >&2; exit 1; }
ok() { echo "ok: $*"; }

echo "== Grok fork daily-driver setup =="
echo "repo: $REPO_ROOT"
echo

# --- git ---
[[ -d .git ]] || die "not a git checkout: $REPO_ROOT"
if ! git remote get-url origin &>/dev/null; then
  die "missing remote 'origin' (clone your fork, not a bare tarball)"
fi
if ! git remote get-url upstream &>/dev/null; then
  echo "adding upstream → https://github.com/xai-org/grok-build.git"
  git remote add upstream https://github.com/xai-org/grok-build.git
fi
ok "remotes: origin=$(git remote get-url origin)"
ok "         upstream=$(git remote get-url upstream)"
git fetch --quiet upstream main 2>/dev/null || echo "warning: could not fetch upstream (offline? continuing)"
echo

# --- toolchain ---
command -v cargo >/dev/null || die "cargo not found — install Rust from https://rustup.rs then re-run"
ok "cargo: $(command -v cargo)"
if ! command -v dotslash >/dev/null; then
  echo "installing dotslash (needed for bin/protoc)…"
  cargo install dotslash
fi
ok "dotslash: $(command -v dotslash)"
echo

# --- stock home ---
if [[ ! -d "${GROK_HOME:-$HOME/.grok}" ]]; then
  echo "warning: ${GROK_HOME:-$HOME/.grok} missing — install stock Grok from https://x.ai/cli first"
  echo "         continuing; install-local will create layout but grok-official needs a stock binary"
fi

# --- install (launcher, not static binary) ---
echo "running install-local.sh (seed release build; can take several minutes)…"
./scripts/install-local.sh
echo

# --- verify (hard fail) ---
GROK_BIN="${GROK_HOME:-$HOME/.grok}/bin/grok"
[[ -e "$GROK_BIN" ]] || die "missing $GROK_BIN after install"
if [[ -L "$GROK_BIN" ]]; then
  target="$(readlink "$GROK_BIN")"
else
  target="$GROK_BIN"
fi
case "$target" in
  */scripts/grok)
    ok "daily driver → $target"
    ;;
  *)
    die "daily driver is NOT the launcher.
  expected symlink ending in /scripts/grok
  got: $target
  fix:  cd $REPO_ROOT && ./scripts/install-local.sh --skip-build
  never use --static-binary for the daily driver"
    ;;
esac

# Smoke: launcher should at least resolve; skip network if offline.
if ! GROK_SKIP_SYNC=1 GROK_SKIP_REBUILD=1 "$GROK_BIN" --version >/dev/null 2>&1; then
  # Still success if binary will build on first real launch
  if [[ -x "$REPO_ROOT/target/release/xai-grok-pager" ]]; then
    die "launcher failed --version even with an existing release binary"
  fi
  echo "warning: --version failed (binary may still be building on first open); launcher link is correct"
else
  ver="$(GROK_SKIP_SYNC=1 GROK_SKIP_REBUILD=1 "$GROK_BIN" --version 2>/dev/null | tail -1 || true)"
  ok "grok --version → $ver"
fi

echo
echo "== setup complete =="
echo "Every open of \`grok\` will fetch origin + upstream and rebuild at most once if HEAD moved."
echo "Escape hatch (stock): grok-official"
echo "Details: $REPO_ROOT/FORK.md"
