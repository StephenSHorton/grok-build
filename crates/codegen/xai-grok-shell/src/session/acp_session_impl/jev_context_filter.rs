//! Optional Jev context filter on the next completion request.
//!
//! `prune_conversation` stays sync and HTTP-free. When `context_filter` is on,
//! this pass looks at the same older tool results that prune considers and may
//! omit one from the request clone. Session history is unchanged. Errors keep
//! the item. At most [`MAX_FILTER_CALLS_PER_REQUEST`] Decide calls run.

use xai_grok_sampling_types::{ContentPart, ConversationItem};

use super::SessionActor;

/// Match `PruningConfig::keep_last_n_turns` so recent turns are never filtered.
const KEEP_LAST_N_TURNS: usize = 3;

/// Cap HTTP so a long transcript cannot stall the turn.
const MAX_FILTER_CALLS_PER_REQUEST: usize = 4;

/// Request-only placeholder. Session log keeps the original tool result.
pub(super) const JEV_FILTER_PLACEHOLDER: &str = "[Tool result omitted — not relevant]";

impl SessionActor {
    /// Flag off / no key: return immediately. Does not lock resources or send HTTP.
    pub(super) async fn apply_jev_context_filter(&self, items: &mut [ConversationItem]) {
        let Some(settings) = self
            .rebuild_spec
            .jev_settings
            .as_ref()
            .filter(|settings| settings.context_filter_active())
        else {
            return;
        };
        let toolset = self.tool_bridge_handle().toolset();
        let client = {
            let resources = toolset.resources.lock().await;
            resources
                .get::<xai_grok_tools::implementations::grok_build::JevClient>()
                .cloned()
        };
        let query = last_user_query(items);
        let mut calls = 0usize;
        for snippet in older_tool_result_contents_mut(items) {
            if calls >= MAX_FILTER_CALLS_PER_REQUEST {
                break;
            }
            if snippet.trim().is_empty() || snippet.as_ref() == JEV_FILTER_PLACEHOLDER {
                continue;
            }
            if snippet.starts_with("[Tool result omitted") {
                continue;
            }
            calls += 1;
            let keep = xai_grok_tools::implementations::grok_build::maybe_keep_snippet(
                Some(settings),
                client.as_ref(),
                &query,
                snippet,
            )
            .await;
            if !keep {
                *snippet = std::sync::Arc::<str>::from(JEV_FILTER_PLACEHOLDER);
            }
        }
    }
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

/// Same turn-age walk as `prune_conversation`: skip the last `KEEP_LAST_N_TURNS`.
fn older_tool_result_contents_mut(items: &mut [ConversationItem]) -> Vec<&mut std::sync::Arc<str>> {
    let mut turn_from_end: usize = 0;
    let mut seen_first_user = false;
    let mut out = Vec::new();
    for item in items.iter_mut().rev() {
        if matches!(item, ConversationItem::User(_)) {
            if seen_first_user {
                turn_from_end += 1;
            }
            seen_first_user = true;
            continue;
        }
        let ConversationItem::ToolResult(tool_result) = item else {
            continue;
        };
        if turn_from_end < KEEP_LAST_N_TURNS {
            continue;
        }
        out.push(&mut tool_result.content);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_sampling_types::ConversationItem;

    #[test]
    fn older_tool_results_skip_recent_turns() {
        let mut items = vec![
            ConversationItem::user("t1"),
            ConversationItem::tool_result("old", "ancient"),
            ConversationItem::user("t2"),
            ConversationItem::tool_result("mid", "older"),
            ConversationItem::user("t3"),
            ConversationItem::tool_result("recent3", "keep3"),
            ConversationItem::user("t4"),
            ConversationItem::tool_result("recent2", "keep2"),
            ConversationItem::user("t5"),
            ConversationItem::tool_result("recent1", "keep1"),
        ];
        let older: Vec<String> = older_tool_result_contents_mut(&mut items)
            .into_iter()
            .map(|s| s.as_ref().to_owned())
            .collect();
        assert!(older.contains(&"ancient".to_owned()));
        assert!(older.contains(&"older".to_owned()));
        assert!(!older.iter().any(|s| s.starts_with("keep")));
    }
}
