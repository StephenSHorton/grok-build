# Stephen’s Grok Build fork

Personal fork of [xai-org/grok-build](https://github.com/xai-org/grok-build).
Not the official product.

## Critical invariant (read this)

**`~/.grok/bin/grok` must be the launch wrapper (`scripts/grok`), never a frozen binary.**

Every time you open `grok`, the wrapper:

1. Fetches **`origin`** (your GitHub fork: new commits from other machines / merges)
2. Fetches **`upstream`** (`xai-org/grok-build`: official monorepo syncs)
3. Fast-forwards onto origin when possible; rebases onto upstream when possible
4. Rebuilds the release binary **at most once** if `HEAD` moved (or the binary is missing)
5. Execs `target/release/xai-grok-pager`

If `bin/grok` points at `~/.grok/downloads/grok-fork-*` instead, **auto-update is dead** and you will silently rot on an old build. That is what went wrong on 2026-08-05 after the codesign install path.

| Command | Role |
|---------|------|
| `grok` / `agent` | Fork launcher (git sync + single rebuild + run) |
| `grok-official` / `agent-official` | Always stock — escape hatch if the fork is broken |

```bash
# Correct daily driver (what install-local does by default):
readlink ~/.grok/bin/grok
# → …/projects/grok-build/scripts/grok

# Wrong (frozen; no git sync):
# → ../downloads/grok-fork-macos-aarch64
```

Repair if unwired:

```bash
cd ~/projects/grok-build
./scripts/install-local.sh --skip-build   # re-link launcher only
# or full seed build + link:
./scripts/install-local.sh
```

## Patches on `main`

| Patch | Notes |
|-------|--------|
| **Transparent canvas** | App canvas BGs cleared (`Color::Reset`) so the host underlay (e.g. suzuri rain) shows through. **Selection / hover bands kept** (`bg_visual`, `bg_hover`, `bg_highlight`) so list pickers stay readable. Opt out: `GROK_SOLID_BG=1` |
| Launch wrapper | `scripts/grok` — **required** daily driver; origin + upstream sync; one rebuild max |
| Install helpers | `scripts/install-local.sh` — wires launcher + `grok-official`; optional seed build |

### macOS: `zsh: killed     grok`

Local `cargo build --release` binaries are **linker adhoc-signed (4k pages)**. After `cp` into `~/.grok/downloads/`, macOS often kills them at launch with **Code Signature Invalid / Invalid Page** (`SIGKILL`).

The **launcher runs the binary from `target/release/`** (no copy), which avoids that class of failure. Prefer the launcher path. Only use `./scripts/install-local.sh --static-binary` if you knowingly want a frozen, re-signed download (and accept no auto-sync).

## Remotes

```bash
git remote add upstream https://github.com/xai-org/grok-build.git   # once
# origin  → your fork (e.g. StephenSHorton/grok-build)
# upstream → xai-org/grok-build
```

| Remote | On every `grok` open |
|--------|----------------------|
| `origin` | `git fetch` + **fast-forward** if you are behind (no auto-push; no auto-merge if diverged) |
| `upstream` | `git fetch` + **rebase** onto `upstream/main` if upstream moved |

Both fetches happen in one launch pass; **cargo runs at most once** after all git work.

Keep personal patches on `main` (or merge feature branches into `main` before relying on the launcher).
`SOURCE_REV` is the monorepo commit SHA for this tree.

### How to tell you are current

```bash
cd ~/projects/grok-build
git fetch origin && git fetch upstream
git status -sb
git rev-list --count HEAD..origin/main      # 0 = not behind your fork remote
git rev-list --count HEAD..upstream/main   # 0 = fully rebased onto xAI
# Opening grok should log e.g.:
#   [grok-fork] fetching origin + upstream …
#   [grok-fork] already up to date with …
#   [grok-fork] binary current (…)
```

Soft failures (diverged origin, rebase conflicts) write `~/projects/grok-build/.fork-upstream-status` and still launch the last good binary.

## Daily driver install

Stock CLI lives under `~/.grok/`:

| Path | Role |
|------|------|
| `~/.grok/bin/grok` | **must be** `scripts/grok` (launcher) |
| `~/.grok/bin/agent` | same launcher |
| **`~/.grok/bin/grok-official`** | **always stock** escape hatch |
| `~/.grok/bin/agent-official` | same as `grok-official` |
| `~/.grok/downloads/` | stock binaries (+ optional frozen fork if you use `--static-binary`) |
| `~/.grok/config.toml` | `auto_update = false` so stock channel does not overwrite the fork |

### Install / repair

```bash
./scripts/install-local.sh              # seed build + wire launcher + auto_update=false
./scripts/install-local.sh --skip-build # re-wire launcher only (no cargo)
./scripts/install-local.sh --rollback   # stock download as bin/grok
./scripts/install-local.sh --ensure-official  # stock escape hatch only
# Avoid unless you know you want no auto-sync:
./scripts/install-local.sh --static-binary
```

### Escape hatch: `grok-official`

```bash
grok-official --version    # stock
grok --version             # fork launcher → fork binary
```

### Env skips (launcher)

| Var | Effect |
|-----|--------|
| `GROK_SKIP_SYNC=1` | don’t fetch/ff/rebase |
| `GROK_SKIP_REBUILD=1` | don’t cargo build |
| `GROK_SOLID_BG=1` | stock solid backgrounds |
| `GROK_FORK_REPO` | override checkout path (default `~/projects/grok-build`) |
| `GROK_FORK_BRANCH` | branch to keep checked out (default `main`) |

### Manual build (normally unnecessary)

```bash
cargo install dotslash   # once; bin/protoc
cargo build -p xai-grok-pager-bin --release
# → target/release/xai-grok-pager  (what the launcher execs)
```

## What went wrong (2026-08-05) — do not repeat

Two install stories lived side by side:

1. **Intended:** `scripts/grok` as system `grok` (launch-time upstream sync).
2. **`install-local.sh`:** copy a release binary into `~/.grok/downloads/` and point `bin/grok` at it (needed briefly for adhoc codesign after `cp`).

Running (2) after (1) **silently replaced the launcher** with a static binary. Auto-update stopped; nobody noticed until remotes moved.

**Policy now:** default `install-local.sh` only wires the launcher. Static binary install is opt-in (`--static-binary`) and prints a warning. Docs call out the `readlink` check above.

## Windows

Stock install: `%USERPROFILE%\.grok\bin\`. Prefer the PowerShell launcher `scripts/grok.ps1` for the same origin+upstream+single-rebuild behavior (keep it in sync with `scripts/grok` when changing sync policy).

**Do not run stock `grok update`** — it overwrites the fork entrypoint. Keep `auto_update = false`.

## Docs in this tree

| Doc | Status |
|-----|--------|
| This file (`FORK.md`) | Fork-specific; safe to edit |
| Root `README.md` | Fork banner + pointers |
| `crates/.../docs/user-guide/` | Upstream product docs — avoid fork-only edits (monorepo sync) |

## License

Same as upstream: Apache License 2.0 for first-party code. See `LICENSE` and
`THIRD-PARTY-NOTICES`.
