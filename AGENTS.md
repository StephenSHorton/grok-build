# Agent notes — personal fork of Grok Build

This checkout is **Stephen’s personal fork** of [`xai-org/grok-build`](https://github.com/xai-org/grok-build), not the official product.

**`README.md` stays upstream.** Do not rewrite it for fork install policy. All fork operations live in **this file** and under `scripts/`.

---

## Hard rules

1. **Daily driver is the launcher, not a frozen binary.**
   - Unix: `~/.grok/bin/grok` → `scripts/grok`
   - Windows: `%USERPROFILE%\.grok\bin\grok.cmd` → `scripts\grok.ps1` (stock `grok.exe` must not shadow it)
2. **Never** install with `--static-binary` / frozen copy over `bin/grok` unless the human explicitly asked (that disables auto-update).
3. **Do not** edit `crates/**/docs/user-guide/**` for fork-only docs (monorepo sync overwrites them).
4. After install, run the **success checks** below and stop if they fail.
5. Keep personal patches on **`main`**. Launcher always checks out / syncs `main`.

---

## What every open of `grok` does

**Intentional model: local git + local `cargo`.** Rust on each machine is fine and expected.
Every launch re-checks remotes and rebuilds only when the tree actually moved — that is how
we stay on latest (origin patches and upstream monorepo syncs), not via GitHub Release downloads.

One pass, **at most one rebuild**:

1. `git fetch origin` (this fork on GitHub)
2. `git fetch upstream` (`xai-org/grok-build`)
3. Fast-forward onto origin when behind (no auto-push; no auto-merge if diverged)
4. Rebase onto `upstream/main` when upstream moved
5. `cargo build --release` only if `HEAD` changed (or binary missing)
6. Exec the binary (`xai-grok-pager` / `.exe`)

Escape hatch (always stock): `grok-official` (Unix) / `grok-official.cmd` (Windows).

Stock channel: keep `auto_update = false` in `~/.grok/config.toml` so official updates do not overwrite the fork entrypoint.

---

## New machine

**Prerequisites (all OS):** stock Grok already installed from [x.ai/cli](https://x.ai/cli) · [rustup](https://rustup.rs) · git access to this fork.

### macOS / Linux

```bash
git clone git@github.com:StephenSHorton/grok-build.git ~/projects/grok-build
cd ~/projects/grok-build
./scripts/setup-daily-driver.sh
```

### Windows (PowerShell)

```powershell
git clone git@github.com:StephenSHorton/grok-build.git $HOME\projects\grok-build
cd $HOME\projects\grok-build
.\scripts\setup-daily-driver.ps1
```

Any clone path is fine; launchers resolve the repo from their own location (symlink-safe on Unix).

### Success checks (install failed if these fail)

**Unix**

```bash
readlink ~/.grok/bin/grok          # must end with /scripts/grok
grok --version                    # stderr: [grok-fork] fetching origin + upstream …
```

**Windows**

```powershell
Get-Content $HOME\.grok\bin\grok.cmd   # must invoke scripts\grok.ps1
# grok.exe must NOT be the daily driver (should be absent or renamed stock backup)
Get-Command grok | Format-List
grok --version                         # stderr: [grok-fork] …
```

---

## Scripts (cross-platform)

| Script | OS | Role |
|--------|-----|------|
| `scripts/setup-daily-driver.sh` | macOS/Linux | remotes + deps + install + hard verify |
| `scripts/setup-daily-driver.ps1` | Windows | same |
| `scripts/install-local.sh` | macOS/Linux | wire launcher / rollback / `grok-official` |
| `scripts/install-local.ps1` | Windows | wire `grok.cmd` launcher / rollback / official |
| `scripts/grok` | macOS/Linux | launch-time sync + rebuild + exec |
| `scripts/grok.ps1` | Windows | same |
| `scripts/grok-official` | macOS/Linux | stock binary from `~/.grok/downloads` |

### Repair

```bash
# Unix
./scripts/install-local.sh --skip-build
./scripts/install-local.sh --rollback
```

```powershell
# Windows
.\scripts\install-local.ps1 -SkipBuild
.\scripts\install-local.ps1 -Rollback
```

### Env skips (all OS)

| Variable | Effect |
|----------|--------|
| `GROK_SKIP_SYNC=1` | skip fetch / ff / rebase |
| `GROK_SKIP_REBUILD=1` | skip cargo |
| `GROK_SOLID_BG=1` | stock solid canvas backgrounds |
| `GROK_FORK_REPO` | override checkout path |
| `GROK_FORK_BRANCH` | default `main` |

---

## Remotes

| Remote | Points at | On open |
|--------|-----------|---------|
| `origin` | `StephenSHorton/grok-build` | fetch + FF if behind |
| `upstream` | `https://github.com/xai-org/grok-build.git` | fetch + rebase if ahead |

```bash
git remote add upstream https://github.com/xai-org/grok-build.git   # once
```

---

## Why local build (not CI prebuilts)

We deliberately do **not** ship fork binaries via GitHub Actions the way suzuri does app releases.

- Opening `grok` already **fetches origin + upstream** and rebases when needed → source is current.
- Rebuild runs **only if `HEAD` moved** → you get the latest code without a release train or download channel.
- Same loop on every OS that has Rust; no “which release asset matches this SHA” machinery.
- Stock Grok still has its own install channel (`grok-official`); the fork stays a thin launcher over a local tree.

Prerequisites on each machine: stock Grok + rustup + this clone. That is the product.

---

## Fork product patches (code)

| Patch | Notes |
|-------|--------|
| Transparent canvas | Default clear canvas for host underlay; keep selection/hover bands. `GROK_SOLID_BG=1` for solid. |

---

## Silent failure we already hit (do not repeat)

`install-local` once pointed `bin/grok` at a **copied** release binary under `downloads/` (macOS codesign workaround). That removed launch-time git sync with no error. Default install wires the launcher only; setup scripts **fail** if the link/cmd is wrong.

---

## Upstream docs

Official product docs: [docs.x.ai/build](https://docs.x.ai/build/overview). In-tree user guide under `crates/.../docs/user-guide/` is upstream — leave it alone for fork ops.
