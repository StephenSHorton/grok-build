use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const SESSIONS_CLAIM_TOOL_NAME: &str = "sessions_claim";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsClaimInput {
    #[schemars(
        description = "Duty slug this conversation owns, e.g. pr-reviews. Exclusive while this session is live."
    )]
    pub role: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsClaimOutput {
    pub role: String,
    pub session_id: String,
}

impl xai_tool_runtime::ToolOutput for SessionsClaimOutput {}

#[derive(Debug, Default)]
pub struct SessionsClaimTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsClaimTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsSend
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Claim an exclusive duty role for THIS conversation (e.g. pr-reviews). Other chats send work to that role. Fails if another live conversation already holds it. Dead holders are replaced."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsClaimTool {
    type Args = SessionsClaimInput;
    type Output = SessionsClaimOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_CLAIM_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_CLAIM_TOOL_NAME,
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

    #[tracing::instrument(name = "tool.sessions_claim", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsClaimInput,
    ) -> Result<SessionsClaimOutput, xai_tool_runtime::ToolError> {
        let (session_id, title, cwd) = super::caller_identity(&ctx).await?;
        let bus = super::open_bus()?;
        super::ensure_registered(&bus, &session_id, title, cwd)?;
        bus.claim(&session_id, &input.role)
            .map_err(super::bus_tool_error)?;
        Ok(SessionsClaimOutput {
            role: input.role,
            session_id,
        })
    }
}
