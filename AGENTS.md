# Agent notes (this fork)

This is a **personal fork** of `xai-org/grok-build`, not stock Grok.

## Installing / repairing the daily driver

Follow **[README.md](README.md) → “Install on a new machine”** only.

```bash
./scripts/setup-daily-driver.sh
```

Success is defined there (`readlink` → `scripts/grok`, `[grok-fork]` on `grok --version`).

## Hard rules

1. **`~/.grok/bin/grok` must be `scripts/grok`**, never `downloads/grok-fork-*`.
2. **Never** pass `--static-binary` to `install-local.sh` unless the human explicitly asked for a frozen binary.
3. Do **not** invent alternate install paths (copying the release binary over `bin/grok`, stock `grok update`, etc.).
4. After any install change, run the README success checks and stop if they fail.
5. Longer context: [FORK.md](FORK.md). Do not edit upstream user-guide crates for fork-only docs.
