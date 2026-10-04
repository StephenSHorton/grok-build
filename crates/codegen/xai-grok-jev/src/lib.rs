//! Minimal Jev decide client (Rock `internal/jev/client.go`).
//!
//! This crate is not linked into the pager/shell hot path yet. Callers must not
//! construct [`Client`] unless `jev_enabled()` is true.

#![deny(clippy::indexing_slicing)]

mod client;
mod types;

pub use client::{
    Client, Error, HOSTED_DECIDE_URL, OFFICIAL_DECIDE_URL, RESPONSE_BODY_CAP, TIMEOUT, endpoint_for,
};
pub use types::{
    Choice, Noul, Question, Response, Score, Usage, choice_q, decode_choice, decode_noul,
    decode_score, noul_q, score_q,
};
