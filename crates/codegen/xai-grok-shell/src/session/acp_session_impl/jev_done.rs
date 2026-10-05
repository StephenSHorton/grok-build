//! Optional Jev done check after a successful mutate.
//!
//! Flag off / no key / subagent / Jev fault: continue. Only a live noul ≥ 0.90 ends the turn.

use xai_grok_sampling_types::{ContentPart, ConversationItem};
use xai_grok_tools::types::tool::ToolKind;

use super::SessionActor;

impl SessionActor {
    /// After a mutate batch. Fail-open is continue (`false`).
    pub(super) async fn apply_jev_done_check(&self, mutate_batch: bool) -> bool {
        if !mutate_batch || self.startup_hints.is_subagent {
            return false;
        }
        let Some(settings) = self
            .rebuild_spec
            .jev_settings
            .get()
            .filter(|settings| settings.done_check_active())
        else {
            return false;
        };
        let items = self.chat_state_handle.get_conversation().await;
        let query = last_user_query(&items);
        let evidence = last_tool_evidence(&items);
        let toolset = self.tool_bridge_handle().toolset();
        let client = {
            let resources = toolset.resources.lock().await;
            resources
                .get::<xai_grok_tools::implementations::grok_build::JevClient>()
                .cloned()
        };
        let (verdict, record) =
            xai_grok_tools::implementations::grok_build::maybe_done_check_recorded(
                Some(&settings),
                client.as_ref(),
                &query,
                &evidence,
            )
            .await;
        if let Some(record) = record {
            let mut resources = toolset.resources.lock().await;
            if let Some(metrics) =
                resources.get_mut::<xai_grok_tools::implementations::grok_build::JevMetrics>()
            {
                metrics.record(record);
            }
        }
        if verdict.ends_turn() {
            tracing::info!(
                session_id = %self.session_info.id,
                "jev done_check: ending turn on live high-confidence noul"
            );
            true
        } else {
            false
        }
    }
}

pub(super) fn is_mutate_kind(kind: Option<ToolKind>) -> bool {
    matches!(
        kind,
        Some(ToolKind::Edit | ToolKind::Write | ToolKind::Execute)
    )
}

fn last_user_query(items: &[ConversationItem]) -> String {
    for item in items.iter().rev() {
        let ConversationItem::User(user) = item else {
            continue;
        };
        let text: String = user
            .content
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.as_ref()),
                ContentPart::Image { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !text.trim().is_empty() {
            return text;
        }
    }
    String::new()
}

fn last_tool_evidence(items: &[ConversationItem]) -> String {
    for item in items.iter().rev() {
        let ConversationItem::ToolResult(tool_result) = item else {
            continue;
        };
        let text = tool_result.content.as_ref();
        if !text.trim().is_empty() {
            return text.to_owned();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_sampling_types::ConversationItem;

    #[test]
    fn mutate_kinds() {
        assert!(is_mutate_kind(Some(ToolKind::Edit)));
        assert!(is_mutate_kind(Some(ToolKind::Write)));
        assert!(is_mutate_kind(Some(ToolKind::Execute)));
        assert!(!is_mutate_kind(Some(ToolKind::Read)));
        assert!(!is_mutate_kind(Some(ToolKind::Search)));
        assert!(!is_mutate_kind(None));
    }

    #[test]
    fn last_tool_evidence_skips_empty() {
        let items = vec![
            ConversationItem::user("make the test pass"),
            ConversationItem::tool_result("edit", "applied"),
        ];
        assert_eq!(last_user_query(&items), "make the test pass");
        assert_eq!(last_tool_evidence(&items), "applied");
    }
}
