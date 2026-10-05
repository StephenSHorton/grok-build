//! Agent-facing `ask_jev` mapping. Match Rock `internal/jev/ask.go`.
//!
//! Validate before HTTP. A failed Decide never fabricates a value.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::client::Client;
use crate::metrics::{DecideRecord, DecideSource};
use crate::types::{Usage, choice_q, decode_choice, decode_noul, decode_score, noul_q, score_q};

/// Agent-facing boolean (wire `noul`).
pub const MODE_BOOLEAN: &str = "boolean";
pub const MODE_CHOICE: &str = "choice";
pub const MODE_SCORE: &str = "score";

/// Set on every answer when Decide does not return one. Never invent a value to go with it.
pub const FAILED_DETAIL: &str = "Jev call failed, question not answered";

/// Live response omitted this named answer.
pub const MISSING_ANSWER_DETAIL: &str = "Jev returned no answer";

/// One agent question. [`ask`] batches them in a single Decide call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub name: String,
    pub question: String,
    pub mode: String,
    pub options: BTreeMap<String, String>,
    pub levels: Vec<String>,
}

/// One typed result the model can act on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Answer {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub probabilities: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub legend: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// `ask_jev` tool payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AskResult {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("{0}")]
pub struct ValidateError(pub String);

fn is_zero_f64(value: &f64) -> bool {
    *value == 0.0
}

impl Query {
    pub fn agent_mode(&self) -> String {
        match self.mode.trim().to_ascii_lowercase().as_str() {
            MODE_BOOLEAN | "noul" | "yes" | "yesno" | "yes/no" => MODE_BOOLEAN.to_owned(),
            MODE_CHOICE => MODE_CHOICE.to_owned(),
            MODE_SCORE => MODE_SCORE.to_owned(),
            other => other.to_owned(),
        }
    }
}

/// Check one question before any HTTP call.
pub fn validate_query(q: &Query) -> Result<(), ValidateError> {
    if q.question.trim().is_empty() {
        return Err(ValidateError("question is required".to_owned()));
    }
    match q.agent_mode().as_str() {
        MODE_BOOLEAN => Ok(()),
        MODE_CHOICE => {
            if q.options.is_empty() {
                return Err(ValidateError(format!(
                    "choice {:?} needs an options list",
                    q.name
                )));
            }
            if q.options.len() > 255 {
                return Err(ValidateError(format!(
                    "choice {:?} has {} options; Jev choice allows at most 255",
                    q.name,
                    q.options.len()
                )));
            }
            Ok(())
        }
        MODE_SCORE => {
            if q.levels.len() < 2 {
                return Err(ValidateError(format!(
                    "score {:?} needs a range or at least two levels",
                    q.name
                )));
            }
            if q.levels.len() > 10 {
                return Err(ValidateError(format!(
                    "score {:?} has {} levels; Jev score allows 2-10",
                    q.name,
                    q.levels.len()
                )));
            }
            Ok(())
        }
        _ => Err(ValidateError(format!(
            "unknown mode {:?} (want boolean, choice, or score)",
            q.mode
        ))),
    }
}

/// Check the batch before any HTTP call.
pub fn validate_queries(questions: &[Query]) -> Result<(), ValidateError> {
    if questions.is_empty() {
        return Err(ValidateError(
            "ask_jev needs at least one question".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for (i, q) in questions.iter().enumerate() {
        if q.name.trim().is_empty() {
            return Err(ValidateError(format!("questions[{i}] needs a name")));
        }
        if !seen.insert(q.name.clone()) {
            return Err(ValidateError(format!(
                "duplicate question name {:?}",
                q.name
            )));
        }
        validate_query(q)?;
    }
    Ok(())
}

fn wire_question(q: &Query) -> crate::types::Question {
    match q.agent_mode().as_str() {
        MODE_CHOICE => choice_q(&q.question, q.options.clone()),
        MODE_SCORE => score_q(&q.question, q.levels.clone()),
        _ => noul_q(&q.question),
    }
}

/// Run questions in one Decide call. A failed call sets `error` and leaves every
/// value empty — never a fabricated answer.
pub async fn ask<S: Serialize>(client: &Client, state: &S, questions: &[Query]) -> AskResult {
    ask_recorded(client, state, questions).await.0
}

/// [`ask`] plus a Decide record (sizes, latency, usage). `AskResult` JSON is unchanged.
pub async fn ask_recorded<S: Serialize>(
    client: &Client,
    state: &S,
    questions: &[Query],
) -> (AskResult, DecideRecord) {
    let mut qs = BTreeMap::new();
    for q in questions {
        qs.insert(q.name.clone(), wire_question(q));
    }
    let (res, mut record) = client.decide_timed(state, &qs, DecideSource::AskJev).await;
    record.modes = questions.iter().map(Query::agent_mode).collect();
    record.question_count = questions.len() as u32;
    let result = match res {
        Ok(res) => decode_live(questions, res),
        Err(err) => ask_failed(questions, &err.to_string()),
    };
    (result, record)
}

/// Same payload as a missing client (Rock: "jev client is not wired").
pub fn ask_failed(questions: &[Query], err: &str) -> AskResult {
    let mut out = AskResult {
        error: err.to_owned(),
        answers: BTreeMap::new(),
        ..AskResult::default()
    };
    for q in questions {
        out.answers.insert(
            q.name.clone(),
            Answer {
                mode: q.agent_mode(),
                question: nonempty_question(&q.question),
                detail: FAILED_DETAIL.to_owned(),
                ..Answer::default()
            },
        );
    }
    out
}

fn decode_live(questions: &[Query], res: crate::types::Response) -> AskResult {
    let mut out = AskResult {
        source: "live".to_owned(),
        model: res.model,
        answers: BTreeMap::new(),
        ..AskResult::default()
    };
    if let Some(usage) = res.usage
        && (usage.input_tokens.unwrap_or(0) != 0 || usage.output_tokens.unwrap_or(0) != 0)
    {
        out.usage = Some(usage);
    }
    for q in questions {
        let Some(raw) = res.answers.get(&q.name) else {
            out.answers.insert(
                q.name.clone(),
                Answer {
                    mode: q.agent_mode(),
                    question: nonempty_question(&q.question),
                    detail: MISSING_ANSWER_DETAIL.to_owned(),
                    ..Answer::default()
                },
            );
            continue;
        };
        out.answers.insert(q.name.clone(), decode_ask(q, raw));
    }
    out
}

fn decode_ask(q: &Query, raw: &serde_json::Value) -> Answer {
    let mut a = Answer {
        mode: q.agent_mode(),
        question: nonempty_question(&q.question),
        reason: extra_reason(raw),
        ..Answer::default()
    };
    match q.agent_mode().as_str() {
        MODE_CHOICE => match decode_choice(raw) {
            Ok(ch) => {
                a.value = Some(serde_json::Value::String(ch.choice));
                a.confidence = ch.confidence;
                a.probabilities = ch.probabilities;
            }
            Err(err) => a.detail = format!("decode: {err}"),
        },
        MODE_SCORE => match decode_score(raw) {
            Ok(score) => {
                a.value = Some(serde_json::json!(score.score));
                a.confidence = score.confidence;
                a.probabilities = score.probabilities;
                a.legend = score.legend;
            }
            Err(err) => a.detail = format!("decode: {err}"),
        },
        _ => match decode_noul(raw) {
            Ok(noul) => a.value = Some(serde_json::json!(noul.noul)),
            Err(err) => a.detail = format!("decode: {err}"),
        },
    }
    a
}

fn nonempty_question(question: &str) -> Option<String> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(question.to_owned())
    }
}

fn extra_reason(raw: &serde_json::Value) -> String {
    let reason = raw
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(reason) = reason {
        return reason.to_owned();
    }
    raw.get("explanation")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod tests;
