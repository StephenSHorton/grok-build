# Jev speed plan

North star: make the Grok Build CLI **200× faster** with Jev. Jev is a typed judge (boolean / choice / score) that answers in **~100–400ms**. A grok-build model sample is typically **seconds** (TTFT plus decode, plus any reasoning tokens). That gap is the whole thesis: offload enumerable decisions so the large model does fewer, shorter turns.

`ask_jev` as a model-called tool is not 200× by itself. It still costs a tool round and usually another sample. The large wins are **harness-side** Decide calls that **skip a model round-trip**. This doc is how we measure that, where time goes today, and which skips are worth building.

User-facing Jev: [`README.md`](README.md). Insertion points: [`harness-map.md`](harness-map.md). Cheap bench: [`bench/run.sh`](bench/run.sh).

---

## 1. Reproducible benchmark

Without a fixed task set and the same meters, “200×” is a slogan.

### Tasks

Each task is a fresh temp git repo plus one headless prompt. The script in `bench/run.sh` builds the fixture, runs `grok -p`, and writes a JSON row. Do not reuse sessions (`--fork-session` / new `--session-id` each run). Pin `--model` and `--reasoning-effort`.

| Id | Prompt shape | What it stresses |
|---|---|---|
| `trivial-status` | “What is the git status?” | Routing: should not need a long reasoning turn. |
| `find-symbol` | “Where is `charge_invoice` defined?” (~20 files, one hit) | File pick vs glob→grep→read wander. |
| `one-line-fix` | Failing unit test, obvious off-by-one | Short mutate + done/validation. |
| `pick-among-fixes` | Three comments, one correct patch | Choice among candidate fixes. |
| `relevance-triage` | Grep-bait: same word in 8 files, one real bug | Search-hit classification. |
| `already-done` | Test already passes; “make the test pass” | Early-done / stop-the-loop. |
| `multi-file-rename` | Rename a function across 4 files | Plan pick + which files to edit. |

Skip `rm -rf` / safety tasks here. Those measure deny quality, not speed.

Run each task **Jev off** (`JEV_API_KEY` / `TYPESAFE_API_KEY` unset, no `[jev].api_key`) and **Jev on** (key present; nudge on, `safety_check` / `context_filter` off unless the row says otherwise). Repeat ≥3 times. Compare medians, not single runs.

### Meters

Wall-clock is the scoreboard. The rest explain *why*.

| Meter | How |
|---|---|
| Wall-clock | `/usr/bin/time -p` around `grok -p` (script records `elapsed_s`). |
| Model samples | Count `loop_index` / `shell.turn.inference_done` in unified logs, or `tool_use` cycles in `--output-format streaming-json`. One inner-loop iteration in `process_conversation_turn_inner` = one sample. |
| Tool calls | Count tool-call items in the stream, by name (`read_file`, `grep`, `ask_jev`, …). |
| Tokens | `prompt_tokens`, `completion_tokens`, `reasoning_tokens`, `cached_prompt_tokens` from `shell.turn.inference_done` (see `turn.rs` after `run_turn_via_sampler`). |
| TTFT / model ms | Same log: `ttft_ms`, `model_elapsed_ms`. |
| Jev | `/jev-stats` session totals and `$GROK_HOME/jev_stats.jsonl` (`DecideRecord`: `source`, `latency_ms`, `ok`, `question_count`). Off run must have **zero** Decide rows. |
| Outcome | Task-specific check (test exit 0, expected file hash, expected symbol path). Speed without a pass is a skip, not a win. |

Headless flags the script uses: `grok -p --output-format streaming-json --max-turns 24 --no-subagents --disable-web-search --permission-mode yolo` (or `dontAsk` if yolo is unavailable). Isolate `GROK_HOME` per run so stats and config do not leak.

### What “200×” would look like

If Jev-off median wall-clock for `one-line-fix` is 80s and Jev-on is 0.4s, that is 200×. We will not see that from prompt text alone. Publish the table (task × mode × median seconds / samples / tokens / tools / pass-rate) before claiming any factor.

---

## 2. Where the time goes today

A user `Prompt` is three nested loops ([`harness-map.md`](harness-map.md) §2): `handle_turn_input` → outer continuation → inner `process_conversation_turn_inner` (`crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs`). Sampling is `run_turn_via_sampler` (`sampler_turn.rs`). Tools re-enter only as conversation items, then another `Complete`.

Rough share of wall-clock on a typical coding prompt (order-of-magnitude, not a measurement — the bench is how we replace this guess):

| Bucket | Typical | Code path |
|---|---|---|
| **Model sample** | **Most of the turn.** Seconds each: TTFT + decode + reasoning tokens. 5–40 samples per task is common. | `run_turn_via_sampler` → `submit_and_collect_with_metadata`. Logged as `shell.turn.inference_done` (`model_elapsed_ms`, `ttft_ms`, token counts). |
| **Sample count** | Multiplies the row above. Each tool result forces another sample. | Inner loop in `process_conversation_turn_inner`; `tool_turn_count` / `max_turns`. Parallel tools still wait for the next sample after the batch. |
| **Context size** | Grows TTFT and cost. System prompt + tools + history. | `ChatStateHandle::build_request` → `ChatStateActor::build_conversation_request` (`xai-chat-state` `actor/request_builder.rs`). Soft-trim / hard-clear of old tool bodies when usage **> 50%** of the window (`PruningConfig`: keep last 3 user turns, soft-trim >4000 chars, hard-clear after 10). Optional Jev `context_filter` runs **after** that, on the request clone (`apply_jev_context_filter` in `turn.rs`), default **off**. |
| **Compaction** | Occasional **extra full model call** (up to 300s budget). | `CompactionPolicy` default **85%** (`xai-grok-agent` `compaction.rs`). `check_auto_compact_needed` / `run_compact_inner` (`session/compaction.rs`). Optional two-pass + memory flush. Compact-on-error and model-switch compact also exist. |
| **Tool execution** | Often small vs the sample (read/grep tens of ms). Shell, tests, MCP, and media-gen can dominate a single iteration. | `execute_tool_calls` → `execute_tool_calls_batch` (`tool_calls.rs`). Same-file edits serialize; otherwise a batch can run concurrently, then **one** next sample. Permission / PreToolUse / plan-mode sit in `prepare_tool_call` before dispatch. |
| **Prep / bookkeeping** | Usually tens–hundreds of ms. | `prepare_tool_definitions_timed` (MCP wait), `build_request_ms`, image budget, doom-loop / identical-call detectors. Not the 200× lever. |

`ask_jev` today is a **tool** in that loop. Best case it shortens reasoning tokens in the sample that called it and avoids later wasted samples. Worst case it adds 100–400ms plus another sample that just reads the answer. The prompt rewrite ([#17](https://github.com/StephenSHorton/grok-build/pull/17)) pushes the model toward the first case. It cannot skip the sample that decided to call.

---

## 3. Ranked harness-side opportunities

These replace or skip a **model** round-trip. Model-called `ask_jev` stays useful for in-turn judgment; it is not in this ranking.

Estimates are **honest ranges** for wall-clock on the tasks they apply to, assuming fail-open (timeout / HTTP / missing answer ⇒ current behavior). They are not promises.

| Rank | Skip | Where to hook | Est. speedup | Risk |
|---|---|---|---|---|
| 1 | **Pick files / context up front** instead of glob→grep→read wander | After first `grep` / `glob` / `list_dir` result, before the next `Complete`: choice over hit paths (extend `maybe_keep_snippet` / filter **f2**). Or a pre-sample “which paths belong in this request?” | **2–4×** on `find-symbol`, `relevance-triage`, `multi-file-rename`. Those tasks are mostly exploratory reads. | Medium. Wrong file looks like a silent miss. Cap candidates, keep the raw hit list in the session log, fail-open. |
| 2 | **Done / validation that ends the loop** | After a successful edit/write/allowed shell (`JevNudgeReminder` already fires here — today it is **text only**). Noul: “is the user request satisfied / is the failure gone?” High confidence ⇒ `TurnOutcome` end instead of another sample. | **1.5–3×** on `one-line-fix` and `already-done`. Agents often re-read and polish. | Medium-high. False “done” is a product bug. Require a concrete check in `state` (test output, diff hunk). Low confidence ⇒ continue. Start as a stronger nudge; only auto-end after the bench says the noul is calibrated. |
| 3 | **Choose the next tool (or none) without a sample** | Tail of `execute_tool_calls` when the batch is a read-only search/read and the next action is enumerable. Choice over `{read, grep, edit, stop, ask_user}`. | **1.5–3×** if it removes ~⅓–½ of samples. Compounds with (1). | High. Wrong tool loops or mutates. Start **read-only** (never auto-edit). Identical-call / stationarity detectors stay. |
| 4 | **Cheaper model per sample** | Before `run_turn_via_sampler`, score “how hard is this step?” Easy → flash / low effort; hard → current model. `maybe_compact_on_model_switch` already exists. | **2–5×** if most inner-loop samples are easy (read-next, format, “run the test”). Little help if every step is a hard design call. | High. Quality cliff. Pin the hard model for the first sample and for any mutate plan. Measure pass-rate on the bench, not only seconds. |
| 5 | **Route trivial requests** | First sample of `handle_turn_input`, before tools: choice `{direct_answer, one_tool, full_agent}`. `trivial-status` / “what time is it” / “print cwd”. | **10–50× on those turns**, **~1.0–1.2×** on real coding tasks (they are rare in the set). | Low if conservative (default `full_agent`). High if it swallows a real bug-fix prompt. |

### Also-rans (do not rank in the top 5)

| Skip | Why it is not top 5 |
|---|---|
| **Filter search / grep hits** (keep/drop on the request clone; grep `KeepSnippet`) | Real, fail-open, already half-built (`context_filter`, default off). Maybe **1.2–2×** on search-heavy tasks via smaller prompts, not fewer samples. Do it — it is cheap — but it is not a 200× lever. |
| **Decide whether to compact** | Compaction is already 85% + optional two-pass. Jev might skip a 300s-budget summary or fire earlier to cheapen later samples. **1.1–1.5×**, and a bad “don’t compact” blows the next window. |
| **Skip redundant re-reads** | Same-path `read_file` with unchanged mtime → serve the cached body, no sample. Nice; overlaps (1) and (3). |
| **`ask_jev` from the model** | Necessary product, not a harness skip. Optimistic **1.1–2×** if the prompt rewrite actually cuts reasoning and later wander. Can **regress** if the model calls it and then restates the answer. |
| **`safety_check`** | Adds 100–400ms per mutate. Correctness, not speed. |

---

## 4. How far toward 200×?

**Jev alone will not get us 200× on real coding tasks.** Arithmetic: a 2-minute Jev-off run would have to finish in 0.6s. That is less than **one** current model sample. The only way there is to **almost never call the large model**.

A plausible stack, stacked on this fork:

| Layer | What it does | Combined (guess) |
|---|---|---|
| Prompt + `ask_jev` (landed) | Shorter reasoning, fewer dumb branches, **if** the model obeys. | **1.1–2×** |
| Top-5 harness skips (this doc) | Delete exploratory samples; stop when done; cheap model on easy steps. | **5–15×** on search/fix tasks that are mostly wander; **~2–4×** on already-tight tasks. |
| Index / repo map up front | So (1) is not guessing from a cold tree. | Extra **1.5–3×** on large repos. |
| Cache + speculative tools | Don’t re-read; start the test before the model asks. | Extra **1.2–2×**. |
| Tiny / local model for most samples | Large model only for the hard 10%. | Extra **3–10×** — this is the rest of the 200×, and it is **not Jev**. |

**Honest ceiling for Jev-shaped work (typed decide, 100–400ms, fail-open): about 10× on messy tasks, 2–4× on tight ones, plus a 10–50× special case for trivia.** Getting from 10× to 200× is model routing, caching, and not sampling. Jev is the cheap classifier those layers need; it is not the layers.

What else is required, besides more Decide hooks:

1. **The bench table**, published, or we will optimize vibes.
2. **Calibration** — noul thresholds for “done” and “this file matters” need labeled traces, not one magic constant.
3. **A fast model path** that does not go through the full grok-build sample (or uses a much smaller model). Without that, one remaining `Complete` caps the speedup.
4. **Repo context that is not exploratory reads** (index, embeddings, or a cheap tree digest at session start).
5. **Product discipline** — auto-end and auto-edit are how you get 200× *and* how you ship wrong patches. Fail-open, read-only first, measure pass-rate.

Next concrete work, in order: run `bench/run.sh` on the seven tasks (off vs on) and paste the table into this file; then implement rank 1 as an optional, fail-open, read-only hit picker (slice **f2**-shaped). Do not auto-end turns until the `already-done` / `one-line-fix` pass-rate is in the table.
