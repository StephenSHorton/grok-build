//! Minimal Jev decide client and `ask_jev` mapping (Rock `client.go` / `ask.go`).
//!
//! Callers must not construct [`Client`] unless `jev_enabled()` is true.

#![deny(clippy::indexing_slicing)]

mod ask;
mod client;
mod keep;
mod safety;
mod types;

pub use ask::{
    Answer, AskResult, FAILED_DETAIL, MISSING_ANSWER_DETAIL, Query, ValidateError, ask, ask_failed,
    validate_queries,
};
pub use client::{
    Client, Error, HOSTED_DECIDE_URL, OFFICIAL_DECIDE_URL, RESPONSE_BODY_CAP, Settings, TIMEOUT,
    endpoint_for,
};
pub use keep::{
    DEFAULT_MIN_CONFIDENCE, KEEP_ANSWER_NAME, KEEP_INSTRUCTIONS, QUERY_CLIP_CHARS,
    SNIPPET_CLIP_CHARS, clip_text, keep_question, keep_snippet, maybe_keep_snippet,
};
pub use safety::{
    ARGS_CLIP_CHARS, DEFAULT_RISK_BLOCK, RISK_ANSWER_NAME, RISK_INSTRUCTIONS, SafetyVerdict,
    clip_args, maybe_risk_check, risk_check, risk_question,
};
pub use types::{
    Choice, Noul, Question, Response, Score, Usage, choice_q, decode_choice, decode_noul,
    decode_score, noul_q, score_q,
};
