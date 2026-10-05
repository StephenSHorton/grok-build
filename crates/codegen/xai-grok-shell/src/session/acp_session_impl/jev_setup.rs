//! `/jev-setup` builtin: persist `[jev]`, optionally rebuild this session.

use super::*;
use crate::util::config::{
    JevSetupRequest, clear_user_api_key, collect_status_from_file, env_key_is_live, format_help,
    format_status, save_user_api_key, set_user_flag, validate_key,
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
                    let apply = self.apply_jev_settings_to_session().await;
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
        match validate_key(key).await {
            Ok(()) => {}
            Err(err) if force => {
                if let Err(save_err) = save_user_api_key(key).await {
                    return save_err;
                }
                let apply = self.apply_jev_settings_to_session().await;
                return format!(
                    "Saved [jev].api_key even though validation failed:\n{err}\n\n{apply}"
                );
            }
            Err(err) => {
                return format!(
                    "{err}\nNot saved. Re-run with --force if you want to store this key anyway."
                );
            }
        }
        if let Err(err) = save_user_api_key(key).await {
            return err;
        }
        let apply = self.apply_jev_settings_to_session().await;
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
        let apply = self.apply_jev_settings_to_session().await;
        format!("Removed [jev].api_key. Jev is off unless an env key is set.\n{apply}")
    }

    /// Reload file/env settings into the rebuild spec and rebuild the harness when idle.
    pub(super) async fn apply_jev_settings_to_session(self: &Arc<Self>) -> String {
        let settings = load_typed_config().jev_settings();
        self.rebuild_spec.jev_settings.set(settings.clone());
        let definition = self.agent.borrow().definition().clone();
        let label = self
            .agent
            .borrow()
            .prompt_context()
            .system_prompt_label
            .clone();
        match self
            .handle_rebuild_agent_for_definition(definition, label)
            .await
        {
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
}

fn load_typed_config() -> crate::agent::config::Config {
    crate::config::load_effective_config()
        .ok()
        .and_then(|raw| crate::agent::config::Config::new_from_toml_cfg(&raw).ok())
        .unwrap_or_default()
}
