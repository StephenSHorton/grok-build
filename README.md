# Grok Build (personal fork)

This is **my personal fork** of [xai-org/grok-build](https://github.com/xai-org/grok-build) —
SpaceXAI's terminal-based AI coding agent (`grok`).

It is **not** the official product, and it is **not** a community-maintained alternative.

## What this fork is for

Local builds and experiments on top of the public source tree:

- **Transparent canvas by default** — app canvas cleared so the host underlay (e.g. suzuri rain) shows through; selection/hover bands kept so lists stay readable (`GROK_SOLID_BG=1` for stock solid look)
- **Launch-time auto-update** — `grok` is the wrapper (`scripts/grok`): every open fetches **origin** (your fork) + **upstream** (xAI), then rebuilds **at most once** if `HEAD` moved
- Daily-driver install that wires that wrapper over stock `~/.grok/bin/grok` (never leave a frozen binary as `grok`)
- Personal patches on `main` while staying current with upstream
- Source inspection / debugging against a known monorepo `SOURCE_REV`

## Upstream

| | |
|--|--|
| **Source of truth** | [github.com/xai-org/grok-build](https://github.com/xai-org/grok-build) |
| **Product / install** | [x.ai/cli](https://x.ai/cli) |
| **Docs** | [docs.x.ai/build](https://docs.x.ai/build/overview) · [changelog](https://x.ai/build/changelog) |

Bug reports and contributions for Grok Build itself should go **upstream**, not here.
External PRs are not accepted on the public tree (see [CONTRIBUTING.md](CONTRIBUTING.md)).

## Daily driver (replace stock)

Full notes: **[FORK.md](FORK.md)** — start with **New machine** + **Critical invariant**.

### New machine (stock Grok already installed)

```bash
git clone git@github.com:StephenSHorton/grok-build.git ~/projects/grok-build
cd ~/projects/grok-build
git remote add upstream https://github.com/xai-org/grok-build.git   # once
cargo install dotslash                                              # once (needs rustup)
./scripts/install-local.sh                                          # seed build + wire launcher

readlink ~/.grok/bin/grok   # MUST end in /scripts/grok  (not downloads/grok-fork-*)
grok --version              # should log [grok-fork] fetching origin + upstream …
```

Do **not** use `--static-binary` (that freezes a binary and kills auto-update).

Rollback / escape hatch:

```bash
./scripts/install-local.sh --rollback   # stock as default grok
grok-official                           # stock without changing default
```

---

## Building from source

Requirements:

- **Rust** — pinned by [`rust-toolchain.toml`](rust-toolchain.toml); `rustup` installs it on first build
- **[DotSlash](https://dotslash-cli.com)** — hermetic tools under [`bin/`](bin/) (notably `bin/protoc`):

  ```sh
  cargo install dotslash
  /usr/bin/env dotslash --help
  ```

- **protoc** — via `./bin/protoc` (DotSlash) or `protoc` / `$PROTOC` on `PATH`
- macOS and Linux are supported; Windows is best-effort

```sh
cargo run -p xai-grok-pager-bin              # build + launch the TUI
cargo build -p xai-grok-pager-bin --release  # → target/release/xai-grok-pager
cargo check -p xai-grok-pager-bin
```

The artifact is named `xai-grok-pager`; official installs ship it as `grok`.
Use `./scripts/install-local.sh` to install under `~/.grok/` as `grok`.

`SOURCE_REV` records the monorepo commit SHA for this tree.

## Documentation

- **Fork / install-as-daily-driver:** [FORK.md](FORK.md)
- **Product docs (upstream):** [docs.x.ai/build](https://docs.x.ai/build/overview)
- **In-tree user guide:** [`crates/codegen/xai-grok-pager/docs/user-guide/`](crates/codegen/xai-grok-pager/docs/user-guide/)

Prefer not to put fork-only notes in the crate user guide — monorepo syncs overwrite that tree.

## Repository layout

| Path | Contents |
|------|----------|
| `crates/codegen/xai-grok-pager-bin` | Composition-root; builds `xai-grok-pager` |
| `crates/codegen/xai-grok-pager` | TUI |
| `crates/codegen/xai-grok-shell` | Agent runtime + entry points |
| `crates/codegen/xai-grok-tools` | Tools (terminal, edit, search, …) |
| `crates/codegen/xai-grok-workspace` | Host FS, VCS, execution, checkpoints |
| `crates/common/`, `crates/build/`, `prod/mc/` | Shared leaf crates |
| `third_party/` | Vendored upstream (e.g. Mermaid stack) |
| `scripts/install-local.sh` | Fork daily-driver installer |
| `FORK.md` | Fork remotes, install, update policy |

> [!IMPORTANT]
> The root `Cargo.toml` is **generated** — treat it as read-only. Edit per-crate
> `Cargo.toml` files instead.

## Development

```sh
cargo check -p <crate>        # target specific crates; full workspace is slow
cargo test -p xai-grok-config
cargo clippy -p <crate>
cargo fmt --all
```

## License

First-party code: **Apache License 2.0** — see [`LICENSE`](LICENSE).

Third-party / vendored code keeps its original licenses — see
[`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES),
[`crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md`](crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md),
and [`third_party/NOTICE`](third_party/NOTICE).
