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

One pass, **at most one rebuild** (or one prebuilt fetch later — see “Prebuilt releases”):

1. `git fetch origin` (this fork on GitHub)
2. `git fetch upstream` (`xai-org/grok-build`)
3. Fast-forward onto origin when behind (no auto-push; no auto-merge if diverged)
4. Rebase onto `upstream/main` when upstream moved
5. Build release binary only if `HEAD` changed (or binary missing)
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
| `GROK_SKIP_REBUILD=1` | skip cargo (and later: skip download) |
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

## Prebuilt releases (suzuri-style) — design notes, not required today

**Today:** each machine builds with `cargo` after git sync. Works offline-ish once cached; needs Rust everywhere; Windows builds are slow/fragile.

**Suzuri model:** tag/push → GitHub Actions builds OS matrices → release assets → clients download.

**Why we might want that here**

- No Rust toolchain on every laptop
- Windows machines stop fighting local MSVC/protoc
- Faster “open Grok after someone pushed a fix”

**Why it’s not a free win**

- This workspace is a large Rust monorepo slice; CI minutes and caches are heavy (not a small Go host like suzuri)
- Launch-time **git** sync (origin + upstream rebase) is still needed for *source* truth; prebuilts only replace the **cargo** step
- macOS notarization/Developer ID optional for personal use (adhoc is enough for self)
- Must stamp each asset with git SHA so the launcher downloads only when `HEAD` (post-sync) ≠ installed SHA — still **one** binary update per open, not cargo + download

**Target shape (when implemented)**

1. Workflow on `main` (and/or tags): matrix `macos-aarch64`, `linux-x86_64`, `windows-x86_64` → `cargo build -p xai-grok-pager-bin --release` → upload to GitHub Releases named by SHA/OS.
2. Launcher after git sync: if a release asset exists for `HEAD`+OS, download into `~/.grok/downloads/` (or repo `target/release/`) and skip cargo; else cargo fallback.
3. Same success rule: daily driver remains the **launcher** scripts, never “only the downloaded exe on PATH”.

Until that exists, use local cargo via the setup scripts above.

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
