//! Wire types for the Jev decide API. Match Rock `client.go` / `ask.go`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One named question in a Decide call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    #[serde(rename = "type")]
    pub kind: String,
    pub instructions: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<serde_json::Value>,
}

/// Choice answer on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    #[serde(rename = "type")]
    pub kind: String,
    pub choice: String,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub probabilities: BTreeMap<String, f64>,
}

/// Score answer on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    #[serde(rename = "type")]
    pub kind: String,
    pub score: f64,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub legend: BTreeMap<String, String>,
    #[serde(default)]
    pub probabilities: BTreeMap<String, f64>,
}

/// Noul (agent boolean) answer on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Noul {
    #[serde(rename = "type")]
    pub kind: String,
    pub noul: f64,
}

/// Optional token usage. Missing is not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
}

/// Decide response. Answers stay raw so later slices can copy `reason`/`explanation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Response {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub answers: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// Choice question: criteria is a label → meaning object.
pub fn choice_q(
    instructions: impl Into<String>,
    criteria: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> Question {
    let map: BTreeMap<String, String> = criteria
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    Question {
        kind: "choice".to_owned(),
        instructions: instructions.into(),
        criteria: Some(serde_json::to_value(map).unwrap_or(serde_json::Value::Null)),
    }
}

/// Score question: criteria is an array of 2–10 level descriptions.
pub fn score_q(
    instructions: impl Into<String>,
    levels: impl IntoIterator<Item = impl Into<String>>,
) -> Question {
    let levels: Vec<String> = levels.into_iter().map(Into::into).collect();
    Question {
        kind: "score".to_owned(),
        instructions: instructions.into(),
        criteria: Some(serde_json::to_value(levels).unwrap_or(serde_json::Value::Null)),
    }
}

/// Noul question (agent-facing boolean). No criteria.
pub fn noul_q(instructions: impl Into<String>) -> Question {
    Question {
        kind: "noul".to_owned(),
        instructions: instructions.into(),
        criteria: None,
    }
}

pub fn decode_choice(raw: &serde_json::Value) -> Result<Choice, serde_json::Error> {
    serde_json::from_value(raw.clone())
}

pub fn decode_score(raw: &serde_json::Value) -> Result<Score, serde_json::Error> {
    serde_json::from_value(raw.clone())
}

pub fn decode_noul(raw: &serde_json::Value) -> Result<Noul, serde_json::Error> {
    serde_json::from_value(raw.clone())
}
