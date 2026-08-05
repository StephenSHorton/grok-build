# Stephen’s Grok Build fork

Personal fork of [xai-org/grok-build](https://github.com/xai-org/grok-build).
Not the official product.

## Patches on `main`

| Patch | Notes |
|-------|--------|
| **Transparent canvas** | App canvas BGs cleared (`Color::Reset`) so the host underlay (e.g. suzuri rain) shows through. **Selection / hover bands kept** (`bg_visual`, `bg_hover`, `bg_highlight`) so list pickers stay readable. Opt out: `GROK_SOLID_BG=1` |
| Launch wrappers | `scripts/grok` (macOS/Linux), `scripts/grok.ps1` (Windows) — optional sync/rebuild on start |
| Install helpers | `scripts/install-local.sh` — release build, **adhoc re-sign** after copy, smoke-test, then link `~/.grok/bin` |

### macOS: `zsh: killed     grok`

Local `cargo build --release` binaries are **linker adhoc-signed (4k pages)**. After `cp` into `~/.grok/downloads/`, macOS often kills them at launch with **Code Signature Invalid / Invalid Page** (`SIGKILL`) — crash logs show `CODESIGNING`. Stock downloads are **Developer ID** signed and fine.

`install-local.sh` re-signs with `codesign -s - --force` (proper adhoc, 16k pages), verifies, and runs `--version` **before** retargeting `bin/grok`. If the smoke test fails, stock links are left alone.

```bash
./scripts/install-local.sh              # safe install
./scripts/install-local.sh --rollback   # back to newest stock download
```

- **Upstream remote:** `upstream` → `xai-org/grok-build`
- **Fork daily-driver branch:** `main` (not a side branch)
- Official docs/install: [x.ai/cli](https://x.ai/cli) · [docs.x.ai/build](https://docs.x.ai/build/overview)

`SOURCE_REV` is the monorepo commit SHA for this tree.

## Remotes

```bash
git remote add upstream https://github.com/xai-org/grok-build.git   # once
git fetch upstream
# prefer: rebase/merge upstream into main, then re-apply is automatic if patches stay on main
```

Keep personal patches on `main` (or merge feature branches into `main` before installing).
Do not leave the daily-driver patches only on a side branch.

## Daily driver install (replace stock `grok`)

Stock CLI lives under `~/.grok/`:

| Path | Role |
|------|------|
| `~/.grok/bin/grok` | daily driver (fork after install-local; stock after rollback) |
| `~/.grok/bin/agent` | same binary, agent entrypoint |
| **`~/.grok/bin/grok-official`** | **always stock** — escape hatch if the fork is broken |
| `~/.grok/bin/agent-official` | same as `grok-official` |
| `~/.grok/downloads/` | versioned stock + `grok-fork-macos-aarch64` |
| `~/.grok/config.toml` | settings (`auto_update`, …) |

### Escape hatch: `grok-official`

Fork install never replaces this. It always execs the newest stock download
under `~/.grok/downloads/` (Developer ID signed).

```bash
grok-official --version    # stock
grok --version             # fork (if installed as daily driver)

# Only refresh the escape hatch (no build):
./scripts/install-local.sh --ensure-official
```

Also copied to `~/.local/bin/grok-official` when that directory exists.

### Recommended: build + install into `~/.grok`

```bash
./scripts/install-local.sh              # release build + install + auto_update=false
./scripts/install-local.sh --skip-build # install existing target/release/xai-grok-pager
./scripts/install-local.sh --rollback   # newest stock download under downloads/
./scripts/install-local.sh --ensure-official  # stock escape hatch only
```

### Alternative: wrapper launcher (sync + rebuild + run)

```bash
# Point a name on PATH at the repo wrapper (prefer after ~/.grok/bin, or replace it):
ln -sfn ~/projects/grok-build/scripts/grok ~/.local/bin/grok-fork

GROK_FORK_BRANCH=main ~/.local/bin/grok-fork --version
# defaults: GROK_FORK_REPO=~/projects/grok-build, GROK_FORK_BRANCH=main
```

Env skips:

| Var | Effect |
|-----|--------|
| `GROK_SKIP_SYNC=1` | don’t fetch/rebase upstream |
| `GROK_SKIP_REBUILD=1` | don’t cargo build |
| `GROK_SOLID_BG=1` | stock solid backgrounds |

### Manual build

```bash
cargo install dotslash   # once; bin/protoc
cargo build -p xai-grok-pager-bin --release
# → target/release/xai-grok-pager
```

## Windows

Stock install: `%USERPROFILE%\.grok\bin\`. First install can back up as
`grok.upstream.exe` / `agent.upstream.exe`.

```powershell
& "$HOME\projects\grok-build\scripts\grok.ps1" --version
# or after a release build:
Copy-Item target\release\xai-grok-pager.exe $HOME\.grok\bin\grok.exe -Force
Copy-Item target\release\xai-grok-pager.exe $HOME\.grok\bin\agent.exe -Force
```

- Hermetic `bin/protoc` has no Windows entry — install protoc 29.3 / set `PROTOC`, or use `.tools/protoc/` (gitignored).
- `xai-proto-build` uses temp files instead of `/dev/stdout` so protoc dep scanning works on Windows.
- Skips: `$env:GROK_SKIP_SYNC`, `GROK_SKIP_REBUILD`, `GROK_SKIP_INSTALL`

**Do not run stock `grok update`** — it overwrites the fork binary. Use rebuild + reinstall
(`install-local.sh` / `grok.ps1`) instead. Keep `auto_update = false` in config.

## Docs in this tree

| Doc | Status |
|-----|--------|
| This file (`FORK.md`) | Fork-specific; safe to edit |
| Root `README.md` | Fork banner + pointers |
| `crates/.../docs/user-guide/` | Upstream product docs — avoid fork-only edits (monorepo sync) |

## License

Same as upstream: Apache License 2.0 for first-party code. See `LICENSE` and
`THIRD-PARTY-NOTICES`.
