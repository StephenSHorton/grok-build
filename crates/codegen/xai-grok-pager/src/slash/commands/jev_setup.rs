//! `/jev-setup`: enable Jev from the TUI without leaving the app.
//!
//! Bare invocation prints status (keys always masked). `set` enters a masked
//! prompt so the key never lands on the slash line or in scrollback.

use crate::app::actions::Action;
use crate::slash::command::{
    AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand, slash_meta,
};
use xai_grok_shell::util::config::{
    JevSetupRequest, collect_status_from_file, format_help, format_status, parse_args,
};

pub struct JevSetupCommand;

impl SlashCommand for JevSetupCommand {
    slash_meta! {
        name: "jev-setup",
        description: "Enable Jev, set or remove the key, and toggle extras",
        usage: "/jev-setup [status|set [--force]|off|nudge on|off|safety on|off|filter on|off|route on|off|file_pick on|off|done on|off|apply|stats]",
        takes_args: true,
    }

    fn suggest_args(&self, _ctx: &AppCtx, _args_query: &str) -> Option<Vec<ArgItem>> {
        Some(vec![
            arg("status", "Show on/off, key source, and flags"),
            arg("set", "Enter a key (masked prompt)"),
            arg("set --force", "Save even if Decide ping fails"),
            arg("off", "Remove the file key"),
            arg(
                "nudge on",
                "Remind after successful edits/shells (default on with a key)",
            ),
            arg("nudge off", "Turn the nudge off"),
            arg("safety on", "Fail-open destructive-call check"),
            arg("safety off", "Turn the safety check off"),
            arg("filter on", "Omit older tool results Jev says are stale"),
            arg("filter off", "Turn the context filter off"),
            arg(
                "route on",
                "First sample may omit tools on a trivial prompt (default on with a key)",
            ),
            arg("route off", "Always use the full agent"),
            arg(
                "file_pick on",
                "After grep/list_dir, hint the most relevant hit (default off)",
            ),
            arg("file_pick off", "Turn file picking off"),
            arg(
                "done on",
                "End the turn after a mutate if Jev is sure the request is done (default off)",
            ),
            arg("done off", "Turn the done check off"),
            arg("apply", "Turn Jev on in this session if idle"),
            arg("stats", "Decide latency, tokens, and outcomes"),
            arg("help", "Show usage"),
        ])
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        match parse_args(args) {
            Ok(JevSetupRequest::Status) | Ok(JevSetupRequest::Help) => {
                let mut text = format_status(&current_jev_status());
                text.push_str("\n\n");
                text.push_str(&format_help());
                text.push_str(
                    "\n\nIn the TUI, `/jev-setup set` opens a masked prompt. Do not paste the key after the command.",
                );
                if matches!(parse_args(args), Ok(JevSetupRequest::Help)) {
                    CommandResult::Message(format_help())
                } else {
                    CommandResult::Message(text)
                }
            }
            Ok(JevSetupRequest::Set { key, force }) => {
                if key.is_some() {
                    return CommandResult::Error(
                        "Don't paste the key on the slash line (it can land in scrollback). Run /jev-setup set and type it in the masked prompt.".to_string(),
                    );
                }
                CommandResult::Action(Action::EnterJevKeyMode { force })
            }
            Ok(JevSetupRequest::Off) => CommandResult::QueueCommand("/jev-setup off".to_string()),
            Ok(JevSetupRequest::Flag { flag, on }) => CommandResult::QueueCommand(format!(
                "/jev-setup {} {}",
                flag.name(),
                if on { "on" } else { "off" }
            )),
            Ok(JevSetupRequest::Apply) => {
                CommandResult::QueueCommand("/jev-setup apply".to_string())
            }
            Ok(JevSetupRequest::Stats) => {
                CommandResult::QueueCommand("/jev-setup stats".to_string())
            }
            Err(err) => CommandResult::Error(format!("{err}\n\n{}", format_help())),
        }
    }
}

fn arg(insert: &str, description: &str) -> ArgItem {
    ArgItem {
        display: insert.to_string(),
        match_text: insert.to_string(),
        insert_text: insert.to_string(),
        description: description.to_string(),
    }
}

fn current_jev_status() -> xai_grok_shell::util::config::JevSetupStatus {
    let file = xai_grok_shell::config::load_effective_config()
        .ok()
        .and_then(|raw| xai_grok_shell::agent::config::Config::new_from_toml_cfg(&raw).ok())
        .map(|cfg| cfg.jev)
        .unwrap_or_default();
    collect_status_from_file(&file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;

    fn run(args: &str) -> CommandResult {
        let models = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: None,
            bundle_state: &bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        JevSetupCommand.run(&mut ctx, args)
    }

    #[test]
    fn status_is_a_message_without_raw_secrets() {
        let CommandResult::Message(text) = run("") else {
            panic!("expected Message");
        };
        assert!(text.contains("Jev:"));
        assert!(!text.contains("JEV_API_KEY="));
    }

    #[test]
    fn set_without_key_enters_masked_mode() {
        assert!(matches!(
            run("set"),
            CommandResult::Action(Action::EnterJevKeyMode { force: false })
        ));
        assert!(matches!(
            run("set --force"),
            CommandResult::Action(Action::EnterJevKeyMode { force: true })
        ));
    }

    #[test]
    fn set_with_inline_key_is_rejected() {
        let CommandResult::Error(msg) = run("set jv_live_secret") else {
            panic!("expected Error");
        };
        assert!(msg.contains("masked prompt"));
        assert!(!msg.contains("jv_live_secret"));
    }

    #[test]
    fn off_and_flags_go_to_the_shell_builtin() {
        assert!(matches!(
            run("off"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup off"
        ));
        assert!(matches!(
            run("nudge on"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup nudge on"
        ));
        assert!(matches!(
            run("route off"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup route off"
        ));
        assert!(matches!(
            run("file_pick on"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup file_pick on"
        ));
        assert!(matches!(
            run("done on"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup done_check on"
        ));
        assert!(matches!(
            run("apply"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup apply"
        ));
        assert!(matches!(
            run("stats"),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-setup stats"
        ));
    }
}
