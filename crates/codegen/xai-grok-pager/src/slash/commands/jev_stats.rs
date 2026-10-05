//! `/jev-stats`: session + all-time Jev Decide metrics (queued to the shell).

use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand, slash_meta};

pub struct JevStatsCommand;

impl SlashCommand for JevStatsCommand {
    slash_meta! {
        name: "jev-stats",
        description: "Show Jev Decide latency, tokens, and outcomes",
        usage: "/jev-stats",
        session_scoped: true,
    }

    fn run(&self, ctx: &mut CommandExecCtx, _args: &str) -> CommandResult {
        if ctx.session_id.is_none() {
            return CommandResult::Error("No active session".to_string());
        }
        CommandResult::QueueCommand("/jev-stats".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;

    fn run() -> CommandResult {
        let models = ModelState::default();
        let bundle = crate::app::bundle::BundleState::default();
        let sid = agent_client_protocol::SessionId::from("test-session".to_string());
        let mut ctx = CommandExecCtx {
            models: &models,
            session_id: Some(&sid),
            bundle_state: &bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: crate::settings::PagerLocalSnapshot::default(),
        };
        JevStatsCommand.run(&mut ctx, "")
    }

    #[test]
    fn queues_shell_builtin() {
        assert!(matches!(
            run(),
            CommandResult::QueueCommand(cmd) if cmd == "/jev-stats"
        ));
    }
}
