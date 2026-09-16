use std::path::Path;

use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const SESSIONS_OPEN_TOOL_NAME: &str = "sessions_open";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsOpenInput {
    #[schemars(
        description = "First user turn for the new conversation. This is not a fork of the current chat."
    )]
    pub prompt: String,
    #[serde(default)]
    #[schemars(description = "Pane title. Defaults to a short prefix of the prompt.")]
    pub title: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Working directory for the new session. Defaults to this conversation's cwd."
    )]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SessionsOpenOutput {
    pub session_id: String,
    /// Host advertised a suzuri pane; the pager emits OSC 7880 to split it.
    pub pane: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub cwd: String,
    pub prompt: String,
}

impl xai_tool_runtime::ToolOutput for SessionsOpenOutput {}

#[derive(Debug, Default)]
pub struct SessionsOpenTool;

impl crate::types::tool_metadata::ToolMetadata for SessionsOpenTool {
    fn kind(&self) -> ToolKind {
        ToolKind::SessionsSend
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Start a new Grok Build conversation in a live suzuri pane. Not a fork of this history and not a subagent: the child keeps its own context and stays open after the task. Returns session_id; talk to it with sessions_send. Do not wait for the user to /fork."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        super::sessions_bundle_requires_expr()
    }
}

impl xai_tool_runtime::Tool for SessionsOpenTool {
    type Args = SessionsOpenInput;
    type Output = SessionsOpenOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSIONS_OPEN_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSIONS_OPEN_TOOL_NAME,
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

    #[tracing::instrument(name = "tool.sessions_open", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionsOpenInput,
    ) -> Result<SessionsOpenOutput, xai_tool_runtime::ToolError> {
        let prompt = input.prompt.trim();
        if prompt.is_empty() {
            return Err(xai_tool_runtime::ToolError::custom(
                "empty_prompt",
                "prompt is required",
            ));
        }
        if !suzuri_pane_available() {
            return Err(xai_tool_runtime::ToolError::custom(
                "pane_unavailable",
                "sessions_open needs a suzuri pane (SUZURI=1). Use spawn_subagent for ephemeral work.",
            ));
        }
        let (_from_session, _from_title, caller_cwd) = super::caller_identity(&ctx).await?;
        let cwd = resolve_cwd(input.cwd.as_deref(), caller_cwd.as_deref())?;
        let title = resolve_title(input.title.as_deref(), prompt);
        let session_id = uuid::Uuid::now_v7().to_string();
        let bus = super::open_bus()?;
        bus.register(&session_id, title.clone(), Some(cwd.clone()), None, None)
            .map_err(super::bus_tool_error)?;
        Ok(SessionsOpenOutput {
            session_id,
            pane: true,
            title,
            cwd,
            prompt: prompt.to_string(),
        })
    }
}

fn suzuri_pane_available() -> bool {
    env_flag("SUZURI_FORK_SPLIT") || env_flag("SUZURI")
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

fn resolve_cwd(
    requested: Option<&str>,
    caller: Option<&str>,
) -> Result<String, xai_tool_runtime::ToolError> {
    if let Some(c) = requested.map(str::trim).filter(|s| !s.is_empty()) {
        let p = Path::new(c);
        if !p.is_absolute() {
            return Err(xai_tool_runtime::ToolError::custom(
                "cwd_not_absolute",
                "cwd must be an absolute path",
            ));
        }
        if !p.is_dir() {
            return Err(xai_tool_runtime::ToolError::custom(
                "cwd_missing",
                format!("cwd is not a directory: {c}"),
            ));
        }
        return Ok(c.to_string());
    }
    if let Some(c) = caller.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(c.to_string());
    }
    std::env::current_dir()
        .map(|p| p.display().to_string())
        .map_err(|e| xai_tool_runtime::ToolError::custom("cwd", e.to_string()))
}

fn resolve_title(requested: Option<&str>, prompt: &str) -> Option<String> {
    if let Some(t) = requested.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(t.to_string());
    }
    let line = prompt.lines().next().unwrap_or(prompt).trim();
    if line.is_empty() {
        return None;
    }
    const MAX: usize = 40;
    if line.chars().count() <= MAX {
        Some(line.to_string())
    } else {
        Some(format!(
            "{}…",
            line.chars().take(MAX.saturating_sub(1)).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_from_prompt_prefix() {
        assert_eq!(
            resolve_title(None, "review the auth PR please"),
            Some("review the auth PR please".into())
        );
        let long = "a".repeat(80);
        let t = resolve_title(None, &long).unwrap();
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 40);
    }

    #[test]
    fn title_prefers_explicit() {
        assert_eq!(
            resolve_title(Some("auth-review"), "a long prompt"),
            Some("auth-review".into())
        );
    }
}
