//! MCP channel inbound: Claude-compatible `notifications/claude/channel` plus
//! the Grok alias `notifications/x.ai/channel`.
//!
//! A channel server (Discord, webhook, …) pushes these notifications over the
//! MCP client connection. Grok's `--channels` path injects them as same-session
//! turns. This module only parses and formats; it does not start a turn.

use serde_json::Value;

/// Claude Code's channel notification method. Discord/Telegram plugins emit this.
pub const CLAUDE_CHANNEL_METHOD: &str = "notifications/claude/channel";
/// Grok-native alias so servers that do not want the Claude method name still work.
pub const XAI_CHANNEL_METHOD: &str = "notifications/x.ai/channel";

/// True when `method` is a channel-inbound notification we know how to inject.
pub fn is_channel_method(method: &str) -> bool {
    method == CLAUDE_CHANNEL_METHOD || method == XAI_CHANNEL_METHOD
}

/// One inbound channel event, already validated enough to inject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelInbound {
    pub server: String,
    pub content: String,
    /// Identifier-keyed routing attributes (`chat_id`, `user`, …). Non-identifier
    /// keys are dropped here, matching Claude's channel contract.
    pub meta: Vec<(String, String)>,
}

impl ChannelInbound {
    /// Parse Claude/Grok channel notification params.
    ///
    /// `content` must be a string (empty is allowed). `meta` is optional; each
    /// value is stringified. Keys that are not XML-attribute identifiers are
    /// skipped.
    pub fn from_params(server: &str, params: &Value) -> Option<Self> {
        let obj = params.as_object()?;
        let content = match obj.get("content") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => return None,
        };
        let mut meta = Vec::new();
        if let Some(meta_val) = obj.get("meta") {
            match meta_val {
                Value::Object(map) => {
                    for (k, v) in map {
                        if !is_identifier(k) {
                            continue;
                        }
                        meta.push((k.clone(), json_to_attr(v)));
                    }
                }
                _ => {}
            }
        }
        Some(Self {
            server: server.to_string(),
            content,
            meta,
        })
    }

    /// Claude-shaped prompt body: `<channel source="…" …>content</channel>`.
    pub fn to_prompt(&self) -> String {
        let mut tag = format!("<channel source=\"{}\"", xml_attr(&self.server));
        for (k, v) in &self.meta {
            tag.push(' ');
            tag.push_str(k);
            tag.push_str("=\"");
            tag.push_str(&xml_attr(v));
            tag.push('"');
        }
        tag.push('>');
        if self.content.is_empty() {
            tag.push_str("</channel>");
            return tag;
        }
        format!("{tag}\n{}\n</channel>", xml_text(&self.content))
    }

    pub fn message_id(&self) -> Option<&str> {
        self.meta
            .iter()
            .find(|(k, _)| k == "message_id")
            .map(|(_, v)| v.as_str())
    }
}

fn json_to_attr(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Claude channel meta keys: letters, digits, underscore; must start with a letter or `_`.
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn xml_attr(s: &str) -> String {
    xml_escape(s, true)
}

fn xml_text(s: &str) -> String {
    xml_escape(s, false)
}

fn xml_escape(s: &str, attr: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attr => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn recognizes_claude_and_xai_methods() {
        assert!(is_channel_method(CLAUDE_CHANNEL_METHOD));
        assert!(is_channel_method(XAI_CHANNEL_METHOD));
        assert!(!is_channel_method("notifications/tools/list_changed"));
        assert!(!is_channel_method("notifications/elicitation/complete"));
    }

    #[test]
    fn parses_discord_shaped_payload() {
        let params = json!({
            "content": "hey grok, look at this",
            "meta": {
                "chat_id": "1547378432738988075",
                "message_id": "99",
                "user": "poppingpopper",
                "user_id": "402578266008846337",
                "ts": "2026-09-09T00:00:00.000Z",
                "hyphen-key": "dropped",
            }
        });
        let inbound = ChannelInbound::from_params("tsukumo", &params).unwrap();
        assert_eq!(inbound.server, "tsukumo");
        assert_eq!(inbound.content, "hey grok, look at this");
        assert_eq!(inbound.message_id(), Some("99"));
        assert!(!inbound.meta.iter().any(|(k, _)| k.contains('-')));
        let prompt = inbound.to_prompt();
        assert!(prompt.starts_with("<channel source=\"tsukumo\""));
        assert!(prompt.contains("chat_id=\"1547378432738988075\""));
        assert!(prompt.contains("user=\"poppingpopper\""));
        assert!(prompt.contains("hey grok, look at this"));
        assert!(prompt.ends_with("</channel>"));
    }

    #[test]
    fn rejects_missing_content() {
        assert!(ChannelInbound::from_params("s", &json!({"meta": {}})).is_none());
        assert!(ChannelInbound::from_params("s", &json!("not-an-object")).is_none());
    }

    #[test]
    fn escapes_xml_in_content_and_attrs() {
        let params = json!({
            "content": "<script>&",
            "meta": { "user": "a\"b" }
        });
        let prompt = ChannelInbound::from_params("srv", &params)
            .unwrap()
            .to_prompt();
        assert!(prompt.contains("&lt;script&gt;&amp;"));
        assert!(prompt.contains("user=\"a&quot;b\""));
    }

    #[test]
    fn empty_content_still_emits_tag() {
        let inbound = ChannelInbound::from_params("s", &json!({"content": ""})).unwrap();
        assert_eq!(inbound.to_prompt(), "<channel source=\"s\"></channel>");
    }
}
