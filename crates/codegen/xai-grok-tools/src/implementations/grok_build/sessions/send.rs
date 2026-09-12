use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

use super::store::{Delivery, PeerMessage};

pub const SESSIONS_SEND_TOOL_NAME: &str = "sessions_send";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsSendInput {
    #[schemars(
        description = "Target: a role slug (pr-reviews) or a session id from sessions_list"
    )]
    pub to: String,
    #[schemars(description = "Message body the other conversation will see as a turn")]
    pub content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsSendOutput {
    pub delivered: Delivery,
    pub message_id: String,
    pub to_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_title: Option<String>,
}

impl xai_tool_runtime::ToolOutput for SessionsSendOutput {}

#[derive(Debug, Default)]
pub struct SessionsSendTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsSendTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsSend
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Send a message to another Grok Build conversation, by session id or by duty role. Live chats get an injected turn immediately; dormant chats get mailbox delivery on next open. Not Discord and not spawn_subagent."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsSendTool {
    type Args = SessionsSendInput;
    type Output = SessionsSendOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_SEND_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_SEND_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.sessions_send", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsSendInput,
    ) -> Result<SessionsSendOutput, xai_tool_runtime::ToolError> {
        let content = input.content.trim();
        if content.is_empty() {
            return Err(xai_tool_runtime::ToolError::custom(
                "empty_content",
                "content is required",
            ));
        }
        let (from_session, from_title, cwd) = super::caller_identity(&ctx).await?;
        let bus = super::open_bus()?;
        super::ensure_registered(&bus, &from_session, from_title.clone(), cwd)?;
        let to_session = bus
            .resolve_target(&input.to)
            .map_err(super::bus_tool_error)?;
        let to_title = Some(super::store::peer_display_label(
            &input.to,
            bus.title_for_session(&to_session).as_deref(),
        ));
        let from_title = from_title.or_else(|| bus.title_for_session(&from_session));
        let role = super::store::validate_role(&input.to).ok();
        let message_id = uuid::Uuid::now_v7().to_string();
        let msg = PeerMessage {
            message_id: message_id.clone(),
            from_session,
            from_title,
            role,
            content: content.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let delivered = bus.send(&to_session, &msg).map_err(super::bus_tool_error)?;
        Ok(SessionsSendOutput {
            delivered,
            message_id,
            to_session,
            to_title,
        })
    }
}
