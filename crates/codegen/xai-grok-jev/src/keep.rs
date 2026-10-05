//! Optional fail-open snippet keep check (Rock `Gates.KeepSnippet`).
//!
//! Errors, timeouts, missing answers, empty snippets, and a disabled flag all
//! keep the item. State is clipped well under Jev's ~64k / ~32k budget.

use std::collections::BTreeMap;

use crate::client::{Client, Settings};
use crate::metrics::{DecideRecord, DecideSource, FilterOutcome};
use crate::types::{Question, decode_noul, noul_q};

/// Rock default `MinConfidence`.
pub const DEFAULT_MIN_CONFIDENCE: f64 = 0.55;

pub const KEEP_ANSWER_NAME: &str = "keep";

/// Rock: "Does this snippet help answer the query?"
pub const KEEP_INSTRUCTIONS: &str = "Does this still help?";

/// Rock `clip(query, 400)`.
pub const QUERY_CLIP_CHARS: usize = 400;

/// Rock `clip(snippet, 800)`. Combined with the query this stays far under 32k.
pub const SNIPPET_CLIP_CHARS: usize = 800;

pub fn keep_question() -> Question {
    noul_q(KEEP_INSTRUCTIONS)
}

pub fn clip_text(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i == max_chars {
            break;
        }
        out.push(ch);
    }
    out
}

/// `true` means keep the snippet. Fail-open on any Jev fault.
pub async fn keep_snippet(
    client: &Client,
    query: &str,
    snippet: &str,
    min_confidence: f64,
) -> bool {
    keep_snippet_recorded(client, query, snippet, min_confidence)
        .await
        .0
}

/// [`keep_snippet`] plus a record when a Decide ran. Empty snippets return `(true, None)`.
pub async fn keep_snippet_recorded(
    client: &Client,
    query: &str,
    snippet: &str,
    min_confidence: f64,
) -> (bool, Option<DecideRecord>) {
    if snippet.trim().is_empty() {
        return (true, None);
    }
    let snippet_chars = snippet.chars().count();
    let threshold = if min_confidence > 0.0 {
        min_confidence
    } else {
        DEFAULT_MIN_CONFIDENCE
    };
    let mut questions = BTreeMap::new();
    questions.insert(KEEP_ANSWER_NAME.to_owned(), keep_question());
    let state = serde_json::json!({
        "query": clip_text(query, QUERY_CLIP_CHARS),
        "snippet": clip_text(snippet, SNIPPET_CLIP_CHARS),
    });
    let (response, mut record) = client
        .decide_timed(&state, &questions, DecideSource::Filter)
        .await;
    let keep = match response {
        Err(_) => true,
        Ok(response) => match response
            .answers
            .get(KEEP_ANSWER_NAME)
            .and_then(|raw| decode_noul(raw).ok())
        {
            None => true,
            Some(noul) => noul.noul >= threshold,
        },
    };
    let outcome = if keep {
        FilterOutcome::Kept
    } else {
        FilterOutcome::Dropped
    };
    record = record.with_filter(outcome, snippet_chars);
    (keep, Some(record))
}

/// No-op keep when the flag is off, no key, or the client is missing.
pub async fn maybe_keep_snippet(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    snippet: &str,
) -> bool {
    maybe_keep_snippet_recorded(settings, client, query, snippet)
        .await
        .0
}

/// [`maybe_keep_snippet`] plus a record when a Decide actually ran.
pub async fn maybe_keep_snippet_recorded(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    snippet: &str,
) -> (bool, Option<DecideRecord>) {
    if !settings.is_some_and(|s| s.context_filter_active()) {
        return (true, None);
    }
    let Some(client) = client else {
        return (true, None);
    };
    keep_snippet_recorded(client, query, snippet, DEFAULT_MIN_CONFIDENCE).await
}

#[cfg(test)]
#[path = "keep_tests.rs"]
mod tests;
