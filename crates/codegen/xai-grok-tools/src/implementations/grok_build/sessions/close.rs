use std::time::{Duration, Instant};

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

use super::store::pid_is_alive;

pub const SESSIONS_CLOSE_TOOL_NAME: &str = "sessions_close";

const DEFAULT_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsCloseInput {
    #[schemars(
        description = "Session id from sessions_list/sessions_open, or a duty role. Not this conversation."
    )]
    pub session: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsCloseOutput {
    pub session_id: String,
    /// True when the sibling process exited (hooks had a chance to run; suzuri then closes the pane).
    pub closed: bool,
    pub waited_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl xai_tool_runtime::ToolOutput for SessionsCloseOutput {}

#[derive(Debug, Default)]
pub struct SessionsCloseTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsCloseTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsSend
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Ask another Grok Build conversation to quit: SIGTERM so it runs session-end hooks, then its suzuri pane closes when the process exits. Use when the user asks to close a pane/session, or when a sibling you opened is finished. Not this conversation. Not Discord."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsCloseTool {
    type Args = SessionsCloseInput;
    type Output = SessionsCloseOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_CLOSE_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_CLOSE_TOOL_NAME,
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

    #[tracing::instrument(name = "tool.sessions_close", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsCloseInput,
    ) -> Result<SessionsCloseOutput, xai_tool_runtime::ToolError> {
        let target = input.session.trim();
        if target.is_empty() {
            return Err(xai_tool_runtime::ToolError::custom(
                "empty_session",
                "session is required",
            ));
        }
        let (from_session, _, _) = super::caller_identity(&ctx).await?;
        let bus = super::open_bus()?;
        let session_id = bus.resolve_target(target).map_err(super::bus_tool_error)?;
        if session_id == from_session {
            return Err(xai_tool_runtime::ToolError::custom(
                "cannot_close_self",
                "sessions_close cannot quit this conversation",
            ));
        }
        let title = bus.title_for_session(&session_id);
        let pid = bus
            .session_pid(&session_id)
            .map_err(super::bus_tool_error)?;
        let live = bus.is_live(&session_id);
        if !live && !pid.is_some_and(pid_is_alive) {
            return Ok(SessionsCloseOutput {
                session_id,
                closed: true,
                waited_ms: 0,
                title,
            });
        }
        let Some(pid) = pid.filter(|p| pid_is_alive(*p)) else {
            return Err(xai_tool_runtime::ToolError::custom(
                "no_pid",
                "session is marked live but has no pid to signal",
            ));
        };
        if pid == std::process::id() {
            return Err(xai_tool_runtime::ToolError::custom(
                "cannot_close_self",
                "sessions_close cannot quit this process",
            ));
        }
        send_term(pid)?;
        let started = Instant::now();
        loop {
            let gone = !bus.is_live(&session_id) && !pid_is_alive(pid);
            if gone {
                return Ok(SessionsCloseOutput {
                    session_id,
                    closed: true,
                    waited_ms: started.elapsed().as_millis() as u64,
                    title,
                });
            }
            if started.elapsed() >= DEFAULT_WAIT {
                return Ok(SessionsCloseOutput {
                    session_id,
                    closed: false,
                    waited_ms: started.elapsed().as_millis() as u64,
                    title,
                });
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

fn send_term(pid: u32) -> Result<(), xai_tool_runtime::ToolError> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        match signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            Ok(()) => Ok(()),
            Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(err) => Err(xai_tool_runtime::ToolError::custom(
                "signal",
                format!("SIGTERM {pid}: {err}"),
            )),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(xai_tool_runtime::ToolError::custom(
            "unsupported",
            "sessions_close needs SIGTERM (unix)",
        ))
    }
}
