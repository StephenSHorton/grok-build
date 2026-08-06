# Grok Build (personal fork)

Personal fork of [xai-org/grok-build](https://github.com/xai-org/grok-build) (SpaceXAI’s terminal coding agent).
**Not** the official product. **Not** a community alternative.

If you are setting this up on a machine that already has stock `grok`, start here and stop when the success checks pass.

---

## Install on a new machine

**Needs:** stock Grok from [x.ai/cli](https://x.ai/cli) already installed · [rustup](https://rustup.rs) · git access to this repo · macOS or Linux.

```bash
git clone git@github.com:StephenSHorton/grok-build.git ~/projects/grok-build
cd ~/projects/grok-build
./scripts/setup-daily-driver.sh
```

That script adds the `upstream` remote if missing, ensures `dotslash`, runs `install-local.sh` (seed release build + wire daily driver), and **fails** if the daily driver is not the launch wrapper.

### Success (install is wrong if either fails)

```bash
readlink ~/.grok/bin/grok
# must end with: /scripts/grok
# must NOT be:   …/downloads/grok-fork-…

grok --version
# stderr includes: [grok-fork] fetching origin + upstream …
```

### What you get

| Command | Behavior |
|---------|----------|
| `grok` / `agent` | Fork **launcher**: each open fetches **origin** (this fork) + **upstream** (xAI), fast-forwards / rebases when safe, rebuilds **at most once** if `HEAD` moved, then runs the build |
| `grok-official` | Always stock (escape hatch) |

`~/.grok/config.toml` gets `auto_update = false` so the stock channel does not overwrite the fork entrypoint.

### Do not

- Do **not** run `./scripts/install-local.sh --static-binary` for normal use (frozen binary → **no** auto-update).
- Do **not** point `~/.grok/bin/grok` at anything under `downloads/grok-fork-*`.
- Do **not** use stock `grok update` while the fork is the daily driver.

### Repair / rollback

```bash
cd ~/projects/grok-build   # or wherever you cloned
./scripts/install-local.sh --skip-build   # re-wire launcher only
./scripts/install-local.sh --rollback     # stock binary as default grok
grok-official                             # stock without changing default
```

Full policy, remotes, and history of the silent-unwire bug: **[FORK.md](FORK.md)**.

---

## What this fork changes

- **Transparent canvas by default** — host underlay (e.g. suzuri rain) shows through; selection/hover bands kept (`GROK_SOLID_BG=1` for solid stock look)
- **Launch-time sync** — always origin + upstream, single rebuild when needed
- Patches live on **`main`**

## Upstream

| | |
|--|--|
| Official source | [github.com/xai-org/grok-build](https://github.com/xai-org/grok-build) |
| Product install | [x.ai/cli](https://x.ai/cli) |
| Docs / changelog | [docs.x.ai/build](https://docs.x.ai/build/overview) · [changelog](https://x.ai/build/changelog) |

Report Grok Build product bugs **upstream**. This fork does not take external PRs for the public tree ([CONTRIBUTING.md](CONTRIBUTING.md)).

## Building without installing

```sh
cargo install dotslash   # once; hermetic bin/protoc
cargo build -p xai-grok-pager-bin --release   # → target/release/xai-grok-pager
cargo run -p xai-grok-pager-bin
```

For daily use, prefer `./scripts/setup-daily-driver.sh` / `install-local.sh` so `grok` stays the launcher.

`SOURCE_REV` is the monorepo commit SHA for this tree.

## Docs map

| Doc | Use |
|-----|-----|
| **This README** | New-machine install + success checks |
| [FORK.md](FORK.md) | Invariant, remotes, repair, design notes |
| [docs.x.ai/build](https://docs.x.ai/build/overview) | Official product docs |
| `crates/.../docs/user-guide/` | Upstream in-tree guide (avoid fork-only edits; monorepo sync) |

## Layout (short)

| Path | Role |
|------|------|
| `scripts/setup-daily-driver.sh` | New-machine one-shot |
| `scripts/install-local.sh` | Wire launcher / rollback / official escape hatch |
| `scripts/grok` | Daily-driver launcher (git sync + rebuild + exec) |
| `scripts/grok-official` | Stock escape hatch |
| `FORK.md` | Fork operations |
| `crates/codegen/xai-grok-pager-bin` | Binary composition root |

> [!IMPORTANT]
> Root `Cargo.toml` is **generated** — read-only. Edit per-crate `Cargo.toml` files.

## License

First-party: **Apache-2.0** — [`LICENSE`](LICENSE).  
Third-party: [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) and related notice files.
