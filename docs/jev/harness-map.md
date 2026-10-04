# Grok Build harness map (Jev insertion points)

Slice 0 reading of this fork (`StephenSHorton/grok-build`, main at the time of writing). Paths and type names are for this tree. This is a map, not a design. The implementation plan is [`plan.md`](plan.md).

Jev is not in this tree yet. The goal of later slices is to add it as a **strictly optional** feature: no key means no tool, no HTTP, no errors, behavior identical to upstream.

---

## 1. Tool system

### Definition

Every built-in tool implements two traits:

| Trait | Crate | Role |
|---|---|---|
| `xai_tool_runtime::Tool` | `crates/common/xai-tool-runtime/src/tool.rs` | Typed `Args` / `Output`, `id()`, `description()`, `run` or streaming `execute` |
| `ToolMetadata` | `crates/codegen/xai-grok-tools/src/types/tool_metadata.rs` | `kind()`, `tool_namespace()`, `description_template()`, optional `requires_expr()`, `is_read_only()`, `lock_path_param()` |

`Tool::should_list(&ListToolsContext) -> bool` (`xai-tool-runtime`) is a per-turn listing predicate. Default is `true`. Return `false` to hide the tool from the model-facing manifest for that turn.

`ToolDefinition` / `FunctionTool` (`crates/common/xai-tool-types/src/definition.rs`) is the Chat Completions function-tool schema sent to the model: `{ "type": "function", "function": { name, description, parameters } }`. Schemas are generated from `Tool::Args` via `schemars::JsonSchema` at `ToolRegistryBuilder::register` (`generate_schema_cached`).

`ToolKind` (`crates/codegen/xai-grok-tools/src/types/tool.rs`) classifies tools (`Read`, `Edit`, `Execute`, `AskUser`, `WebSearch`, …, `Other`). Used for permission `AccessKind`, prompt templates (`${{ tools.by_kind.<kind> }}`), and capability-mode filtering. `ask_jev` will need a kind (likely `Other` unless a dedicated variant is added).

`ToolNamespace::GrokBuild` produces qualified ids such as `"GrokBuild:read_file"`. Client-facing names can differ (`run_terminal_command`, `spawn_subagent`) via `ToolConfig::name_override`.

### Registration

`ToolRegistryBuilder::new()` (`crates/codegen/xai-grok-tools/src/registry/types.rs`) registers every built-in type it knows (`register` / `register_with_params`). That is the catalog, not the session toolset.

The session toolset is a `ToolServerConfig { tools: Vec<ToolConfig> }` built by `xai-grok-agent`:

- `default_grok_build_toolset()` / `grok_build_core_toolset_with` (`crates/codegen/xai-grok-agent/src/config.rs`) — core list: bash, read, search_replace, list_dir, grep, task/lifecycle, scheduler, monitor, `search_tool` / `use_tool` (MCP meta), goals, optional workflow / feedback, plus this fork’s session-bus tools.
- `workspace_grok_build_toolset()` — core plus write, plan-mode enter/exit, ask-user, web search/fetch, and other mode-injected tools.
- `AgentBuilder` (`crates/codegen/xai-grok-agent/src/builder.rs`) then **conditionally pushes** more `ToolConfig`s when `definition.inject_default_tools` is set: memory tools if a backend exists, `WebSearchTool` / `WebFetchTool` if those configs are enabled, image/video gen if those configs are enabled, `OpenCodeWriteTool` if `write_file_enabled`, plan-mode tools for the primary audience, `AskUserQuestionTool` if `ask_user_question_enabled`.

`AgentBuilder` also applies `tools:` allowlists and `disallowed_tools` denylists via `listed_tools` (`crates/codegen/xai-grok-agent/src/tool_list.rs`).

Out-of-tree packs can call `register_tool_pack` (`xai-grok-tools` registry) or `register_toolset_preset` (`xai-grok-agent` config). Prefer in-tree registration for Jev so the no-key path never loads a pack.

`ToolRegistryBuilder::finalize` evaluates each tool’s `requires_expr()` against the proposed toolset (`Expr<ToolRequirement>` in `crates/codegen/xai-grok-tools/src/types/requirements.rs`). A failed requirement drops the tool. Default is `Expr::True`.

### Schema exposure

`Agent::tool_definitions()` → `ToolBridge::tool_definitions()` → `FinalizedToolset::tool_definitions()` returns `Vec<ToolDefinition>`. Those go on the sampling request as `tools[]` (`xai-grok-sampling-types` `ToolSpec` / `ToolDefinition`). Hosted backend-search tools are a separate `HostedTool` list, not `ToolSpec`s.

MCP tools are discovered at runtime and either listed directly or reached through `search_tool` / `use_tool`.

### How to register a tool only when Jev is on

Existing patterns, in recommended order for `ask_jev`:

1. **Builder gate (best match).** Same as `web_search_config.is_enabled()` / `video_gen_config.is_enabled()` / `ask_user_question_enabled` in `AgentBuilder`: push `ToolConfig::for_tool::<AskJevTool>()` only when `jev_enabled()`. When the gate is false, the tool is never in `ToolServerConfig`, never in `tools[]`, never dispatched.
2. **`Tool::should_list`.** Hide a registered tool from the manifest for a turn. The type is still in the registry. Prefer (1) so a disabled Jev tool is not registered at all.
3. **`requires_expr`.** Dependency on other tools/params at finalize. Not a feature flag.
4. **Allow/deny lists.** User/agent `tools:` / `disallowed_tools`. Complementary, not the master switch.

Do **not** register `ask_jev` and then error at call time when no key is set. The no-key path must not advertise the tool.

### Dispatch

`SessionActor::execute_tool_calls` (`crates/codegen/xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`) → `prepare_tool_call` (parse, PreToolUse, plan-mode, permission) → `dispatch_observed` (`tool_dispatch.rs`) → `WorkspaceOps::call_tool_with_context` → `ToolDispatch::call` / `call_terminal` (`crates/common/xai-tool-runtime/src/dispatch.rs`) → typed `Tool::execute` / `run`.

Results become `ConversationItem::tool_result` and are pushed onto chat state for the next completion.

Post-execution `Reminder`s (`xai-grok-tools` `types/tool.rs`) can append system-reminder text after a tool result. That is the closest existing hook to Rock’s self-validation nudge.

---

## 2. Agent loop

### Objects

- `Agent` (`crates/codegen/xai-grok-agent/src/agent.rs`) — definition, rendered system prompt, `ToolBridge`, `CompactionPolicy`, `ReminderPolicy`. It does **not** run the turn loop.
- `SessionActor` (`crates/codegen/xai-grok-shell/src/session/acp_session_impl/`) — the loop: sample → tool calls → feed results → sample again.
- Turn body: `crates/codegen/xai-grok-shell/src/session/acp_session_impl/turn.rs`.
- Subagents reuse the same actor/tool path; they inherit the parent’s PreToolUse hooks (`crates/codegen/xai-grok-shell/src/agent/subagent/mod.rs`).
- Headless `-p`, TUI, and ACP (`grok agent stdio` / `serve`) all enter this session actor.

### Turn lifecycle

1. User (or injected) message is appended to chat state.
2. Optional preflight overflow check: `check_preflight_overflow` / `run_compact_only` if the estimated prompt is over the window.
3. System prompt is the cached `Agent::system_prompt()` (re-rendered by `finalize_prompt` / `PromptContext::render` when tools or context change).
4. Sampler streams a completion (`sampler_turn.rs`). Tool calls arrive as `ToolCall` / `ToolCallResponse`.
5. Repeat / doom-loop detectors (`identical_tool_calls`, `NUDGE_AFTER_IDENTICAL_TOOL_CALLS = 8`, `MAX_CONSECUTIVE_IDENTICAL_PROBLEMATIC_TOOL_CALLS`) may nudge or stop. This is **not** Jev.
6. Phase → `ToolExecution`. `execute_tool_calls` runs the batch (plan-exit tools are split to the tail).
7. Each call: prepare → permission → dispatch → post-flight (reminders, skill announcements, deferred followups).
8. Results are `push_tool_result`’d. `ToolLoop::Continue` loops back to another sample. `PermissionReject`, `Cancelled`, `FollowupMessage`, `HookDenied`, `MaxTurnsReached` stop or divert.
9. `max_turns` (if set) stops after N tool rounds.

A new tool result re-enters the loop only as a conversation item. There is no side channel that bypasses the next `Complete`.

---

## 3. Permission and safety

### Where a call is approved or denied

Order inside `SessionActor::prepare_tool_call` (`tool_calls.rs`), then dispatch:

1. **Parse / existence.** Unknown MCP names and JSON parse failures become tool errors, not permission prompts.
2. **`PreToolUse` hooks** (`apply_pre_tool_use_gate`). `HookEventName::PreToolUse` in `crates/codegen/xai-grok-hooks/src/event.rs`. Hooks can allow, deny, ask, rewrite args, or defer to the normal permission flow. Config, plugin, and ACP client hooks all feed this gate.
3. **Plan-mode edit gate** (`plan_mode_edit_gate`). Mutating tools are rejected in plan mode except the plan file itself (`plan_file_auto_approve`).
4. **Permission manager.** `AccessKind` + `PermissionRequest` → `PermissionHandle::request` → `Decision` (`crates/codegen/xai-grok-permission-rules/src/types.rs`): `Allow`, `Ask`, `FollowupMessage`, `Reject`, `PolicyDeny`, `Cancelled`.
5. **Modes** (docs: `crates/codegen/xai-grok-pager/docs/user-guide/22-permissions-and-safety.md`): `ask` (default), `acceptEdits`, `auto`, `dontAsk`, `bypassPermissions` / always-approve (`--yolo`). Deny rules and hooks still apply under yolo.
6. **Config rules.** `[permission]` `PermissionConfig` / `PermissionRule` (`crates/codegen/xai-grok-config-types/src/permission.rs`): `allow` / `ask` / `deny` with `ToolFilter` (`Any`, `Bash`, `Edit`, `Read`, `Grep`, `Mcp`, `WebFetch`, `AgentMessage`) and glob/domain patterns.
7. **Sandbox.** OS-level seatbelt / Landlock (`xai-grok-sandbox`, `xai-grok-workspace` `permission/sandbox_gate.rs`). Independent of Jev.
8. **`exec_risk` / bash splitting** (`xai-grok-permission-rules`). Built-in destructive-ish shell classification. Not a Jev call.

`AccessKind::Tool(String)` is the catch-all for unclassified mutating tools (every call prompts). A read-only `ask_jev` should **not** land there; treat it like other read-only tools (default allow, allowed in plan mode), matching Rock.

### Hook points for an optional Jev safety check

Best insertion: **after** the existing decision is not already deny, **before** dispatch — same moment Rock calls `Gates.Risk` (Rock: after allow/ask/deny, only if not already deny). In this tree that is the tail of `prepare_tool_call` once `Decision::Allow` (or auto-approved `Ask`) is known, still inside the `tool.decision` span.

Fail-open: timeout, HTTP error, or missing answer must not block the call. Only a live, decoded noul above a configured threshold (and only when the feature is on) may deny.

Do **not** add a hard-coded Rock-style Risk gate that always runs. Yolo already does not skip deny rules; Jev must not become a second always-on block.

---

## 4. Context management

### What goes into the prompt

`PromptContext` (`crates/codegen/xai-grok-agent/src/prompt/context.rs`) is the structured input. `PromptContext::render` / `ToolBridge::render_prompt` fills MiniJinja templates (`TemplateRenderer` in `xai-grok-tools`).

Typical sections:

- Base template (`prompt/template.rs`; compact variant for subagents / post-compaction via `COMPACT_SYSTEM_PROMPT`).
- `prompt_body` (agent markdown, orchestrator addendum).
- AGENTS.md / project rules (`prompt/agents_md.rs`), personas, skills listings (`prompt/skills.rs`).
- Memory section when `memory_enabled` / `memory_v2_enabled`.
- Tool-name placeholders from the finalized toolset (`${{ tools.by_kind.read }}`, …).
- User-message wrappers (`prompt/user_message.rs`).
- Runtime `<system-reminder>` blocks (`system_reminder.rs`, tool `Reminder`s, identical-call nudges).

The conversation itself is chat-state history (user / assistant / tool results), not the system prompt.

### Compaction

- Policy: `CompactionPolicy` (`crates/codegen/xai-grok-agent/src/compaction.rs`). Default auto-compact threshold **85%** of the model context window. Optional two-pass, optional memory flush, 300s wall-clock budget.
- Trigger: `SessionActor::should_auto_compact` / `check_auto_compact_needed` / `run_compact_only` (`crates/codegen/xai-grok-shell/src/session/compaction.rs`). Also mid-turn `check_preflight_overflow` after tool rounds, and compact-on-context-overflow errors (`should_compact_on_error`).
- Implementation helpers: `crates/codegen/xai-grok-shell/src/session/helpers/session_compact.rs`, `xai-chat-state` compaction utils, `crates/common/xai-grok-compaction`, `crates/codegen/xai-compaction-transcript`.
- Compaction is a summarization sample (`SELF_SUMMARIZATION_PROMPT`), not a snippet keep/drop loop. There is **no** Rock `KeepSnippet` equivalent on grep today.

### Truncation

- Tool results: `DEFAULT_TOOL_OUTPUT_BYTES = 40_000`, `DEFAULT_TOOL_OUTPUT_CHARS = 20_000` (`xai-grok-tools/src/lib.rs`).
- Helpers: `crates/codegen/xai-grok-tools/src/util/truncate.rs` (`truncate_line`, `truncate_str`, preview + marker), `util/mcp_truncate.rs` (MCP inline cap / `GROK_MAX_MCP_OUTPUT_BYTES`).
- Terminal / background task output is independently size-capped (`xai-grok-shell-terminal`).

### Where a Jev filter could run (fail-open)

1. **Compaction input selection** — before `run_compact_only` builds the summarization payload (`helpers/prepared_compaction_history.rs`, `compaction.rs`). Ask noul/choice “keep this older tool result / file clip?” and drop only on a live yes-to-drop. Errors keep the item.
2. **Tool-result truncation** — `truncate.rs` / MCP truncate, when a result is about to be clipped. Less useful than (1); truncation already keeps a head/preview.
3. **Grep hit filter** — Rock’s `KeepSnippet` analog would sit in `GrepTool` after each match. Highest token leverage for search dumps; not compaction. Offline/error must keep the hit.

There is no existing relevance scorer to hook. Any filter is new code behind `jev_enabled()` plus an explicit feature flag.

---

## 5. Configuration

### Files and location

Home: `$GROK_HOME` or `~/.grok` (`xai_dirs::grok_home` / `resolve_grok_home`, `crates/codegen/xai-dirs`).

| File | Loader | Role |
|---|---|---|
| `$GROK_HOME/config.toml` | `load_from_disk` / `USER_CONFIG_FILENAME` | User config |
| `$GROK_HOME/managed_config.toml` | `load_managed_config` | Org defaults (below user) |
| `/etc/grok/managed_config.toml` | `load_system_managed_config` | System managed (lowest file tier) |
| `requirements.toml` / MDM | `load_requirements` | Org clamp (highest file tier) |
| `$GROK_HOME/sandbox.toml` | sandbox loader | Sandbox profiles |
| Project `.grok/` | agent discovery, not the user-tier merge | Agents, local overlays; must not be promoted to user tier |

`ConfigLayers::load` / `load_effective_config` (`crates/codegen/xai-grok-config/src/config_layers.rs`, `effective_config.rs`) deep-merge lowest → highest:

`system managed` → `managed` → `user config.toml` → `GROK_CONFIG` / `GROK_CONFIG_PATH` overlay → requirements / MDM.

`$VAR` expansion happens in `load_toml_file` (`loader.rs`). The overlay is an allowlisted soft-settings merge (`env_overlay.rs`); it must not become a permission-escalation or secret-injection path. **Do not** put `jev.api_key` on the overlay allowlist.

User-facing precedence (CLI > env > requirements > overlay > config.toml > managed > defaults) is documented in `crates/codegen/xai-grok-pager/docs/user-guide/05-configuration.md`.

### Env vars and API keys

LLM auth is **not** a `[models] api_key` field the way a small TOML app would do it:

- `XAI_API_KEY`, fallback `GROK_CODE_XAI_API_KEY` — `xai_grok_login::auth_method::read_xai_api_key_env` (`crates/codegen/xai-grok-login/src/auth_method.rs`).
- `grok login` / cached session token / `auth.json` `xai::api_key`.
- Per-model BYOK and `[auth_provider.<name>]` command providers (`crates/codegen/xai-grok-config-types/src/auth_provider.rs`).
- Tools that call HTTP use `ApiKeyProvider` (`crates/codegen/xai-grok-tools/src/types/api_key_provider.rs`).

Jev must **not** reuse `XAI_API_KEY`. Use `JEV_API_KEY` then `TYPESAFE_API_KEY`, plus an optional config-file field, with env winning.

### Where a `[jev]` section should live

Add a small `JevConfig` struct next to other leaf config types (`xai-grok-config-types` or a new `xai-grok-jev` crate that only the shell/agent depend on). Parse it from the **effective** config so file layers merge, then overlay env:

1. `std::env::var("JEV_API_KEY")` if non-empty.
2. Else `TYPESAFE_API_KEY` if non-empty.
3. Else `[jev].api_key` from the merged TOML if non-empty.
4. Else no key.

`jev_enabled()` is true iff the resolved key is non-empty (after trim). Feature flags (`nudge`, `safety_check`, `context_filter`) are additional ANDs on top of `jev_enabled()`. Default those flags **false** so a key only turns on `ask_jev` until a later slice opts in.

Security gates that must not read `GROK_CONFIG` overlay should read the overlay-free `effective_config_base_without_overlay` for anything that could change trust. A Jev API key is a secret: prefer env; if the file field exists, keep it out of overlay allowlists and out of logs (`xai-grok-secrets` already treats `api_key` as sensitive).

---

## 6. Fork-only notes

This fork is 19 commits ahead of `xai-org/grok-build` main (merge-base `2bdd1d6a`). Extra surface that later slices should not break:

- Session-bus tools (`SessionsList` / `Claim` / `Open` / `Close` / `Release` / send) registered from `session_bus_tools()` in `xai-grok-agent` `config.rs`.
- Suzuri pane `/fork` and `sessions_open` / `sessions_close`.
- MCP channel notifications injected as same-session turns.
- `grok-fork` launcher scripts.

Conditional `ask_jev` registration must not disturb those toolset helpers.
