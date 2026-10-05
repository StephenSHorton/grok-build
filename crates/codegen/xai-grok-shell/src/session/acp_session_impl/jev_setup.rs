//! `/jev-setup` builtin: persist `[jev]`, then live-refresh tools + the `<jev>` section.

use super::*;
use crate::sampling::ConversationItem;
use crate::session::commands::AdvertiseTrigger;
use crate::util::config::{
    JevFlag, JevSetupRequest, clear_user_api_key, collect_status_from_file, env_key_is_live,
    format_help, format_status, jev_stats_log_path, save_user_api_key, set_user_flag,
    validate_key_recorded,
};
use xai_grok_tools::implementations::grok_build::{
    ASK_JEV_TOOL_NAME, AskJevTool, JevClient, JevMetrics,
};

impl SessionActor {
    pub(super) async fn execute_jev_setup(
        self: &Arc<Self>,
        request: Result<JevSetupRequest, String>,
    ) -> String {
        let request = match request {
            Ok(request) => request,
            Err(err) => return format!("{err}\n\n{}", format_help()),
        };
        match request {
            JevSetupRequest::Status => self.jev_setup_status_text(),
            JevSetupRequest::Help => format_help(),
            JevSetupRequest::Set { key, force } => {
                let Some(key) = key else {
                    return "Usage: /jev-setup set [--force] <api-key>\nThe TUI command /jev-setup set opens a masked prompt instead of taking the key on the slash line.".to_string();
                };
                self.jev_setup_save_key(&key.0, force).await
            }
            JevSetupRequest::Off => self.jev_setup_off().await,
            JevSetupRequest::Flag { flag, on } => match set_user_flag(flag, on).await {
                Ok(()) => {
                    let mut settings = load_typed_config()
                        .jev_settings()
                        .or_else(|| self.rebuild_spec.jev_settings.get());
                    if let Some(ref mut settings) = settings {
                        match flag {
                            JevFlag::Nudge => settings.nudge = on,
                            JevFlag::SafetyCheck => settings.safety_check = on,
                            JevFlag::ContextFilter => settings.context_filter = on,
                            JevFlag::Route => settings.route = on,
                            JevFlag::FilePick => settings.file_pick = on,
                            JevFlag::DoneCheck => settings.done_check = on,
                        }
                    }
                    let apply = self.apply_jev_settings(settings).await;
                    format!(
                        "{} is {}.\n{}\n\n{}",
                        flag.name(),
                        if on { "on" } else { "off" },
                        flag.explain(),
                        apply
                    )
                }
                Err(err) => err,
            },
            JevSetupRequest::Apply => self.apply_jev_settings_to_session().await,
            JevSetupRequest::Stats => self.execute_jev_stats().await,
        }
    }

    pub(super) async fn execute_jev_stats(&self) -> String {
        let jev_on = self
            .rebuild_spec
            .jev_settings
            .get()
            .is_some_and(|s| s.is_enabled());
        let toolset = self.tool_bridge_handle().toolset();
        let resources = toolset.resources.lock().await;
        if let Some(metrics) = resources.get::<JevMetrics>() {
            metrics.format_report(jev_on)
        } else {
            let all_time =
                xai_grok_jev::load_all_time(&jev_stats_log_path()).filter(|acc| !acc.is_empty());
            xai_grok_jev::format_stats_report(
                &xai_grok_jev::StatsAccumulator::default(),
                all_time.as_ref(),
                jev_on,
            )
        }
    }

    fn jev_setup_status_text(&self) -> String {
        let cfg = load_typed_config();
        let mut text = format_status(&collect_status_from_file(&cfg.jev));
        text.push_str("\n\n");
        text.push_str(&format_help());
        text
    }

    async fn jev_setup_save_key(self: &Arc<Self>, key: &str, force: bool) -> String {
        if key.trim().is_empty() {
            return "Refusing to save an empty key.".to_string();
        }
        match validate_key_recorded(key).await {
            (Ok(()), record) => {
                if let Some(record) = record {
                    self.record_jev_decide(record).await;
                }
            }
            (Err(err), record) if force => {
                if let Some(record) = record {
                    self.record_jev_decide(record).await;
                }
                if let Err(save_err) = save_user_api_key(key).await {
                    return save_err;
                }
                let apply = self
                    .apply_jev_settings(settings_after_saving_key(key))
                    .await;
                return format!(
                    "Saved [jev].api_key even though validation failed:\n{err}\n\n{apply}"
                );
            }
            (Err(err), record) => {
                if let Some(record) = record {
                    self.record_jev_decide(record).await;
                }
                return format!(
                    "{err}\nNot saved. Re-run with --force if you want to store this key anyway."
                );
            }
        }
        if let Err(err) = save_user_api_key(key).await {
            return err;
        }
        let apply = self
            .apply_jev_settings(settings_after_saving_key(key))
            .await;
        format!("Saved [jev].api_key (masked in status).\n{apply}")
    }

    async fn jev_setup_off(self: &Arc<Self>) -> String {
        if let Err(err) = clear_user_api_key().await {
            return err;
        }
        if env_key_is_live() {
            let apply = self.apply_jev_settings_to_session().await;
            return format!(
                "Removed [jev].api_key. Env JEV_API_KEY / TYPESAFE_API_KEY is still set and wins, so Jev stays on until you unset the env var.\n{apply}"
            );
        }
        let apply = self.apply_jev_settings(None).await;
        format!("Removed [jev].api_key. Jev is off unless an env key is set.\n{apply}")
    }

    /// Reload file/env settings and live-apply them without a harness rebuild.
    pub(super) async fn apply_jev_settings_to_session(&self) -> String {
        self.apply_jev_settings(load_typed_config().jev_settings())
            .await
    }

    /// Live-apply `settings` on the current agent: swap `ask_jev` and the `<jev>`
    /// suffix only. History is not rewritten and compaction is not triggered.
    pub(super) async fn apply_jev_settings(
        &self,
        settings: Option<xai_grok_tools::implementations::grok_build::JevSettings>,
    ) -> String {
        self.rebuild_spec.jev_settings.set(settings.clone());
        match self.refresh_jev_on_live_agent(settings.as_ref()).await {
            Ok(()) => {
                if settings.is_some() {
                    "Jev is on in this session (ask_jev registered, prompt section added). No restart needed.".to_string()
                } else {
                    "Jev is off in this session (ask_jev removed).".to_string()
                }
            }
            Err(_) => {
                if settings.is_some() {
                    "Saved. A turn is in progress, so this session still has the previous Jev setup. Start a new session, or run /jev-setup apply when idle.".to_string()
                } else {
                    "Saved. A turn is in progress; start a new session (or /jev-setup apply when idle) to drop ask_jev.".to_string()
                }
            }
        }
    }

    /// Install or remove `ask_jev` + the `<jev>` section on the live agent.
    /// Fails only when a turn pins the agent (`running_task`).
    pub(super) async fn refresh_jev_on_live_agent(
        &self,
        settings: Option<&xai_grok_tools::implementations::grok_build::JevSettings>,
    ) -> Result<(), acp::Error> {
        {
            let state = self.state.lock().await;
            if state.running_task.is_some() {
                tracing::warn!(
                    session_id = %self.session_info.id.0,
                    "refresh_jev_on_live_agent: turn in flight, refusing to swap tools/prompt"
                );
                return Err(acp::Error::internal_error()
                    .data("jev_setup: turn in flight, refusing to refresh Jev"));
            }
        }
        let enabled = settings.is_some_and(|s| s.is_enabled());
        let jev_info =
            settings
                .filter(|s| s.is_enabled())
                .map(|settings| xai_grok_agent::JevPromptInfo {
                    nudge: settings.nudge_active(),
                    safety_check: settings.safety_active(),
                    context_filter: settings.context_filter_active(),
                    route: settings.route_active(),
                    file_pick: settings.file_pick_active(),
                    done_check: settings.done_check_active(),
                });
        let bridge = self.agent.borrow().tool_bridge().clone();
        if let Err(err) = self.sync_ask_jev_tool(&bridge, enabled) {
            tracing::warn!(
                session_id = %self.session_info.id.0,
                error = %err,
                "refresh_jev_on_live_agent: ask_jev register/unregister failed"
            );
            return Err(acp::Error::internal_error().data(err));
        }
        self.sync_jev_resources(&bridge, settings).await;
        {
            let mut prompt_context = self.agent.borrow().prompt_context().clone();
            if prompt_context.jev != jev_info {
                prompt_context.jev = jev_info;
                let current = self.agent.borrow().system_prompt().to_string();
                let rendered =
                    xai_grok_agent::RenderedPrompt::with_updated_jev(prompt_context, &current);
                self.agent.borrow_mut().set_rendered_prompt(rendered);
            }
        }
        self.publish_jev_prompt().await;
        self.send_available_commands_update(AdvertiseTrigger::JevSetup)
            .await;
        tracing::info!(
            session_id = %self.session_info.id.0,
            enabled,
            "refresh_jev_on_live_agent: ask_jev and <jev> section updated without rebuild"
        );
        Ok(())
    }

    fn sync_ask_jev_tool(
        &self,
        bridge: &xai_grok_tools::bridge::ToolBridge,
        enabled: bool,
    ) -> Result<(), String> {
        bridge.unregister_tool_by_name(ASK_JEV_TOOL_NAME);
        if enabled {
            bridge
                .register_first_party_tool(ASK_JEV_TOOL_NAME.to_owned(), AskJevTool)
                .map_err(|err| format!("failed to register ask_jev: {err}"))?;
        }
        Ok(())
    }

    async fn sync_jev_resources(
        &self,
        bridge: &xai_grok_tools::bridge::ToolBridge,
        settings: Option<&xai_grok_tools::implementations::grok_build::JevSettings>,
    ) {
        let enabled = settings.is_some_and(|s| s.is_enabled());
        if enabled {
            if let Some(settings) = settings {
                match settings.client() {
                    Ok(client) => bridge.update_resource(client).await,
                    Err(err) => tracing::warn!("failed to construct Jev client: {err}"),
                }
            }
        } else {
            bridge
                .update_resources_with(|resources| {
                    resources.remove::<JevClient>();
                })
                .await;
        }
        let nudge = settings.filter(|s| s.nudge_active());
        bridge
            .update_resources_with(|resources| {
                use xai_grok_tools::types::resources::{
                    EnabledNativeToolNames, NativeToolClientNames,
                };
                if let Some(names) = resources.get_mut::<EnabledNativeToolNames>() {
                    if enabled {
                        names.0.insert(ASK_JEV_TOOL_NAME.to_owned());
                    } else {
                        names.0.remove(ASK_JEV_TOOL_NAME);
                    }
                }
                if let Some(names) = resources.get_mut::<NativeToolClientNames>() {
                    if enabled {
                        names
                            .0
                            .insert(ASK_JEV_TOOL_NAME.to_owned(), ASK_JEV_TOOL_NAME.to_owned());
                    } else {
                        names.0.remove(ASK_JEV_TOOL_NAME);
                    }
                }
                match nudge {
                    Some(settings) => {
                        resources.insert(xai_grok_tools::reminders::JevNudgeConfig {
                            every: settings.nudge_every,
                        });
                    }
                    None => {
                        resources.remove::<xai_grok_tools::reminders::JevNudgeConfig>();
                    }
                }
                if settings.is_some_and(|s| s.file_pick_active()) {
                    resources.insert(xai_grok_tools::reminders::JevFilePickConfig);
                } else {
                    resources.remove::<xai_grok_tools::reminders::JevFilePickConfig>();
                }
                sync_jev_metrics_resource(resources, enabled);
            })
            .await;
    }

    async fn record_jev_decide(&self, record: xai_grok_jev::DecideRecord) {
        let toolset = self.tool_bridge_handle().toolset();
        let mut resources = toolset.resources.lock().await;
        if let Some(metrics) = resources.get_mut::<JevMetrics>() {
            metrics.record(record);
            return;
        }
        let mut metrics = JevMetrics::session_only();
        metrics.record(record);
        resources.insert(metrics);
    }

    /// Persist the prompt artifacts and swap only the `<jev>` suffix on the
    /// conversation system head (memory manifests and inherited prefixes stay).
    async fn publish_jev_prompt(&self) {
        self.abort_and_clear_prefire().await;
        let (system_prompt, persisted_context, jev) = {
            let agent = self.agent.borrow();
            let mut persisted_context = agent.prompt_context().clone();
            persisted_context.normalize_for_persistence();
            (
                agent.system_prompt().to_string(),
                persisted_context,
                agent.prompt_context().jev.clone(),
            )
        };
        super::save_prompt_context(&self.session_info, &persisted_context);
        super::save_system_prompt(&self.session_info, &system_prompt);
        let conversation = self.chat_state_handle.get_conversation().await;
        let current_head = match conversation.first() {
            Some(ConversationItem::System(sys)) => sys.content.as_ref(),
            _ => system_prompt.as_str(),
        };
        let head = xai_grok_agent::PromptContext::splice_jev_section(current_head, jev.as_ref());
        self.chat_state_handle.replace_system_head(&head).await;
    }
}

fn sync_jev_metrics_resource(
    resources: &mut xai_grok_tools::types::resources::Resources,
    enabled: bool,
) {
    let path = jev_stats_log_path();
    if enabled {
        if let Some(metrics) = resources.get_mut::<JevMetrics>() {
            metrics.set_log_path(&path);
            metrics.set_persist_enabled(true);
        } else {
            resources.insert(JevMetrics::with_persist(path));
        }
    } else if let Some(metrics) = resources.get_mut::<JevMetrics>() {
        metrics.set_persist_enabled(false);
    }
}

fn load_typed_config() -> crate::agent::config::Config {
    crate::config::load_effective_config()
        .ok()
        .and_then(|raw| crate::agent::config::Config::new_from_toml_cfg(&raw).ok())
        .unwrap_or_default()
}

/// Prefer a fresh effective-config read; if that snapshot is stale, still enable from the key we just wrote.
fn settings_after_saving_key(
    key: &str,
) -> Option<xai_grok_tools::implementations::grok_build::JevSettings> {
    load_typed_config().jev_settings().or_else(|| {
        xai_grok_tools::implementations::grok_build::JevSettings::from_resolved(key, None, None)
    })
}
