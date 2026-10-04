# Jev on grok-build — sliced plan

Jev is a fast typed judge (choice / score / noul). On this fork it is **strictly optional**. No key ⇒ fully off: no tool registered, no HTTP, no errors, behavior identical to upstream `xai-org/grok-build` plus this fork’s existing extras.

Rock (`StephenSHorton/rock`) is a Go Jev-first clone. This plan **matches Rock’s wire and `ask_jev` contract** and **does not** copy Rock’s process model. Rock refuses to start without a key (or runs an offline policy that is “not Jev”). We never do that.

Read with [`harness-map.md`](harness-map.md). User-facing setup: [`README.md`](README.md).

---

## What shipped (2026-10-04)

All planned slices landed on this fork’s `main`. Feature flags stay **off** even when a key is present. No key ⇒ no tool, no HTTP, no errors.

| Slice | PR | What actually shipped |
|---|---|---|
| (a) config + `jev_enabled()` | [#5](https://github.com/StephenSHorton/grok-build/pull/5) | `JevConfig` on typed `Config`. Precedence `JEV_API_KEY` → `TYPESAFE_API_KEY` → `[jev].api_key`. `[jev]` is not on `OVERLAY_ALLOW_PATHS`. No `grok inspect` line (would change no-key output). |
| (b) client | [#6](https://github.com/StephenSHorton/grok-build/pull/6) | Isolated `xai-grok-jev` crate: `EndpointFor`, 30s timeout, 1 MiB cap. Empty key is an error at `Decide`; no HTTP. No gates, no offline policy. |
| (c) `ask_jev` | [#8](https://github.com/StephenSHorton/grok-build/pull/8) | Tool registered only when settings are enabled. `ToolKind::Other`, `is_read_only()`, plan-mode allowed. Failed Decide is JSON `error`/`detail`, no fabricated `value`. `JEV_API_KEY` / `TYPESAFE_API_KEY` added to BYOK scrub. No extra system-prompt sentence. Default builder tool-name snapshot locked to the pre-`ask_jev` fixture. |
| (d) nudge | [#9](https://github.com/StephenSHorton/grok-build/pull/9) | `JevNudgeReminder` always registered; emits only when `JevNudgeConfig` is in Resources (`key && nudge && nudge_every > 0`). Counts successful `search_replace`/`write`/`apply_patch`/bash-exit-0. **No extra Decide call.** No system-prompt sentence (keeps the no-key prompt identical). |
| (e) safety check | [#10](https://github.com/StephenSHorton/grok-build/pull/10) | After permission allow (plan-mode already not deny), optional noul “is this destructive?” for bash / edit (including write) / MCP / `apply_patch`. Deny only on **live** noul ≥ `risk_block` (default 0.72) when `allow_destructive` is false. Jev faults always **allow**. No hard-coded Rock `Gates.Risk`, no offline deny. |
| (f) context filter | [#11](https://github.com/StephenSHorton/grok-build/pull/11) | **Smaller safe version.** `prune_conversation` stays sync and HTTP-free (`xai-chat-state` has no Jev client). After `build_request`, if `context_filter` is on, the same older tool results prune considers (skip last 3 user turns) may be replaced on the **request clone** with `[Tool result omitted — not relevant]`. Drop only on live keep=false (noul below 0.55). Errors keep the item. Query/snippet clipped to 400/800 chars. At most 4 Decide calls per request. Compaction input and grep `KeepSnippet` were **not** hooked (follow-up **f2**). |

`docs/jev/README.md` is the short user-facing guide (key, `[jev]` fields, flags).

Sources used (2026-10-04):

- Rock `main` `605cad9`: [`docs/synthesis/ask-jev-plan.md`](https://github.com/StephenSHorton/rock/blob/605cad98167b31c60ef175102e840e8dc9220fda/docs/synthesis/ask-jev-plan.md), [`docs/synthesis/jev-audit.md`](https://github.com/StephenSHorton/rock/blob/605cad98167b31c60ef175102e840e8dc9220fda/docs/synthesis/jev-audit.md).
- Rock Go: `internal/jev/client.go` (wire), `internal/jev/ask.go` (`ask_jev` mapping), `internal/jev/gates.go` (hard gates — **not** ported as always-on).
- Public decide how-to: <https://jevtypesafeai.com/decide/how-to-use>.

---

## Where Jev helps most here

The harness already has permission rules, PreToolUse hooks, sandbox, identical-call nudges, and LLM compaction. Jev is a **cheap typed extra observation**, not a replacement for those.

Highest leverage, in order:

1. **Agent-callable `ask_jev`** — boolean / choice / score in the moment. Self-validation after a fix, error triage, “which of these files matters?” The model decides when to call. This is the product.
2. **Optional self-validation nudge** — after a successful edit/write or allowed shell, a short reminder that the agent *may* call `ask_jev`. Text only; does not call Jev itself. Matches Rock slice (b).
3. **Optional permission-layer safety check** — noul “is this destructive?” after the existing decision is not deny. Fail-open. **Not** Rock’s hard `Gates.Risk` (Rock keeps that hard-coded and yolo does not bypass it). We will not add an always-on block.
4. **Optional context filter** — first at per-turn `prune_conversation` (what the next `Complete` sees), then compaction input, later maybe grep hits. Fail-open: errors keep the snippet. There is no `KeepSnippet` today.

Out of scope for this effort unless a later slice explicitly takes it: Rock’s every-turn `BeforeTurn` (model/skill/stuck/compact), `SubagentKind`, `PlanReady`, process fail-fast, TUI diamonds as a 1.0 requirement. A later TUI mark for `ask_jev` can follow the tool.

---

## Jev API (as Rock uses it — implement this)

Do not invent endpoints, types, or verbs.

**Keys.** Env `JEV_API_KEY`, fallback `TYPESAFE_API_KEY`. On this fork both are optional, plus a config-file field. Env overrides file.

**Endpoint by key prefix.**

| Key | URL |
|---|---|
| `jv_live_*` | `POST https://jevtypesafeai.com/api/v1/decide` |
| anything else (including official TypeSafe keys) | `POST https://api.typesafe.ai/v1/systemone` |

Optional `[jev].base_url` overrides the prefix rule (Rock: `EndpointFor` then `Client.BaseURL`).

**Headers.** `Authorization: Bearer <key>`, `Content-Type: application/json`.

**Client limits.** 30s timeout, 1 MiB response cap (`io.LimitReader` 1<<20), HTTP status `>= 300` is an error. Default model `jev-latest`.

**Request.**

```json
{
  "model": "jev-latest",
  "state": "<any JSON context>",
  "questions": {
    "<name>": {
      "type": "noul | choice | score",
      "instructions": "...",
      "criteria": "..."
    }
  }
}
```

Batch named questions in one call. They share `state`.

| Agent mode | Wire `type` | Criteria | Answer shape |
|---|---|---|---|
| boolean | `noul` | omit | `{"type":"noul","noul": <float 0..1>}` |
| choice | `choice` | object label → meaning, ≤255 | `{type, choice, confidence, probabilities}` |
| score | `score` | array of 2–10 level descriptions | `{type, score, confidence, legend, probabilities}` |

There is **no** documented Jev type named `boolean`. Agent-facing boolean is `noul` on the wire.

**Response.** `{"model", "answers": {name: answer}, "usage": {"input_tokens", "output_tokens"}}`. `usage` is optional. An answer object may include `reason` or `explanation`; copy if present, do not invent.

Jev does not open files. File questions mean the harness puts bytes (or a clip) into `state`.

---

## `ask_jev` tool contract (match Rock `ask.go`)

**Register only when `jev_enabled()`.** Plan mode and the default allow list should treat it as read-only (Rock: default allow, plan-mode exception).

**Input.**

```
ask_jev
  state: string | object     facts the questions are about
  questions: [               one or more; one Decide call
    {
      name: string
      question: string       → wire instructions
      mode: boolean | choice | score
      options?: {label: meaning, ...}   # choice
      levels?:  [level, ...]            # score, 2–10
    }
  ]
```

Accept Rock’s aliases where cheap: `mode` `noul` / `yes` / `yesno` → boolean; optional single-question sugar can wait if it bloats the first tool PR.

**Validate in Rust before HTTP** (`ValidateQueries`):

- at least one question
- unique non-empty names
- non-empty `question`
- choice: `options` present, `len <= 255`
- score: `2 <= levels.len() <= 10`
- unknown mode is an argument error (tool `Run` error, no HTTP)

**Result.**

```json
{
  "source": "live",
  "model": "jev-1.x",
  "answers": {
    "<name>": {
      "mode": "boolean | choice | score",
      "value": "...",
      "confidence": 0,
      "probabilities": {},
      "legend": {},
      "reason": "",
      "detail": ""
    }
  },
  "error": "",
  "usage": {"input_tokens": 0, "output_tokens": 0}
}
```

- boolean `value` = noul float. No yes/no threshold in the tool.
- choice `value` = winning key.
- score `value` = numeric score; include `legend` when the API sent one.
- `reason` = `reason` or `explanation` if the JSON has it.

**Hard rule — never fabricate a value.**

| Case | Result |
|---|---|
| Network / timeout / HTTP / decode / missing client | `error` set; every answer has `detail`: `Jev call failed, question not answered`; no `value` |
| Live response missing a named answer | that answer `detail`: `Jev returned no answer`; no `value` |
| Bad arguments | tool error (invalid args), no HTTP |
| No key | tool is **not registered**; this path must not run |

The turn continues after a failed Decide. Failed Jev is an observation, not a loop abort.

---

## Slices

Each slice is independently mergeable. Every slice keeps the no-key path identical to pre-Jev main. Feature flags for (d)(e)(f) default **off** even when a key is present, so a key only enables `ask_jev` until the user opts into extras.

### (a) Jev config + `jev_enabled()`

**What.** Env + file, env wins, one gate.

**Files (expected).**

- `crates/codegen/xai-grok-config-types/src/jev.rs` (new) + re-export from `lib.rs` — `JevConfig { api_key, base_url, model, nudge, nudge_every, safety_check, context_filter, … }`
- `crates/codegen/xai-grok-shell/src/agent/config.rs` — field on typed `Config` (`Config::new_from_toml_cfg`); optional `BoolFlag` for feature gates
- `crates/codegen/xai-grok-config/src/` — deserialize `[jev]` from effective config; **do not** add `jev` to `OVERLAY_ALLOW_PATHS`
- `crates/codegen/xai-grok-jev/src/config.rs` (new crate) or a small module in shell — `jev_key()`, `jev_enabled()`, `JevSettings::from_env_and_file`
- Optional: `grok inspect` line `jev: off` only when we can add it without changing inspect output on the no-key path (prefer skip inspect until a later slice if output would change)

**Precedence.** `JEV_API_KEY` → `TYPESAFE_API_KEY` → `[jev].api_key` → off.

**Tests.**

- empty env + empty file ⇒ `jev_enabled() == false`
- file key only ⇒ enabled; env empty-string does not count
- `JEV_API_KEY` overrides file and `TYPESAFE_API_KEY`
- `TYPESAFE_API_KEY` used when `JEV_API_KEY` unset
- overlay / `GROK_CONFIG` cannot inject a key

**No-key verify.** Unit tests plus `cargo test -p xai-grok-config` (and the new crate). `git grep -n jev` in shipped binaries’ default config should find only docs / optional struct defaults. Do not write a default `config.toml`.

### (b) Minimal Jev client (fail-open)

**What.** Rock `client.go` in Rust: `EndpointFor`, `Decide(state, questions) -> Result<Response, Error>`. Timeouts and body cap as above. No gates. No offline policy. Empty key is an error if someone calls `Decide` — callers must not call it unless `jev_enabled()`.

**Files.**

- `crates/codegen/xai-grok-jev/` — `client.rs`, `types.rs` (`Question`, `Choice`, `Score`, `Noul`, `Response`, `Usage`), helpers `choice_q` / `score_q` / `noul_q`
- `Cargo.toml` workspace member; `reqwest` + `serde_json`

**Tests.** `httptest`-style (wiremock / `axum` test server / `mockito`):

- `jv_live_` prefix → hosted URL; other keys → `api.typesafe.ai`
- `Authorization: Bearer` + JSON body `{model, state, questions}`
- 30s client timeout configured; 1 MiB cap (oversize body is an error)
- status 300+ → error
- missing key → error, no HTTP

**No-key verify.** Client crate is not linked into the pager/shell default path yet, **or** is linked but never constructed. Prefer: shell depends on the crate, constructs `Client` only inside `jev_enabled()` branches that do not exist until (c). After (b) merge, `rg "JevClient|jev::Client" crates/codegen/xai-grok-shell` should be empty except maybe a unused import we should not add.

### (c) `ask_jev` tool, registered only when enabled

**What.** Rock `ask.go` mapping + tool. This is the first user-visible slice.

**Files.**

- `crates/codegen/xai-grok-jev/src/ask.rs` — `Query`, `Answer`, `AskResult`, `validate_queries`, `ask` (failed → `FailedDetail`)
- `crates/codegen/xai-grok-tools/src/implementations/grok_build/ask_jev/` — `AskJevTool` (`Tool` + `ToolMetadata`), `ToolKind::Other` (or a new kind if templates need it), `is_read_only() == true`
- `ToolRegistryBuilder::new` — `register::<AskJevTool>()` in the catalog
- `crates/codegen/xai-grok-agent/src/builder.rs` — push `ToolConfig::for_tool::<AskJevTool>()` only when `jev_enabled()` (same pattern as `web_search_config.is_enabled()`)
- `crates/codegen/xai-grok-agent/src/config.rs` — do **not** add it to `grok_build_core_toolset` unconditionally
- Permission: read-only / default allow; plan mode allowed (`xai-grok-permission-rules` / plan-mode edit gate must not treat it as mutating)
- Optional one-line system-prompt sentence only when enabled (template flag), empty when off

**Tests.**

- `jev_enabled() == false` ⇒ `ask_jev` absent from `Agent::tool_definitions()` and from a finalized grok-build toolset
- enabled + fake HTTP: mixed boolean/choice/score, one batched body, names unique
- 502 / timeout: `error` set, every `detail` is `Jev call failed, question not answered`, no `value`, turn continues
- missing per-question answer: `Jev returned no answer`
- schema errors: empty questions, duplicate names, choice without options, score with 1 level — no HTTP
- plan mode: tool still listed and allowed
- `should_list` / allowlist: a `disallowed_tools = ["ask_jev"]` still works when enabled

**No-key verify.** Existing agent tool-list tests (`tool_list_tests.rs`, builder tests that snapshot names) stay green and must **not** include `ask_jev`. Add an explicit snapshot: default builder with env cleared → names equal to pre-change fixture. Run `cargo test -p xai-grok-agent -p xai-grok-tools --lib`.

### (d) Optional self-validation nudge

**What.** After a successful `search_replace` / write, or an allowed `run_terminal_command`, optionally append a short tool-result or `<system-reminder>` hint: if this was a fix, consider `ask_jev` (boolean: is the failure gone? score/boolean: too risky?) before declaring done. The agent still decides. **No extra `Decide` call.**

Config: `jev.nudge` (default **false** here; Rock defaults on because Jev is identity), `jev.nudge_every` (default 2). `-1` or `nudge = false` disables.

Reuse `Reminder` (`xai-grok-tools` `types/tool.rs`) or a session-actor post-flight in `execute_tool_calls` so it rate-limits across the turn.

**Files.**

- `xai-grok-tools` reminder or `xai-grok-shell` `session/acp_session_impl/tool_calls.rs` post-flight
- `JevConfig` fields already added in (a)
- System prompt sentence only when `jev_enabled() && nudge`

**Tests.**

- nudge off (default) + key set: no extra reminder after edit
- nudge on + every=2: reminder on 2nd successful mutate, not the 1st
- no key: no reminder (even if someone sets `nudge = true` in file)
- file still changes; Risk/permission unchanged
- `ask_jev` after the nudge is a normal tool result

**No-key verify.** Reminder collector tests with env cleared emit the same reminder set as before this slice. No `ask_jev` string in default system prompt (`Agent::system_prompt()` snapshot).

### (e) Optional tool-call safety check (fail-open)

**What.** If `jev_enabled() && safety_check`, after `Decision` is allow (and plan-mode did not deny), ask noul *“Is this tool call destructive or hard to undo?”* for bash / edit / write / MCP (`AccessKind::Bash | Edit | MCPTool`, plus write). Block only when live noul ≥ `jev.risk_block` (default 0.72) **and** `jev.allow_destructive` is false.

Fail-open: any error, timeout, or missing answer ⇒ allow (existing permission already ran). Yolo does not skip an explicit deny if we deny; we should still fail-open on Jev faults so a down Jev cannot freeze always-approve CI.

Rock keeps `Gates.Risk` hard-coded. **We do not.** This slice is off by default.

**Files.**

- `xai-grok-jev` thin `risk_question` helper (same encoder as `ask`)
- `crates/codegen/xai-grok-shell/src/session/acp_session_impl/tool_calls.rs` after permission allow, before `dispatch_observed` (preferred). Alternate: after `GatePreflight::evaluate` in `xai-grok-workspace` `permission/manager/mod.rs` if the check must see the same actor pipeline as yolo/grants.
- config: `safety_check`, `risk_block`, `allow_destructive`

**Tests.**

- flag off: zero HTTP, existing yolo/deny tests unchanged
- flag on + live noul 0.95: deny with a clear tool-result message
- flag on + 502 / timeout: allow, tool runs
- no key: flag ignored, no HTTP

**No-key verify.** `pre_tool_use_decision_tests` / permission tests still pass. New tests force-clear Jev env.

### (f) Optional context filtering (fail-open)

**What.** If `jev_enabled() && context_filter`, optionally noul older tool results / large clips (“does this still help?”). Drop only on live keep=false above `min_confidence`. Errors keep the item. Clip state to stay well under Jev’s ~64k combined / ~32k per-question budget.

Prefer the **per-turn prune path** first (`ChatStateActor::build_conversation_request` / `prune_conversation` in `xai-chat-state` `request_builder.rs`): that is what the next completion sees, including the 50%-window soft-trim. Compaction (`run_compact_inner`) reuses the same prune via `apply_turn_request_pruning`; hook `PreCompact` if you only need an observe point. A grep `KeepSnippet` analog can be a follow-up (f2) if the prune hook stays small.

**Files.**

- `crates/codegen/xai-chat-state/src/actor/request_builder.rs` (`prune_conversation` / post-prune)
- `xai-grok-shell/src/session/compaction.rs` if the compact ladder needs the same filter on verbatim input
- optional `GrepTool` match filter (`xai-grok-tools` grep impl) — only if it stays small

**Tests.**

- flag off: `build_request` / compact payload identical
- flag on + keep=false: older tool result omitted from the next request (still in session log)
- flag on + error: item kept (today’s prune)
- no key: no HTTP, chat-state prune and compact tests unchanged

**No-key verify.** Existing `xai-chat-state` prune tests and compaction unit tests (`session_compact_*`, `xai-grok-compaction`) green with Jev env unset.

---

## Order

```
(a) config + jev_enabled()
  → (b) client
    → (c) ask_jev tool
      → (d) nudge          (needs the tool name in the prompt)
      → (e) safety check   (needs client; independent of c except shared crate)
      → (f) context filter (needs client; independent of c)
```

(d), (e), (f) can land in any order after (b)/(c) as needed; (e) and (f) do not require the tool, only the client + gate. Do not merge (c) before (a)+(b). Do not sneak a hard-coded `Decide` into (d).

---

## No-key path (every slice)

Checklist for reviewers:

1. `JEV_API_KEY` and `TYPESAFE_API_KEY` unset; no `[jev].api_key`.
2. `jev_enabled() == false`.
3. `ask_jev` not in `tool_definitions()`.
4. No Jev HTTP client constructed on the session/tool hot path.
5. System prompt / inspect / TUI status unchanged unless a slice documents a default-off additive (prefer zero output change).
6. `cargo test` for the crates touched.

---

## Open questions

Resolved from Rock + public docs (do not re-litigate):

- Endpoint/auth/body/types: section above. Hosted vs official differs by URL and key prefix; keep `EndpointFor`.
- Agent boolean = wire `noul`.
- Failure strings: `Jev call failed, question not answered` / `Jev returned no answer`.

Resolved in the slices that shipped:

- `ToolKind` for `ask_jev` is `Other` (`is_read_only()` overridden). A dedicated kind is only needed if templates should say `${{ tools.by_kind.ask_jev }}`.
- `grok inspect` does not print `jev: off` (would change no-key inspect text).
- (f) did **not** run inside sync `prune_conversation` (chat-state has no HTTP). The shell filters the request clone after `build_request`, using the same older-tool-result age walk. Compaction / grep `KeepSnippet` are **f2**.
- Crate name is `xai-grok-jev` (isolated client). Tools re-export settings/helpers; the client is constructed only when a key is present.

Still open (not required for optional-tool usefulness):

- TUI `◇ jev` marks (Rock slice (d)); add only if the pager has a cheap tool-row summary.
- (f2) same `keep_snippet` on compaction input and/or grep hits.

Could not find / did not need:

- Rock docs were on `main` at the stated paths; Go sources are current on default branch (SHA moved past `605cad9` but `client.go` / `ask.go` / `gates.go` match the lead’s contract). Nothing material was missing.
- No Jev code exists in this fork today (`rg jev` was empty before these docs).
