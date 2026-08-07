use super::types::{
    SessionEventsDrainInput, SessionEventsEnqueueInput, SessionEventsListInput,
};
use crate::types::output::ToolOutput;
use crate::types::resources::OwnerSessionId;
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::ToolMetadata;

pub const SESSION_EVENTS_LIST_TOOL_NAME: &str = "session_events_list";
pub const SESSION_EVENTS_DRAIN_TOOL_NAME: &str = "session_events_drain";
pub const SESSION_EVENTS_ENQUEUE_TOOL_NAME: &str = "session_events_enqueue";

async fn resolve_session_id(
    ctx: &xai_tool_runtime::ToolCallContext,
    explicit: Option<&str>,
) -> Result<String, xai_tool_runtime::ToolError> {
    if let Some(id) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(id.to_string());
    }
    use crate::types::tool_metadata::shared_resources;
    let resources = shared_resources(ctx)?;
    let guard = resources.lock().await;
    if let Some(owner) = guard.get::<OwnerSessionId>() {
        return Ok(owner.0.clone());
    }
    Err(xai_tool_runtime::ToolError::execution(
        xai_tool_protocol::ToolId::new(SESSION_EVENTS_LIST_TOOL_NAME).expect("valid"),
        "session_id required (no OwnerSessionId in context)".to_string(),
    ))
}

// --- list ---

#[derive(Debug, Default)]
pub struct SessionEventsListImpl;

impl ToolMetadata for SessionEventsListImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "List pending session events for a Grok session without consuming them. \
         Events live under ~/.grok/events/by-session/<id>/ (app-agnostic doorbell queue)."
    }
}

impl xai_tool_runtime::Tool for SessionEventsListImpl {
    type Args = SessionEventsListInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSION_EVENTS_LIST_TOOL_NAME).expect("valid")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSION_EVENTS_LIST_TOOL_NAME,
            ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionEventsListInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let sid = resolve_session_id(&ctx, input.session_id.as_deref()).await?;
        let events = xai_grok_session_events::list_pending(&sid).map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new(SESSION_EVENTS_LIST_TOOL_NAME).expect("valid"),
                e.to_string(),
            )
        })?;
        if events.is_empty() {
            return Ok(ToolOutput::Text(
                format!("No pending session events for {sid}.").into(),
            ));
        }
        let body = xai_grok_session_events::format_events_reminder(&events);
        Ok(ToolOutput::Text(
            format!("Pending (not drained) for {sid}:\n\n{body}").into(),
        ))
    }
}

// --- drain ---

#[derive(Debug, Default)]
pub struct SessionEventsDrainImpl;

impl ToolMetadata for SessionEventsDrainImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Batch-drain and delete all pending session events for a session. \
         Prefer letting the runtime auto-drain at turn start; use this to poll mid-turn. \
         Returns the drained batch as untrusted external content."
    }
}

impl xai_tool_runtime::Tool for SessionEventsDrainImpl {
    type Args = SessionEventsDrainInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSION_EVENTS_DRAIN_TOOL_NAME).expect("valid")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSION_EVENTS_DRAIN_TOOL_NAME,
            ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: SessionEventsDrainInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let sid = resolve_session_id(&ctx, input.session_id.as_deref()).await?;
        let events = xai_grok_session_events::drain_session(&sid).map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new(SESSION_EVENTS_DRAIN_TOOL_NAME).expect("valid"),
                e.to_string(),
            )
        })?;
        if events.is_empty() {
            return Ok(ToolOutput::Text(
                format!("No pending session events to drain for {sid}.").into(),
            ));
        }
        let body = xai_grok_session_events::format_events_reminder(&events);
        Ok(ToolOutput::Text(
            format!(
                "Drained {} event(s) for {sid} (queue cleared):\n\n{body}",
                events.len()
            )
            .into(),
        ))
    }
}

// --- enqueue ---

#[derive(Debug, Default)]
pub struct SessionEventsEnqueueImpl;

impl ToolMetadata for SessionEventsEnqueueImpl {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Enqueue a session event for another (or this) Grok session under ~/.grok/events/. \
         App-agnostic doorbell — does not start a turn by itself (soft inject drains at next turn). \
         Target with session_id or a registered name (e.g. pm)."
    }
}

impl xai_tool_runtime::Tool for SessionEventsEnqueueImpl {
    type Args = SessionEventsEnqueueInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(SESSION_EVENTS_ENQUEUE_TOOL_NAME).expect("valid")
    }

    fn description(
        &self,
        _ctx: &xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            SESSION_EVENTS_ENQUEUE_TOOL_NAME,
            ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    async fn run(
        &self,
        _ctx: xai_tool_runtime::ToolCallContext,
        input: SessionEventsEnqueueInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let sid = xai_grok_session_events::resolve_target(
            input.session_id.as_deref(),
            input.name.as_deref(),
        )
        .map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new(SESSION_EVENTS_ENQUEUE_TOOL_NAME).expect("valid"),
                e.to_string(),
            )
        })?;
        let mut event =
            xai_grok_session_events::Event::new(input.source, input.kind, input.body);
        event.title = input.title;
        event.data = input.data;
        let path = xai_grok_session_events::enqueue(
            &sid,
            event.clone(),
            &xai_grok_session_events::EnqueueOpts::default(),
        )
        .map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new(SESSION_EVENTS_ENQUEUE_TOOL_NAME).expect("valid"),
                e.to_string(),
            )
        })?;
        Ok(ToolOutput::Text(
            format!(
                "Enqueued event {} for session {sid}\npath: {}\npending: {}",
                event.id,
                path.display(),
                xai_grok_session_events::pending_count(&sid).unwrap_or(0)
            )
            .into(),
        ))
    }
}

