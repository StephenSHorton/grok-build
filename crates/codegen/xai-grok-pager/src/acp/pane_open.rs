//! When `sessions_open` completes, emit OSC 7880 `new=1` so suzuri splits a pane.

use std::collections::HashSet;
use std::path::PathBuf;

use agent_client_protocol as acp;
use xai_grok_tools::implementations::grok_build::sessions::SessionsOpenOutput;
use xai_grok_tools::types::output::ToolOutput;

use crate::host_split::HostForkSplit;
use crate::render::draw::EscapeWriter;

/// If `update` is a completed `sessions_open`, emit OSC 7880 once per session id.
/// Returns true when a sequence was written.
pub fn maybe_emit(
    writer: &EscapeWriter,
    update: &acp::SessionUpdate,
    emitted: &mut HashSet<String>,
    host_available: bool,
) -> bool {
    if !host_available {
        return false;
    }
    let Some(out) = output_from_update(update) else {
        return false;
    };
    if out.session_id.trim().is_empty() || !out.pane {
        return false;
    }
    let prompt = Some(out.prompt.clone()).filter(|p| !p.trim().is_empty());
    let Some(req) = HostForkSplit::for_new_session(
        out.session_id.clone(),
        PathBuf::from(&out.cwd),
        prompt,
        out.title.clone(),
    ) else {
        return false;
    };
    if !emitted.insert(out.session_id.clone()) {
        return false;
    }
    writer.emit(crate::host_split::encode_osc(&req));
    true
}

fn output_from_update(update: &acp::SessionUpdate) -> Option<SessionsOpenOutput> {
    let (status, raw_output) = match update {
        acp::SessionUpdate::ToolCall(tc) => (tc.status, tc.raw_output.as_ref()),
        acp::SessionUpdate::ToolCallUpdate(tcu) => (
            tcu.fields.status.unwrap_or(acp::ToolCallStatus::Pending),
            tcu.fields.raw_output.as_ref(),
        ),
        _ => return None,
    };
    if status != acp::ToolCallStatus::Completed {
        return None;
    }
    let raw = raw_output?;
    if let Ok(ToolOutput::SessionsOpen(o)) = serde_json::from_value(raw.clone()) {
        return Some(o);
    }
    serde_json::from_value::<SessionsOpenOutput>(raw.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn completed_open(output: SessionsOpenOutput) -> acp::SessionUpdate {
        acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new(Arc::from("t1")),
            acp::ToolCallUpdateFields::new()
                .status(Some(acp::ToolCallStatus::Completed))
                .raw_output(Some(
                    serde_json::to_value(ToolOutput::SessionsOpen(output)).expect("output"),
                )),
        ))
    }

    #[test]
    fn parses_sessions_open_output() {
        let update = completed_open(SessionsOpenOutput {
            session_id: "sess-new".into(),
            pane: true,
            title: Some("review".into()),
            cwd: "/tmp/proj".into(),
            prompt: "own the review".into(),
        });
        let out = output_from_update(&update).expect("output");
        assert_eq!(out.session_id, "sess-new");
        assert_eq!(out.prompt, "own the review");
        assert_eq!(out.title.as_deref(), Some("review"));
    }

    #[test]
    fn ignores_in_progress() {
        let update = acp::SessionUpdate::ToolCall(
            acp::ToolCall::new(
                acp::ToolCallId::new(Arc::from("t1")),
                "Open Grok conversation review",
            )
            .status(acp::ToolCallStatus::InProgress),
        );
        assert!(output_from_update(&update).is_none());
    }
}
