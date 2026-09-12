//! Maps `sessions_send` tool calls to the same compact SentMessage row as inbound peer mail.

use agent_client_protocol as acp;
use xai_grok_tools::implementations::grok_build::sessions::{
    SESSIONS_SEND_TOOL_NAME, SessionsSendInput, SessionsSendOutput,
};
use xai_grok_tools::tool_taxonomy::{CanonicalToolMeta, TOOL_META_KEY, TOOL_META_VERSION};
use xai_grok_tools::types::output::ToolOutput;
use xai_grok_tools::types::tool::ToolKind;

use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::tool::{
    SentMessagePresentation, SentMessageToolCallBlock, ToolCallBlock,
};

pub(super) fn is_tool(tool_call: &acp::ToolCall) -> bool {
    match tool_call
        .meta
        .as_ref()
        .and_then(|meta| meta.get(TOOL_META_KEY))
    {
        Some(meta) => serde_json::from_value::<CanonicalToolMeta>(meta.clone()).is_ok_and(|meta| {
            meta.version == TOOL_META_VERSION && meta.kind == ToolKind::SessionsSend
        }),
        None => {
            tool_call.title == SESSIONS_SEND_TOOL_NAME
                || tool_call.title.starts_with("Send to Grok conversation")
                || tool_call
                    .raw_input
                    .as_ref()
                    .and_then(|v| v.get("variant"))
                    .and_then(|v| v.as_str())
                    == Some("SessionsSend")
        }
    }
}

pub(super) fn to_block(tool_call: &acp::ToolCall) -> RenderBlock {
    let input = tool_call.raw_input.clone().and_then(|input| {
        serde_json::from_value::<SessionsSendInput>(input.clone())
            .ok()
            .or_else(|| {
                match serde_json::from_value::<xai_grok_tools::types::ToolInput>(input).ok()? {
                    xai_grok_tools::types::ToolInput::SessionsSend(input) => Some(input),
                    _ => None,
                }
            })
    });
    let output =
        tool_call
            .raw_output
            .clone()
            .and_then(
                |output| match serde_json::from_value::<ToolOutput>(output).ok()? {
                    ToolOutput::SessionsSend(output) => Some(output),
                    _ => None,
                },
            );
    let presentation = presentation(tool_call, output.as_ref());
    let (to, text) = input.map_or((None, None), |input| (Some(input.to), Some(input.content)));
    let label = output
        .as_ref()
        .and_then(|o| o.to_title.clone())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            to.as_deref().map(|to| {
                xai_grok_tools::implementations::grok_build::sessions::peer_display_label(to, None)
            })
        });

    RenderBlock::ToolCall(ToolCallBlock::SentMessage(SentMessageToolCallBlock::peer(
        presentation,
        label,
        text,
    )))
}

fn presentation(
    tool_call: &acp::ToolCall,
    output: Option<&SessionsSendOutput>,
) -> SentMessagePresentation {
    if !is_terminal(tool_call.status) {
        return SentMessagePresentation::Sending;
    }
    match output {
        Some(_) => SentMessagePresentation::Sent,
        None if tool_call.status == acp::ToolCallStatus::Failed => {
            SentMessagePresentation::Rejected {
                reason: content_text(tool_call).unwrap_or_else(|| "Send failed".to_owned()),
            }
        }
        None => SentMessagePresentation::Rejected {
            reason: content_text(tool_call).unwrap_or_else(|| {
                "Message was not delivered or delivery details are unavailable.".to_owned()
            }),
        },
    }
}

fn is_terminal(status: acp::ToolCallStatus) -> bool {
    matches!(
        status,
        acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
    )
}

fn content_text(tool_call: &acp::ToolCall) -> Option<String> {
    let text = tool_call
        .content
        .iter()
        .filter_map(|c| match c {
            acp::ToolCallContent::Content(chunk) => match &chunk.content {
                acp::ContentBlock::Text(t) => Some(t.text.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::scrollback::block::BlockContent;
    use crate::scrollback::types::{BlockContext, DisplayMode, line_plain_text};

    fn call(
        status: acp::ToolCallStatus,
        input: Option<serde_json::Value>,
        output: Option<SessionsSendOutput>,
    ) -> acp::ToolCall {
        acp::ToolCall::new(
            acp::ToolCallId::new(Arc::from("t1")),
            "Send to Grok conversation pr-reviews".to_string(),
        )
        .kind(acp::ToolKind::Other)
        .status(status)
        .raw_input(input)
        .raw_output(
            output.map(|output| {
                serde_json::to_value(ToolOutput::SessionsSend(output)).expect("output")
            }),
        )
    }

    fn collapsed_text(tc: &acp::ToolCall) -> String {
        let RenderBlock::ToolCall(ToolCallBlock::SentMessage(block)) = to_block(tc) else {
            panic!("expected SentMessage");
        };
        block
            .output(&BlockContext {
                width: 80,
                mode: DisplayMode::Collapsed,
                is_running: false,
                raw: false,
                max_lines: None,
                appearance: Default::default(),
                is_selected: false,
                cwd: None,
            })
            .lines
            .iter()
            .map(|line| line_plain_text(&line.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn title_fallback_is_recognized() {
        let tc = call(acp::ToolCallStatus::Pending, None, None);
        assert!(is_tool(&tc));
    }

    #[test]
    fn collapsed_row_uses_arrow_and_destination() {
        let tc = call(
            acp::ToolCallStatus::Completed,
            Some(serde_json::json!({
                "to": "pr-reviews",
                "content": "please review https://github.com/org/repo/pull/1"
            })),
            Some(SessionsSendOutput {
                delivered: xai_grok_tools::implementations::grok_build::sessions::Delivery::Inject,
                message_id: "m1".into(),
                to_session: "abc".into(),
                to_title: Some("pr-reviews".into()),
            }),
        );
        let text = collapsed_text(&tc);
        assert!(
            text.starts_with("\u{2192} pr-reviews  \u{00b7}  please review"),
            "collapsed outbound row was {text:?}"
        );
        assert!(!text.contains("Send to Grok conversation"));
    }

    #[test]
    fn session_id_destination_uses_to_title() {
        let tc = call(
            acp::ToolCallStatus::Completed,
            Some(serde_json::json!({
                "to": "01a09207-8741-7d22-b7ae-32cb89fc3cb9",
                "content": "Posted REVIEW on the PR"
            })),
            Some(SessionsSendOutput {
                delivered: xai_grok_tools::implementations::grok_build::sessions::Delivery::Inject,
                message_id: "m1".into(),
                to_session: "01a09207-8741-7d22-b7ae-32cb89fc3cb9".into(),
                to_title: Some("smoke".into()),
            }),
        );
        let text = collapsed_text(&tc);
        assert!(text.contains("smoke"), "row was {text:?}");
        assert!(
            !text.contains("01a09207"),
            "raw session id must not be the label: {text:?}"
        );
    }
}
