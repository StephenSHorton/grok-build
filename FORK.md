# Personal fork notes

This tree is a **personal fork** of [xai-org/grok-build](https://github.com/xai-org/grok-build).
It is not the official product and is not a community-maintained alternative.

Official installs and docs stay at [x.ai/cli](https://x.ai/cli) and
[docs.x.ai/build](https://docs.x.ai/build/overview).

## Remotes

| Remote | URL |
|--------|-----|
| `origin` | your fork (e.g. `git@github.com:StephenSHorton/grok-build.git`) |
| `upstream` | `https://github.com/xai-org/grok-build.git` |

```bash
git remote add upstream https://github.com/xai-org/grok-build.git   # once
git fetch upstream
git merge upstream/main   # or rebase; keep main a clean mirror if you prefer
```

**Policy:** prefer custom work on feature branches. Keep `main` easy to fast-forward
from upstream when the public tree is re-synced from the monorepo.

`SOURCE_REV` is the monorepo commit SHA for this tree.

## Daily driver install (replace stock `grok`)

Stock CLI lives under `~/.grok/`:

| Path | Role |
|------|------|
| `~/.grok/bin/grok` | symlink used by `PATH` |
| `~/.grok/bin/agent` | same binary, agent entrypoint |
| `~/.grok/downloads/` | versioned binaries |
| `~/.grok/config.toml` | settings (`auto_update`, …) |

### One-shot script

From the repo root:

```bash
./scripts/install-local.sh            # release build + install + disable auto_update
./scripts/install-local.sh --skip-build   # install existing target/release/xai-grok-pager
./scripts/install-local.sh --rollback     # point bin/ back at latest stock download
```

### Manual steps

Requirements: Rust (see `rust-toolchain.toml`), [DotSlash](https://dotslash-cli.com)
on `PATH` (`cargo install dotslash`), and `protoc` via `./bin/protoc` or system.

```bash
# 1. Build
cargo build -p xai-grok-pager-bin --release
# → target/release/xai-grok-pager

# 2. Install next to stock downloads (name is fixed so reinstalls overwrite cleanly)
cp -f target/release/xai-grok-pager ~/.grok/downloads/grok-fork-macos-aarch64
chmod +x ~/.grok/downloads/grok-fork-macos-aarch64

# 3. Point CLI entrypoints at the fork
ln -sfn ../downloads/grok-fork-macos-aarch64 ~/.grok/bin/grok
ln -sfn ../downloads/grok-fork-macos-aarch64 ~/.grok/bin/agent

# 4. Stop stock channel from overwriting you on launch
# In ~/.grok/config.toml under [cli]:
#   auto_update = false
```

Verify in a **new** shell (current sessions keep the old binary in memory):

```bash
which grok
grok --version   # expect a local build, e.g. 0.2.119 (gitsha) [alpha]
```

### Roll back to stock

```bash
./scripts/install-local.sh --rollback
# or manually:
ln -sfn ../downloads/grok-0.2.118-macos-aarch64 ~/.grok/bin/grok   # pick a stock file you still have
ln -sfn ../downloads/grok-0.2.118-macos-aarch64 ~/.grok/bin/agent
# re-enable updates if you want:
#   auto_update = true
```

### Rebuild after code changes

```bash
./scripts/install-local.sh
# same as: build → copy → relink
```

## Auto-update and `grok update`

| Action | Effect on a fork install |
|--------|--------------------------|
| Launch with `auto_update = true` (default) | May download stock and re-point `bin/` |
| `grok update` | Pulls official release channel — **not** this fork |
| `auto_update = false` | Safe for daily-driver fork use |

Do not rely on the in-app updater for fork builds. Sync git + rebuild instead.

## Config / data

Fork and stock share the same home layout:

- Config: `~/.grok/config.toml`
- Auth: `~/.grok/auth.json`
- Sessions, memory, skills, plugins: under `~/.grok/`

No separate config profile is required. Only the binary under `bin/` changes.

## Docs in this tree

| Doc | Status |
|-----|--------|
| This file (`FORK.md`) | Fork-specific; safe to edit |
| Root `README.md` | Fork banner + pointers; upstream build notes retained |
| `crates/.../docs/user-guide/` | Upstream product docs; **avoid fork-only edits** (monorepo sync overwrites) |

Product behavior docs: use the shipped user guide or [docs.x.ai/build](https://docs.x.ai/build/overview).

## License

Same as upstream: Apache License 2.0 for first-party code. See `LICENSE` and
`THIRD-PARTY-NOTICES`.
