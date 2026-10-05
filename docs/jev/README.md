# Optional Jev on this fork

Jev is a typed judge (boolean / choice / score), not an LLM. On this fork it is **strictly optional**. A key enables the `ask_jev` tool and turns the self-validation **nudge** on. `safety_check` and `context_filter` stay off until you turn them on.

No key ⇒ no tool, no HTTP, no errors. Behavior matches this fork without Jev.

## `/jev-setup`

Enable Jev from the TUI (or ACP/headless) without editing the file by hand.

1. `/jev-setup` shows on/off, key source (env vs file), a **masked** key, and flag states. The full key is never printed or logged.
2. `/jev-setup set` opens a masked prompt in the TUI. Do not paste the key on the slash line (it can land in scrollback). ACP/headless: `/jev-setup set [--force] <key>` (never echoed).
3. A tiny live Decide ping (`Is 1 less than 2?`) validates the key. Failures are shown; nothing is saved unless you re-run with `--force`.
4. The key is written to `[jev].api_key` in `~/.grok/config.toml` (or `$GROK_HOME`) with comments and sibling tables preserved.
5. Then toggle extras: `/jev-setup nudge on|off` (default **on** with a key), `safety on|off` (default off), `filter on|off` (default off). One-line explanations are in the status text.
6. `/jev-setup off` removes the file key. If `JEV_API_KEY` / `TYPESAFE_API_KEY` is set, the UI says so — env still wins, so Jev stays on until you unset the env var.
7. If the session is idle, Jev turns on immediately (`ask_jev` registered, `<jev>` prompt section added). If a turn is running, run `/jev-setup apply` when idle or start a new session.
8. `/jev-stats` (or `/jev-setup stats`) shows this session’s Decide totals plus all-time totals from a small local log.

## `/jev-stats`

When Jev is on, every Decide is recorded by source (`ask_jev`, safety check, context filter, `/jev-setup` validation):

- latency, success/failure (error kind only — no response bodies)
- question count and modes
- input/output tokens from `usage` when the API sends them
- safety: allowed or denied, plus the live noul
- filter: kept or dropped, plus estimated chars/tokens saved
- nudges shown, and whether `ask_jev` followed within the next 5 tool calls

`/jev-stats` prints session totals, a per-source breakdown, p50/p95 latency, failure rate, tokens, filter drop rate, and safety deny count. A rolling JSONL at `$GROK_HOME/jev_stats.jsonl` (capped) holds all-time totals. The log stores **sizes, not contents** — no keys, no state payloads.

No key ⇒ no recorder, no file, no overhead. `/jev-stats` still runs and says Jev is off.

## Set the key (manual)

First non-empty wins:

1. Environment `JEV_API_KEY`
2. Environment `TYPESAFE_API_KEY`
3. File `[jev].api_key` in `~/.grok/config.toml` (or `$GROK_HOME/config.toml`)

Empty / whitespace values do not count. `GROK_CONFIG` overlays cannot inject a key (`[jev]` is not overlay-allowlisted). Prefer env for secrets. `/jev-setup` writes the file layer only.

Hosted keys starting with `jv_live_` call `https://jevtypesafeai.com/api/v1/decide`. Every other key (including official TypeSafe keys) calls `https://api.typesafe.ai/v1/systemone`. Optional `[jev].base_url` overrides that prefix rule. Default model is `jev-latest`; override with `[jev].model`.

## Config fields

```toml
[jev]
# Optional file-layer key. Env wins when set.
api_key = ""
# Optional Decide URL override.
# base_url = "https://jevtypesafeai.com/api/v1/decide"
# model = "jev-latest"

# Nudge defaults on when a key is present. Safety/filter stay off.
nudge = true
nudge_every = 2          # every N successful edits/shells; <= 0 disables
safety_check = false
risk_block = 0.72        # live noul at/above this may deny
allow_destructive = false
context_filter = false
```

## What each flag does

| Field | Default | Effect when on (and a key is set) |
|---|---|---|
| *(key only)* | — | Registers `ask_jev`. The `<jev>` prompt tells the model to use it as the default for enumerable decisions. |
| `nudge` | **on** (with a key) | After a successful edit/write or allowed shell, a `<system-reminder>` may suggest `ask_jev`. Text only — **no extra Jev call**. Rate-limited by `nudge_every` (default every 2nd successful mutate). Set `nudge = false` to turn it off. |
| `safety_check` | off | After the existing permission layer **allows** a bash/edit/write/MCP/`apply_patch` call, ask Jev noul “is this destructive or hard to undo?”. Deny only on a **live** noul ≥ `risk_block` when `allow_destructive` is false. Timeouts, HTTP errors, and missing answers **allow** (fail-open). There is no hard-coded deny and no offline policy. |
| `context_filter` | off | On the **next completion request** (not the session log), older tool results past the last 3 user turns may be omitted if live Jev says they no longer help (noul below 0.55). Errors keep the item. At most four Jev calls per request. Compaction and grep hits are not filtered yet. |

Yolo / always-approve does not skip an explicit Jev safety deny. A down Jev cannot freeze the session: faults fail open.

## `ask_jev`

One tool call is one Decide request (batched named questions). Modes: `boolean` (wire `noul`), `choice` (needs `options`), `score` (needs 2–10 `levels`). Aliases `noul` / `yes` / `yesno` → boolean.

A failed call is an observation: `error` is set, each answer has `detail` `Jev call failed, question not answered` or `Jev returned no answer`, and **no fabricated `value`**. The turn continues.

Read-only and allowed in plan mode. `disallowed_tools = ["ask_jev"]` still hides it when enabled.

## What the model sees

Only when a key is present, the system prompt gets a short `<jev>` section: use `ask_jev` as the default for enumerable decisions (Jev answers in ~100–400ms vs seconds of model reasoning — offload the choice instead of thinking it through), put facts in `state` (Jev cannot read files), batch named boolean/choice/score questions, act on the answer, fall back to your own judgment on low confidence or failure (never invent answers), skip only already-certain steps and tight loops, and which extras are on. No key ⇒ that section is absent and the prompt is byte-identical to the no-Jev snapshot.

## Related

- Slice history and insertion points: [`plan.md`](plan.md), [`harness-map.md`](harness-map.md)
- Public decide how-to: <https://jevtypesafeai.com/decide/how-to-use>
