//! Optional Jev trivial-request router on the first sample of a turn.
//!
//! Flag off / no key / subagent / Jev fault: full agent (tools stay listed).
//! A live high-confidence `no_tools` sets `tool_choice` to None for that sample only.

use xai_grok_sampling_types::{ContentPart, ConversationItem, ConversationToolChoice};

use super::SessionActor;

impl SessionActor {
    /// First sample only. Never mutates history. Fail-open is [`RouteVerdict::FullAgent`].
    pub(super) async fn apply_jev_route(
        &self,
        request: &mut crate::sampling::ConversationRequest,
        first_sample: bool,
    ) {
        if !first_sample || self.startup_hints.is_subagent {
            return;
        }
        let Some(settings) = self
            .rebuild_spec
            .jev_settings
            .get()
            .filter(|settings| settings.route_active())
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
        let (query, has_image) = last_user_query_and_image(&request.items);
        let (verdict, record) = xai_grok_tools::implementations::grok_build::maybe_route_recorded(
            Some(&settings),
            client.as_ref(),
            &query,
            has_image,
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
        if verdict.omits_tools() {
            request.tool_choice = Some(ConversationToolChoice::None);
            tracing::info!(
                session_id = %self.session_info.id,
                "jev route: first sample omits tools (fail-open full-agent otherwise)"
            );
        }
    }
}

fn last_user_query_and_image(items: &[ConversationItem]) -> (String, bool) {
    for item in items.iter().rev() {
        let ConversationItem::User(user) = item else {
            continue;
        };
        let mut has_image = false;
        let mut texts = Vec::new();
        for part in &user.content {
            match part {
                ContentPart::Text { text } => texts.push(text.as_ref()),
                ContentPart::Image { .. } => has_image = true,
            }
        }
        let text = texts.join("\n");
        if !text.trim().is_empty() || has_image {
            return (text, has_image);
        }
    }
    (String::new(), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_sampling_types::ConversationItem;

    #[test]
    fn last_user_prefers_latest_text() {
        let items = vec![
            ConversationItem::user("old"),
            ConversationItem::tool_result("grep", "hits"),
            ConversationItem::user("What is the git status?"),
        ];
        let (query, has_image) = last_user_query_and_image(&items);
        assert_eq!(query, "What is the git status?");
        assert!(!has_image);
    }
}
