use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SessionEventsListInput {
    /// Session id (defaults to this session when omitted).
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SessionEventsDrainInput {
    /// Session id (defaults to this session when omitted).
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SessionEventsEnqueueInput {
    /// Target session id. Required unless `name` is set.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Named alias (e.g. `pm`) registered for a session.
    #[serde(default)]
    pub name: Option<String>,
    /// Producer id (free string), e.g. `agent.handoff`, `script.notify`.
    pub source: String,
    /// Kind (free string), e.g. `message`, `notify`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Event body (untrusted external content for the target session).
    pub body: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

fn default_kind() -> String {
    "message".into()
}
