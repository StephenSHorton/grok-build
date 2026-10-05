//! Minimal Jev decide client and `ask_jev` mapping (Rock `client.go` / `ask.go`).
//!
//! Callers must not construct [`Client`] unless `jev_enabled()` is true.

#![deny(clippy::indexing_slicing)]

mod ask;
mod client;
mod keep;
mod metrics;
mod route;
mod safety;
mod types;

pub use ask::{
    Answer, AskResult, FAILED_DETAIL, MISSING_ANSWER_DETAIL, Query, ValidateError, ask, ask_failed,
    ask_recorded, validate_queries,
};
pub use client::{
    Client, Error, HOSTED_DECIDE_URL, OFFICIAL_DECIDE_URL, RESPONSE_BODY_CAP, Settings, TIMEOUT,
    endpoint_for,
};
pub use keep::{
    DEFAULT_MIN_CONFIDENCE, KEEP_ANSWER_NAME, KEEP_INSTRUCTIONS, QUERY_CLIP_CHARS,
    SNIPPET_CLIP_CHARS, clip_text, keep_question, keep_snippet, keep_snippet_recorded,
    maybe_keep_snippet, maybe_keep_snippet_recorded,
};
pub use route::{
    ACTION_ANSWER_NAME, ACTION_FULL_AGENT, ACTION_INSTRUCTIONS, ACTION_NO_TOOLS, DEFAULT_ROUTE_MIN,
    QUERY_MAX_CHARS, RouteVerdict, TRIVIAL_ANSWER_NAME, TRIVIAL_INSTRUCTIONS, action_question,
    maybe_route, maybe_route_recorded, route_request, route_request_recorded, should_skip_route,
    trivial_question,
};
pub use metrics::{
    DecideErrorKind, DecideRecord, DecideSource, FilterOutcome, JevMetrics, NUDGE_FOLLOW_WINDOW,
    STATS_LOG_MAX_BYTES, STATS_LOG_MAX_LINES, STATS_LOG_NAME, SafetyOutcome, SourceStats,
    StatsAccumulator, append_decide_log, format_stats_report, load_all_time, percentile_ms,
    state_byte_len, stats_log_path,
};
pub use safety::{
    ARGS_CLIP_CHARS, DEFAULT_RISK_BLOCK, RISK_ANSWER_NAME, RISK_INSTRUCTIONS, SafetyVerdict,
    clip_args, maybe_risk_check, maybe_risk_check_recorded, risk_check, risk_check_recorded,
    risk_question,
};
pub use types::{
    Choice, Noul, Question, Response, Score, Usage, choice_q, decode_choice, decode_noul,
    decode_score, noul_q, score_q,
};
