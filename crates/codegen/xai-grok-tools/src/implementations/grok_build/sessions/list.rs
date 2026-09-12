use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

use super::store::{ListedSession, Talkable};

pub const SESSIONS_LIST_TOOL_NAME: &str = "sessions_list";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsListInput {
    #[serde(default)]
    #[schemars(description = "Only sessions that have claimed this role (e.g. pr-reviews)")]
    pub role: Option<String>,
    #[serde(default)]
    #[schemars(description = "If true, only conversations whose process is up (injectable now)")]
    pub live_only: bool,
    #[serde(default)]
    #[schemars(description = "Case-insensitive filter on title, cwd, session id, or role")]
    pub query: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsListOutput {
    pub sessions: Vec<ListedSessionView>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ListedSessionView {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_turn_summary: Option<String>,
    pub roles: Vec<String>,
    pub live: bool,
    pub talkable: Talkable,
}

impl From<ListedSession> for ListedSessionView {
    fn from(s: ListedSession) -> Self {
        Self {
            session_id: s.session_id,
            title: s.title,
            cwd: s.cwd,
            last_turn_summary: s.last_turn_summary,
            roles: s.roles,
            live: s.live,
            talkable: s.talkable,
        }
    }
}

impl xai_tool_runtime::ToolOutput for SessionsListOutput {}

#[derive(Debug, Default)]
pub struct SessionsListTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsListTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsList
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "List Grok Build conversations on this machine (live and dormant). Filter by role (e.g. pr-reviews), live_only, or text. talkable is inject (send now) or mailbox (delivered when that chat next opens). Other conversations are siblings, not subagents."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsListTool {
    type Args = SessionsListInput;
    type Output = SessionsListOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_LIST_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_LIST_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.sessions_list", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsListInput,
    ) -> Result<SessionsListOutput, xai_tool_runtime::ToolError> {
        let (session_id, title, cwd) = super::caller_identity(&ctx).await?;
        let bus = super::open_bus()?;
        super::ensure_registered(&bus, &session_id, title, cwd)?;
        let sessions = bus
            .list(
                input.role.as_deref(),
                input.live_only,
                input.query.as_deref(),
            )
            .map_err(super::bus_tool_error)?;
        Ok(SessionsListOutput {
            sessions: sessions.into_iter().map(ListedSessionView::from).collect(),
        })
    }
}
