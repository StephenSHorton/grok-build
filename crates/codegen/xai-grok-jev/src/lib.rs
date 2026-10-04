//! Minimal Jev decide client and `ask_jev` mapping (Rock `client.go` / `ask.go`).
//!
//! Callers must not construct [`Client`] unless `jev_enabled()` is true.

#![deny(clippy::indexing_slicing)]

mod ask;
mod client;
mod types;

pub use ask::{
    Answer, AskResult, FAILED_DETAIL, MISSING_ANSWER_DETAIL, Query, ValidateError, ask, ask_failed,
    validate_queries,
};
pub use client::{
    Client, Error, HOSTED_DECIDE_URL, OFFICIAL_DECIDE_URL, RESPONSE_BODY_CAP, Settings, TIMEOUT,
    endpoint_for,
};
pub use types::{
    Choice, Noul, Question, Response, Score, Usage, choice_q, decode_choice, decode_noul,
    decode_score, noul_q, score_q,
};
