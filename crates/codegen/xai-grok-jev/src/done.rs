//! Optional fail-open done check after a successful mutate (speed-plan rank 2).
//!
//! Only [`DoneVerdict::End`] on a live noul at/above [`DEFAULT_DONE_MIN`].
//! Faults, missing answers, empty evidence, and low noul continue the turn.

use std::collections::BTreeMap;

use crate::client::{Client, Settings};
use crate::keep::clip_text;
use crate::metrics::{DecideRecord, DecideSource};
use crate::types::{Question, decode_noul, noul_q};

/// Conservative live threshold. Below this, keep sampling.
pub const DEFAULT_DONE_MIN: f64 = 0.90;

pub const QUERY_CLIP_CHARS: usize = 400;
pub const EVIDENCE_CLIP_CHARS: usize = 800;
pub const DONE_ANSWER_NAME: &str = "done";
pub const DONE_INSTRUCTIONS: &str =
    "Is the user request already satisfied given this evidence? Only yes if a concrete check shows it.";

#[derive(Debug, Clone, PartialEq)]
pub enum DoneVerdict {
    Continue,
    End { noul: f64 },
}

impl DoneVerdict {
    pub fn ends_turn(&self) -> bool {
        matches!(self, Self::End { .. })
    }
}

pub fn done_question() -> Question {
    noul_q(DONE_INSTRUCTIONS)
}

pub async fn done_check(
    client: &Client,
    query: &str,
    evidence: &str,
    min_noul: f64,
) -> DoneVerdict {
    done_check_recorded(client, query, evidence, min_noul)
        .await
        .0
}

pub async fn done_check_recorded(
    client: &Client,
    query: &str,
    evidence: &str,
    min_noul: f64,
) -> (DoneVerdict, Option<DecideRecord>) {
    if query.trim().is_empty() || evidence.trim().is_empty() {
        return (DoneVerdict::Continue, None);
    }
    let threshold = if min_noul > 0.0 {
        min_noul
    } else {
        DEFAULT_DONE_MIN
    };
    let mut questions = BTreeMap::new();
    questions.insert(DONE_ANSWER_NAME.to_owned(), done_question());
    let state = serde_json::json!({
        "query": clip_text(query, QUERY_CLIP_CHARS),
        "evidence": clip_text(evidence, EVIDENCE_CLIP_CHARS),
    });
    let (response, record) = client
        .decide_timed(&state, &questions, DecideSource::DoneCheck)
        .await;
    let verdict = match response {
        Err(_) => DoneVerdict::Continue,
        Ok(response) => match response
            .answers
            .get(DONE_ANSWER_NAME)
            .and_then(|raw| decode_noul(raw).ok())
        {
            Some(noul) if noul.noul >= threshold => DoneVerdict::End { noul: noul.noul },
            _ => DoneVerdict::Continue,
        },
    };
    (verdict, Some(record))
}

pub async fn maybe_done_check(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    evidence: &str,
) -> DoneVerdict {
    maybe_done_check_recorded(settings, client, query, evidence)
        .await
        .0
}

pub async fn maybe_done_check_recorded(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    evidence: &str,
) -> (DoneVerdict, Option<DecideRecord>) {
    if !settings.is_some_and(|s| s.done_check_active()) {
        return (DoneVerdict::Continue, None);
    }
    let Some(client) = client else {
        return (DoneVerdict::Continue, None);
    };
    done_check_recorded(client, query, evidence, DEFAULT_DONE_MIN).await
}

#[cfg(test)]
#[path = "done_tests.rs"]
mod tests;
