//! Sibling Grok conversations: list, claim a duty role, send a turn.

pub mod store;

mod claim;
mod list;
mod release;
mod send;

pub use claim::{
    SESSIONS_CLAIM_TOOL_NAME, SessionsClaimInput, SessionsClaimOutput, SessionsClaimTool,
};
pub use list::{SESSIONS_LIST_TOOL_NAME, SessionsListInput, SessionsListOutput, SessionsListTool};
pub use release::{
    SESSIONS_RELEASE_TOOL_NAME, SessionsReleaseInput, SessionsReleaseOutput, SessionsReleaseTool,
};
pub use send::{SESSIONS_SEND_TOOL_NAME, SessionsSendInput, SessionsSendOutput, SessionsSendTool};
pub use store::{
    BusError, Delivery, ListedSession, PeerMessage, SessionBus, Talkable, looks_like_session_id,
    peer_display_label, peer_message_to_prompt, read_live_frame, write_live_frame,
};

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::{Cwd, SessionFolder};

pub(crate) fn sessions_bundle_requires_expr() -> Expr<ToolRequirement> {
    Expr::And(vec![
        Expr::Value(ToolRequirement::tool::<list::SessionsListTool>()),
        Expr::Value(ToolRequirement::tool::<claim::SessionsClaimTool>()),
        Expr::Value(ToolRequirement::tool::<release::SessionsReleaseTool>()),
        Expr::Value(ToolRequirement::tool::<send::SessionsSendTool>()),
    ])
}

pub(crate) fn open_bus() -> Result<SessionBus, xai_tool_runtime::ToolError> {
    SessionBus::open_default().map_err(bus_tool_error)
}

pub(crate) fn bus_tool_error(err: BusError) -> xai_tool_runtime::ToolError {
    let code = match &err {
        BusError::InvalidRole(_) => "invalid_role",
        BusError::RoleHeld { .. } => "role_held",
        BusError::RoleNotFound(_) => "role_not_found",
        BusError::SessionNotFound(_) => "session_not_found",
        BusError::RoleNotHeld(_) => "role_not_held",
        BusError::Io(_) => "io",
        BusError::Json(_) => "json",
    };
    xai_tool_runtime::ToolError::custom(code, err.to_string())
}

pub(crate) async fn caller_identity(
    ctx: &xai_tool_runtime::ToolCallContext,
) -> Result<(String, Option<String>, Option<String>), xai_tool_runtime::ToolError> {
    let resources = crate::types::tool_metadata::shared_resources(ctx)?;
    let guard = resources.lock().await;
    let folder = guard.require::<SessionFolder>()?.0.clone();
    let cwd = guard.get::<Cwd>().map(|c| c.0.display().to_string());
    drop(guard);
    let session_id = folder
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            xai_tool_runtime::ToolError::custom(
                "missing_session",
                "this conversation has no session id",
            )
        })?
        .to_string();
    let title = read_summary_title(&folder);
    Ok((session_id, title, cwd))
}

fn read_summary_title(session_dir: &std::path::Path) -> Option<String> {
    let raw = std::fs::read_to_string(session_dir.join("summary.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("generated_title")
        .or_else(|| v.get("session_summary"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

pub(crate) fn ensure_registered(
    bus: &SessionBus,
    session_id: &str,
    title: Option<String>,
    cwd: Option<String>,
) -> Result<(), xai_tool_runtime::ToolError> {
    bus.register(session_id, title, cwd, None, Some(std::process::id()))
        .map_err(bus_tool_error)
}
