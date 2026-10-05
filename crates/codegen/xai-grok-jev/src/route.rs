//! Optional fail-open trivial-request router (speed-plan rank 5).
//!
//! Before the first sample, ask whether the prompt can be answered with no
//! tools. Any doubt, Jev fault, image, empty/long prompt, or missing answer
//! is [`RouteVerdict::FullAgent`]. This slice does not execute a tool itself.

use std::collections::BTreeMap;

use crate::client::{Client, Settings};
use crate::keep::clip_text;
use crate::metrics::{DecideRecord, DecideSource};
use crate::types::{Question, choice_q, decode_choice, decode_noul, noul_q};

/// Live noul must be at/above this to take [`RouteVerdict::NoTools`].
pub const DEFAULT_ROUTE_MIN: f64 = 0.80;

/// Skip the Decide call when the prompt is longer than this (already not trivial).
pub const QUERY_MAX_CHARS: usize = 400;

pub const TRIVIAL_ANSWER_NAME: &str = "trivial";
pub const ACTION_ANSWER_NAME: &str = "action";
pub const ACTION_FULL_AGENT: &str = "full_agent";
pub const ACTION_NO_TOOLS: &str = "no_tools";

pub const TRIVIAL_INSTRUCTIONS: &str =
    "Is this a trivial request the model can answer in one short turn with no tools?";

pub const ACTION_INSTRUCTIONS: &str =
    "If the request is trivial and needs no tools, pick no_tools. Otherwise full_agent.";

#[derive(Debug, Clone, PartialEq)]
pub enum RouteVerdict {
    FullAgent,
    NoTools { noul: f64 },
}

impl RouteVerdict {
    pub fn omits_tools(&self) -> bool {
        matches!(self, Self::NoTools { .. })
    }
}

/// True when calling Jev would be wasted: empty, image, or already long.
pub fn should_skip_route(query: &str, has_image: bool) -> bool {
    has_image || query.trim().is_empty() || query.chars().count() > QUERY_MAX_CHARS
}

pub fn trivial_question() -> Question {
    noul_q(TRIVIAL_INSTRUCTIONS)
}

pub fn action_question() -> Question {
    choice_q(
        ACTION_INSTRUCTIONS,
        [
            (
                ACTION_FULL_AGENT,
                "Needs tools, files, code, or more than one step",
            ),
            (
                ACTION_NO_TOOLS,
                "Answer from the prompt alone; no tools this sample",
            ),
        ],
    )
}

fn questions() -> BTreeMap<String, Question> {
    let mut questions = BTreeMap::new();
    questions.insert(TRIVIAL_ANSWER_NAME.to_owned(), trivial_question());
    questions.insert(ACTION_ANSWER_NAME.to_owned(), action_question());
    questions
}

/// Fail-open: only [`RouteVerdict::NoTools`] on a live `no_tools` plus noul ≥ `min`.
pub async fn route_request(
    client: &Client,
    query: &str,
    min_noul: f64,
) -> RouteVerdict {
    route_request_recorded(client, query, min_noul).await.0
}

/// [`route_request`] plus a Decide record. Skip cases return `(FullAgent, None)`.
pub async fn route_request_recorded(
    client: &Client,
    query: &str,
    min_noul: f64,
) -> (RouteVerdict, Option<DecideRecord>) {
    if should_skip_route(query, false) {
        return (RouteVerdict::FullAgent, None);
    }
    let threshold = if min_noul > 0.0 {
        min_noul
    } else {
        DEFAULT_ROUTE_MIN
    };
    let state = serde_json::json!({
        "query": clip_text(query, QUERY_MAX_CHARS),
    });
    let (response, record) = client
        .decide_timed(&state, &questions(), DecideSource::Route)
        .await;
    let verdict = match response {
        Err(_) => RouteVerdict::FullAgent,
        Ok(response) => {
            let noul = response
                .answers
                .get(TRIVIAL_ANSWER_NAME)
                .and_then(|raw| decode_noul(raw).ok());
            let action = response
                .answers
                .get(ACTION_ANSWER_NAME)
                .and_then(|raw| decode_choice(raw).ok());
            match (noul, action) {
                (Some(noul), Some(action))
                    if action.choice == ACTION_NO_TOOLS && noul.noul >= threshold =>
                {
                    RouteVerdict::NoTools { noul: noul.noul }
                }
                _ => RouteVerdict::FullAgent,
            }
        }
    };
    (verdict, Some(record))
}

/// No-op full-agent when the flag is off, no key, or the client is missing.
pub async fn maybe_route(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    has_image: bool,
) -> RouteVerdict {
    maybe_route_recorded(settings, client, query, has_image)
        .await
        .0
}

/// [`maybe_route`] plus a record when a Decide actually ran.
pub async fn maybe_route_recorded(
    settings: Option<&Settings>,
    client: Option<&Client>,
    query: &str,
    has_image: bool,
) -> (RouteVerdict, Option<DecideRecord>) {
    if should_skip_route(query, has_image) {
        return (RouteVerdict::FullAgent, None);
    }
    let Some(settings) = settings.filter(|s| s.route_active()) else {
        return (RouteVerdict::FullAgent, None);
    };
    let Some(client) = client else {
        return (RouteVerdict::FullAgent, None);
    };
    route_request_recorded(client, query, DEFAULT_ROUTE_MIN).await
}

#[cfg(test)]
#[path = "route_tests.rs"]
mod tests;
