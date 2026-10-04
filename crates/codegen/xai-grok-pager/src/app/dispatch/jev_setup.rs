//! `/jev-setup set` masked prompt and validate+save dispatch.

use super::ctx::{get_active_agent_mut, with_active_agent};
use super::queue::{maybe_drain_queue_and_note_peek, push_and_page_flip};
use crate::app::actions::Effect;
use crate::app::agent::AgentId;
use crate::app::agent_view::PromptInputMode;
use crate::app::app_view::AppView;
use crate::scrollback::block::RenderBlock;
use xai_grok_shell::util::config::JevKeyArg;

pub(super) fn dispatch_enter_jev_key_mode(app: &mut AppView, force: bool) -> Vec<Effect> {
    with_active_agent(app, |agent| {
        agent.prompt_input_mode = PromptInputMode::JevKey;
        agent.jev_setup_force = force;
        agent.jev_pending_key = None;
        agent.prompt.set_text("");
    });
    vec![]
}

pub(super) fn dispatch_submit_jev_key(app: &mut AppView, force: bool) -> Vec<Effect> {
    let Some(agent) = get_active_agent_mut(app) else {
        return vec![];
    };
    let Some(key) = agent.jev_pending_key.take() else {
        push_and_page_flip(
            &mut agent.scrollback,
            RenderBlock::system("No key entered.".to_string()),
        );
        return vec![];
    };
    let key = key.trim().to_string();
    if key.is_empty() {
        push_and_page_flip(
            &mut agent.scrollback,
            RenderBlock::system("Refusing to save an empty key.".to_string()),
        );
        return vec![];
    }
    vec![Effect::JevSetupValidateAndSave {
        agent_id: agent.session.id,
        key: JevKeyArg(key),
        force,
    }]
}

pub(super) fn handle_jev_setup_complete(
    app: &mut AppView,
    agent_id: AgentId,
    message: String,
    apply: bool,
) -> Vec<Effect> {
    {
        let Some(agent) = app.agents.get_mut(&agent_id) else {
            return vec![];
        };
        push_and_page_flip(&mut agent.scrollback, RenderBlock::system(message));
        if !apply {
            return vec![];
        }
        agent.session.enqueue_command("/jev-setup apply".to_string());
    }
    maybe_drain_queue_and_note_peek(app, agent_id)
}
