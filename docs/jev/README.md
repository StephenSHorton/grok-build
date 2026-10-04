# Optional Jev on this fork

Jev is a typed judge (boolean / choice / score), not an LLM. On this fork it is **strictly optional**. A key only enables the `ask_jev` tool. Extra features stay off until you turn their flags on.

No key ⇒ no tool, no HTTP, no errors. Behavior matches this fork without Jev.

## Set the key

First non-empty wins:

1. Environment `JEV_API_KEY`
2. Environment `TYPESAFE_API_KEY`
3. File `[jev].api_key` in `~/.grok/config.toml` (or `$GROK_HOME/config.toml`)

Empty / whitespace values do not count. `GROK_CONFIG` overlays cannot inject a key (`[jev]` is not overlay-allowlisted). Prefer env for secrets.

Hosted keys starting with `jv_live_` call `https://jevtypesafeai.com/api/v1/decide`. Every other key (including official TypeSafe keys) calls `https://api.typesafe.ai/v1/systemone`. Optional `[jev].base_url` overrides that prefix rule. Default model is `jev-latest`; override with `[jev].model`.

## Config fields

```toml
[jev]
# Optional file-layer key. Env wins when set.
api_key = ""
# Optional Decide URL override.
# base_url = "https://jevtypesafeai.com/api/v1/decide"
# model = "jev-latest"

# All flags default false even when a key is present.
nudge = false
nudge_every = 2          # every N successful edits/shells; <= 0 disables
safety_check = false
risk_block = 0.72        # live noul at/above this may deny
allow_destructive = false
context_filter = false
```

## What each flag does

| Field | Default | Effect when on (and a key is set) |
|---|---|---|
| *(key only)* | — | Registers `ask_jev`. The model decides when to call it. |
| `nudge` | off | After a successful edit/write or allowed shell, a `<system-reminder>` may suggest considering `ask_jev`. Text only — **no extra Jev call**. Rate-limited by `nudge_every` (default every 2nd successful mutate). |
| `safety_check` | off | After the existing permission layer **allows** a bash/edit/write/MCP/`apply_patch` call, ask Jev noul “is this destructive or hard to undo?”. Deny only on a **live** noul ≥ `risk_block` when `allow_destructive` is false. Timeouts, HTTP errors, and missing answers **allow** (fail-open). There is no hard-coded deny and no offline policy. |
| `context_filter` | off | On the **next completion request** (not the session log), older tool results past the last 3 user turns may be omitted if live Jev says they no longer help (noul below 0.55). Errors keep the item. At most four Jev calls per request. Compaction and grep hits are not filtered yet. |

Yolo / always-approve does not skip an explicit Jev safety deny. A down Jev cannot freeze the session: faults fail open.

## `ask_jev`

One tool call is one Decide request (batched named questions). Modes: `boolean` (wire `noul`), `choice` (needs `options`), `score` (needs 2–10 `levels`). Aliases `noul` / `yes` / `yesno` → boolean.

A failed call is an observation: `error` is set, each answer has `detail` `Jev call failed, question not answered` or `Jev returned no answer`, and **no fabricated `value`**. The turn continues.

Read-only and allowed in plan mode. `disallowed_tools = ["ask_jev"]` still hides it when enabled.

## Related

- Slice history and insertion points: [`plan.md`](plan.md), [`harness-map.md`](harness-map.md)
- Public decide how-to: <https://jevtypesafeai.com/decide/how-to-use>
