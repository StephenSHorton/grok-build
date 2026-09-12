use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const SESSIONS_RELEASE_TOOL_NAME: &str = "sessions_release";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsReleaseInput {
    #[schemars(description = "Duty slug this conversation should stop owning")]
    pub role: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsReleaseOutput {
    pub role: String,
    pub session_id: String,
}

impl xai_tool_runtime::ToolOutput for SessionsReleaseOutput {}

#[derive(Debug, Default)]
pub struct SessionsReleaseTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsReleaseTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsSend
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Drop this conversation's claim on a duty role so another chat can take it."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsReleaseTool {
    type Args = SessionsReleaseInput;
    type Output = SessionsReleaseOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_RELEASE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_RELEASE_TOOL_NAME,
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

    #[tracing::instrument(name = "tool.sessions_release", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsReleaseInput,
    ) -> Result<SessionsReleaseOutput, xai_tool_runtime::ToolError> {
        let (session_id, title, cwd) = super::caller_identity(&ctx).await?;
        let bus = super::open_bus()?;
        super::ensure_registered(&bus, &session_id, title, cwd)?;
        bus.release(&session_id, &input.role)
            .map_err(super::bus_tool_error)?;
        Ok(SessionsReleaseOutput {
            role: input.role,
            session_id,
        })
    }
}
