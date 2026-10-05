//! Live `/jev-setup` apply must not compact history and must put `ask_jev` on the
//! next completion request (the wire tool list), plus the `<jev>` prompt section.

use std::sync::Arc;

use xai_grok_sampling_types::conversation::ConversationItem;
use xai_grok_tools::implementations::grok_build::ASK_JEV_TOOL_NAME;
use xai_grok_tools::types::ToolInput;

use super::support::{create_test_actor, running_task_stub};
use super::{PersistenceMsg, SessionActor};

fn test_jev_settings() -> xai_grok_tools::implementations::grok_build::JevSettings {
    xai_grok_tools::implementations::grok_build::JevSettings::from_resolved(
        "test-jev-key",
        None,
        None,
    )
    .expect("test key")
}

fn history() -> Vec<ConversationItem> {
    vec![
        ConversationItem::system("You are Grok. Existing session."),
        ConversationItem::user("hello from an earlier turn"),
        ConversationItem::assistant("hi — I remember this"),
        ConversationItem::user("and another turn"),
        ConversationItem::assistant("still here"),
    ]
}

fn tail_json(conv: &[ConversationItem]) -> serde_json::Value {
    serde_json::to_value(conv.iter().skip(1).collect::<Vec<_>>()).expect("conversation tail")
}

fn head_text(conv: &[ConversationItem]) -> String {
    match conv.first() {
        Some(ConversationItem::System(sys)) => sys.content.to_string(),
        other => panic!("expected system head, got {other:?}"),
    }
}

fn has_compaction_meta(conv: &[ConversationItem]) -> bool {
    conv.iter().any(|item| match item {
        ConversationItem::System(sys) => {
            sys.content.contains("Summary:") || sys.content.contains("<compaction")
        }
        ConversationItem::User(user) => user.content.iter().any(|part| match part {
            xai_grok_sampling_types::ContentPart::Text { text, .. } => {
                text.contains("compaction") || text.contains("Summary:")
            }
            _ => false,
        }),
        _ => false,
    })
}

async fn actor_with_history() -> SessionActor {
    let (gateway_tx, _grx) =
        tokio::sync::mpsc::unbounded_channel::<xai_acp_lib::AcpClientMessage>();
    let (persistence_tx, _prx) = tokio::sync::mpsc::unbounded_channel::<PersistenceMsg>();
    let actor = create_test_actor(0, 256_000, 85, gateway_tx, persistence_tx).await;
    actor.chat_state_handle.replace_conversation(history());
    let _ = actor.chat_state_handle.get_conversation().await;
    actor
}

async fn outgoing_request(actor: &SessionActor) -> xai_grok_sampling_types::ConversationRequest {
    let defs = actor.prepare_tool_definitions().await;
    let tools = actor.turn_base_tool_specs(&defs);
    actor
        .chat_state_handle
        .build_request(
            tools,
            None,
            false,
            None,
            actor.session_id_string(),
            "jev-live-apply".to_string(),
        )
        .await
        .expect("chat state actor should be alive")
}

fn request_has_ask_jev(request: &xai_grok_sampling_types::ConversationRequest) -> bool {
    request
        .tools
        .iter()
        .any(|tool| tool.name == ASK_JEV_TOOL_NAME)
}

#[tokio::test(flavor = "current_thread")]
async fn live_apply_keeps_history_and_puts_ask_jev_on_the_outgoing_request() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with_history().await;
            let before = actor.chat_state_handle.get_conversation().await;
            let before_tail = tail_json(&before);
            let before_compact = actor
                .chat_state_handle
                .get_last_compaction_prompt_index()
                .await;
            let bridge_before = Arc::as_ptr(actor.agent.borrow().tool_bridge());

            let msg = actor.apply_jev_settings(Some(test_jev_settings())).await;
            assert!(
                msg.contains("ask_jev registered"),
                "apply should report a live tool install: {msg}"
            );

            let after = actor.chat_state_handle.get_conversation().await;
            assert_eq!(
                tail_json(&after),
                before_tail,
                "live apply must not rewrite or compact user/assistant history"
            );
            assert_eq!(after.len(), before.len());
            assert!(!has_compaction_meta(&after));
            assert_eq!(
                actor
                    .chat_state_handle
                    .get_last_compaction_prompt_index()
                    .await,
                before_compact
            );
            assert_eq!(
                Arc::as_ptr(actor.agent.borrow().tool_bridge()),
                bridge_before,
                "must refresh the live agent, not rebuild a new harness"
            );

            let head = head_text(&after);
            assert!(
                head.contains("<jev>"),
                "system head must gain <jev>: {head}"
            );
            assert!(head.contains("ask_jev"), "{head}");
            assert!(head.contains("You are Grok. Existing session."));
            assert_eq!(
                actor.agent.borrow().prompt_context().jev,
                Some(xai_grok_agent::JevPromptInfo::default())
            );

            let request = outgoing_request(&actor).await;
            assert!(
                request_has_ask_jev(&request),
                "next completion request must list ask_jev, got {:?}",
                request
                    .tools
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
            );
            let parsed = actor
                .agent
                .borrow()
                .tool_bridge()
                .try_parse(
                    ASK_JEV_TOOL_NAME,
                    serde_json::json!({
                        "state": {"ok": true},
                        "questions": [{
                            "name": "q",
                            "question": "Is this a test?",
                            "mode": "boolean"
                        }]
                    }),
                )
                .await
                .expect("ask_jev must parse");
            assert!(
                matches!(parsed, ToolInput::AskJev(_)),
                "live register must keep ToolInput::AskJev, got {parsed:?}"
            );
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn live_off_and_flag_toggle_keep_history_and_update_tools_prompt() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with_history().await;
            let _ = actor.apply_jev_settings(Some(test_jev_settings())).await;
            let on_tail = tail_json(&actor.chat_state_handle.get_conversation().await);

            let mut nudged = test_jev_settings();
            nudged.nudge = true;
            let msg = actor.apply_jev_settings(Some(nudged)).await;
            assert!(msg.contains("ask_jev registered"), "{msg}");
            let after_flag = actor.chat_state_handle.get_conversation().await;
            assert_eq!(tail_json(&after_flag), on_tail);
            assert!(!has_compaction_meta(&after_flag));
            let head = head_text(&after_flag);
            assert!(
                head.contains("nudge: a reminder may suggest ask_jev"),
                "{head}"
            );
            assert!(request_has_ask_jev(&outgoing_request(&actor).await));

            let msg = actor.apply_jev_settings(None).await;
            assert!(msg.contains("ask_jev removed"), "{msg}");
            let after_off = actor.chat_state_handle.get_conversation().await;
            assert_eq!(tail_json(&after_off), on_tail);
            assert!(!has_compaction_meta(&after_off));
            let off_head = head_text(&after_off);
            assert!(!off_head.contains("<jev>"), "{off_head}");
            assert!(!off_head.contains("ask_jev"), "{off_head}");
            assert!(!request_has_ask_jev(&outgoing_request(&actor).await));
            assert!(actor.agent.borrow().prompt_context().jev.is_none());
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn live_apply_defers_when_a_turn_is_in_flight() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let actor = actor_with_history().await;
            {
                let mut state = actor.state.lock().await;
                state.running_task = Some(running_task_stub("busy"));
            }
            let msg = actor.apply_jev_settings(Some(test_jev_settings())).await;
            assert!(
                msg.contains("turn is in progress"),
                "must defer while a turn pins the agent: {msg}"
            );
            let conv = actor.chat_state_handle.get_conversation().await;
            assert_eq!(tail_json(&conv), tail_json(&history()));
            assert!(!request_has_ask_jev(&outgoing_request(&actor).await));
            assert!(!head_text(&conv).contains("<jev>"));
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn new_session_build_with_file_key_registers_ask_jev() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            let spec = crate::session::agent_rebuild::test_rebuild_spec_default();
            spec.jev_settings.set(Some(test_jev_settings()));
            let agent = spec
                .build_agent(
                    xai_grok_agent::AgentDefinition::default_grok_build(),
                    "Grok",
                )
                .await
                .expect("spawn-equivalent rebuild with a file key");
            let names: Vec<String> = agent
                .tool_definitions()
                .await
                .into_iter()
                .map(|td| td.function.name)
                .collect();
            assert!(
                names.iter().any(|n| n == ASK_JEV_TOOL_NAME),
                "new session with a key must register ask_jev: {names:?}"
            );
            let prompt = agent.system_prompt();
            assert!(prompt.contains("<jev>"), "{prompt}");
            assert!(prompt.contains("ask_jev"), "{prompt}");
        })
        .await;
}
