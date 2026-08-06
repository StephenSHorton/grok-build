# Stephen’s Grok Build fork

Personal fork of [xai-org/grok-build](https://github.com/xai-org/grok-build).
Not the official product.

**New machine?** Use the README install section (or `./scripts/setup-daily-driver.sh`). This file is the longer operational reference.

## Critical invariant

**`~/.grok/bin/grok` must be the launch wrapper (`scripts/grok`), never a frozen binary.**

| Correct | Wrong |
|---------|--------|
| `readlink ~/.grok/bin/grok` → `…/scripts/grok` | → `…/downloads/grok-fork-*` |
| Each open: fetch origin + upstream, ≤1 rebuild | Static binary; remotes never applied |

| Command | Role |
|---------|------|
| `grok` / `agent` | Launcher: sync remotes → rebuild if needed → run |
| `grok-official` / `agent-official` | Always stock |

Repair:

```bash
./scripts/install-local.sh --skip-build    # re-link launcher
# or
./scripts/setup-daily-driver.sh            # full setup + hard verify
```

## What every open does

One pass, one rebuild maximum:

1. `git fetch origin` (this GitHub fork)
2. `git fetch upstream` (`xai-org/grok-build`)
3. Fast-forward onto origin when behind (no auto-push; no auto-merge if diverged)
4. Rebase onto `upstream/main` when upstream moved
5. `cargo build -p xai-grok-pager-bin --release` only if `HEAD` ≠ last build stamp (or binary missing)
6. `exec` `target/release/xai-grok-pager`

Soft failures (diverged origin, rebase conflicts) write `.fork-upstream-status` in the repo and still launch the last good binary.

## New machine (detail)

Same outcome as README. Preferred entry:

```bash
git clone git@github.com:StephenSHorton/grok-build.git ~/projects/grok-build
cd ~/projects/grok-build
./scripts/setup-daily-driver.sh
```

Manual equivalent:

```bash
git remote add upstream https://github.com/xai-org/grok-build.git   # once
cargo install dotslash                                              # once
./scripts/install-local.sh                                          # never --static-binary
readlink ~/.grok/bin/grok   # must end in /scripts/grok
grok --version              # [grok-fork] lines on stderr
```

Clone path can be anything: the launcher resolves its repo from the real path of `scripts/grok` (symlink-safe).

### Prerequisites

- Stock Grok already installed ([x.ai/cli](https://x.ai/cli)) so `~/.grok/` exists and stock binaries can back `grok-official`
- [rustup](https://rustup.rs) / `cargo` (toolchain pinned by `rust-toolchain.toml`)
- Network for first `cargo build --release` (several minutes)

## Patches on `main`

| Patch | Notes |
|-------|--------|
| **Transparent canvas** | Canvas BGs cleared (`Color::Reset`) for host underlay; **selection/hover bands kept**. Opt out: `GROK_SOLID_BG=1` |
| Launch wrapper | `scripts/grok` — required daily driver |
| Install | `setup-daily-driver.sh` / `install-local.sh` — launcher + `grok-official` |

### macOS: `zsh: killed     grok`

Copying a cargo release binary into `~/.grok/downloads/` without re-signing often dies with **Code Signature Invalid**. The **launcher runs from `target/release/`** (no copy) and avoids that. Do not use `--static-binary` unless you accept frozen builds + manual codesign.

## Remotes

| Remote | URL | On open |
|--------|-----|---------|
| `origin` | your fork (`StephenSHorton/grok-build`) | fetch + FF if behind |
| `upstream` | `https://github.com/xai-org/grok-build.git` | fetch + rebase if ahead |

Keep personal patches on `main`. `SOURCE_REV` is the monorepo SHA for this tree.

### Am I current?

```bash
git fetch origin && git fetch upstream
git rev-list --count HEAD..origin/main     # 0 = not behind fork remote
git rev-list --count HEAD..upstream/main   # 0 = contains latest upstream
```

Or just open `grok` and read the `[grok-fork]` lines on stderr.

## Install commands

```bash
./scripts/setup-daily-driver.sh              # new machine: remotes + deps + install + verify
./scripts/install-local.sh                   # seed build + wire launcher + auto_update=false
./scripts/install-local.sh --skip-build      # re-wire launcher only
./scripts/install-local.sh --rollback        # stock download as bin/grok
./scripts/install-local.sh --ensure-official # stock escape hatch only
./scripts/install-local.sh --static-binary   # NOT for daily driver (disables sync)
```

Layout under `~/.grok/`:

| Path | Role |
|------|------|
| `bin/grok`, `bin/agent` | → `scripts/grok` |
| `bin/grok-official` | stock |
| `downloads/` | stock binaries |
| `config.toml` | `auto_update = false` |

### Env (launcher)

| Var | Effect |
|-----|--------|
| `GROK_SKIP_SYNC=1` | skip fetch/ff/rebase |
| `GROK_SKIP_REBUILD=1` | skip cargo |
| `GROK_SOLID_BG=1` | solid stock backgrounds |
| `GROK_FORK_REPO` | override repo (default: parent of real `scripts/grok`) |
| `GROK_FORK_BRANCH` | branch to keep checked out (default `main`) |

## Why this almost got lost (2026-08-05)

1. Intended: `scripts/grok` as system `grok` (launch-time sync).
2. `install-local.sh` briefly installed a **frozen** re-signed binary under `downloads/` and pointed `bin/grok` at it (codesign workaround).
3. That silently killed auto-update.

**Policy now:** default install only wires the launcher; setup script hard-fails if the link is wrong; `--static-binary` is opt-in and warned.

## Windows

Prefer `scripts/grok.ps1` (same origin + upstream + single-rebuild policy). Keep it aligned when changing sync rules. Do not run stock `grok update` over the fork; keep `auto_update = false`.

## Docs in this tree

| Doc | Status |
|-----|--------|
| [README.md](README.md) | New-machine install + success checks |
| This file | Operations / invariant / history |
| `crates/.../docs/user-guide/` | Upstream — avoid fork-only edits |

## License

Apache-2.0 first-party. See `LICENSE` and `THIRD-PARTY-NOTICES`.
