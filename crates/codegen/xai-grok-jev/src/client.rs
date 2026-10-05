//! HTTP Decide client. Fail-open is the caller's job; this layer returns errors.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use crate::types::{Question, Response};

/// Hosted Jev decide URL (`jv_live_*` keys).
pub const HOSTED_DECIDE_URL: &str = "https://jevtypesafeai.com/api/v1/decide";

/// Official TypeSafe decide URL (every other key).
pub const OFFICIAL_DECIDE_URL: &str = "https://api.typesafe.ai/v1/systemone";

pub const DEFAULT_MODEL: &str = "jev-latest";

/// Client-side HTTP timeout, matching Rock.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Max Decide response body (`io.LimitReader` 1<<20). Oversize is an error.
pub const RESPONSE_BODY_CAP: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("jev key is not set")]
    MissingKey,
    #[error("jev http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("jev response exceeded {RESPONSE_BODY_CAP} bytes")]
    ResponseTooLarge,
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Decode(#[from] serde_json::Error),
}

impl Error {
    pub fn kind(&self) -> crate::DecideErrorKind {
        match self {
            Self::MissingKey => crate::DecideErrorKind::MissingKey,
            Self::Http { .. } => crate::DecideErrorKind::Http,
            Self::ResponseTooLarge => crate::DecideErrorKind::ResponseTooLarge,
            Self::Request(err) if err.is_timeout() => crate::DecideErrorKind::Timeout,
            Self::Request(_) => crate::DecideErrorKind::Request,
            Self::Decode(_) => crate::DecideErrorKind::Decode,
        }
    }
}

/// Settings used to construct a [`Client`]. Empty / whitespace keys are disabled.
/// `nudge` and `route` default on with a key; `safety_check` and `context_filter` stay off.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    /// Optional self-validation nudge after a successful mutate. Default on with a key.
    pub nudge: bool,
    /// Emit the nudge every N successful mutates. `<= 0` disables. Default 2.
    pub nudge_every: i32,
    /// Optional fail-open destructive-call check. Default off.
    pub safety_check: bool,
    /// Live noul at/above this value may deny when [`Self::allow_destructive`] is false.
    /// `<= 0` means [`crate::DEFAULT_RISK_BLOCK`].
    pub risk_block: f64,
    /// When true, a live safety noul at/above `risk_block` does not deny. Default false.
    pub allow_destructive: bool,
    /// Optional fail-open context filter on older tool results. Default off.
    pub context_filter: bool,
    /// Optional fail-open trivial-request router before the first sample. Default on with a key.
    pub route: bool,
    /// Optional fail-open file pick after grep / list_dir. Default off.
    pub file_pick: bool,
    /// Optional fail-open done check after a successful mutate. Default off.
    pub done_check: bool,
}

impl Settings {
    /// `None` when `api_key` is empty or whitespace. Nudge and route default on; other extras stay off.
    pub fn from_resolved(
        api_key: impl Into<String>,
        base_url: Option<String>,
        model: Option<String>,
    ) -> Option<Self> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return None;
        }
        Some(Self {
            api_key,
            base_url: base_url.filter(|url| !url.trim().is_empty()),
            model: model.filter(|model| !model.trim().is_empty()),
            nudge: true,
            nudge_every: 2,
            safety_check: false,
            risk_block: crate::DEFAULT_RISK_BLOCK,
            allow_destructive: false,
            context_filter: false,
            route: true,
            file_pick: false,
            done_check: false,
        })
    }

    pub fn is_enabled(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// Nudge runs only with a key, `nudge = true`, and `nudge_every > 0`.
    pub fn nudge_active(&self) -> bool {
        self.is_enabled() && self.nudge && self.nudge_every > 0
    }

    /// Safety check runs only with a key and `safety_check = true`.
    pub fn safety_active(&self) -> bool {
        self.is_enabled() && self.safety_check
    }

    /// Context filter runs only with a key and `context_filter = true`.
    pub fn context_filter_active(&self) -> bool {
        self.is_enabled() && self.context_filter
    }

    /// Router runs only with a key and `route = true`.
    pub fn route_active(&self) -> bool {
        self.is_enabled() && self.route
    }

    /// File pick runs only with a key and `file_pick = true`.
    pub fn file_pick_active(&self) -> bool {
        self.is_enabled() && self.file_pick
    }

    /// Done check runs only with a key and `done_check = true`.
    pub fn done_check_active(&self) -> bool {
        self.is_enabled() && self.done_check
    }

    /// Rock `riskAt`: non-positive values fall back to [`crate::DEFAULT_RISK_BLOCK`].
    pub fn risk_block_or_default(&self) -> f64 {
        if self.risk_block > 0.0 {
            self.risk_block
        } else {
            crate::DEFAULT_RISK_BLOCK
        }
    }

    pub fn client(&self) -> Result<Client, Error> {
        let mut client = Client::new(self.api_key.clone())?;
        client.base_url = self.base_url.clone();
        client.model = self.model.clone();
        Ok(client)
    }
}

/// Decide client. Empty key is an error at [`Client::decide`]; no HTTP is sent.
#[derive(Debug, Clone)]
pub struct Client {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    http: reqwest::Client,
}

impl Client {
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        Ok(Self {
            api_key: api_key.into(),
            base_url: None,
            model: None,
            http: reqwest::Client::builder().timeout(TIMEOUT).build()?,
        })
    }

    pub fn timeout(&self) -> Duration {
        TIMEOUT
    }

    /// POST `{model, state, questions}`. Does not fabricate answers.
    pub async fn decide<S: Serialize>(
        &self,
        state: &S,
        questions: &BTreeMap<String, Question>,
    ) -> Result<Response, Error> {
        if self.api_key.trim().is_empty() {
            return Err(Error::MissingKey);
        }
        let url = self
            .base_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| endpoint_for(&self.api_key));
        let model = self
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .unwrap_or(DEFAULT_MODEL);

        #[derive(Serialize)]
        struct Body<'a, S: Serialize> {
            model: &'a str,
            state: &'a S,
            questions: &'a BTreeMap<String, Question>,
        }

        let response = self
            .http
            .post(&url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&Body {
                model,
                state,
                questions,
            })
            .send()
            .await?;

        let status = response.status();
        if let Some(len) = response.content_length()
            && len > RESPONSE_BODY_CAP as u64
        {
            return Err(Error::ResponseTooLarge);
        }
        let raw = response.bytes().await?;
        if raw.len() > RESPONSE_BODY_CAP {
            return Err(Error::ResponseTooLarge);
        }
        if status.as_u16() >= 300 {
            return Err(Error::Http {
                status: status.as_u16(),
                body: truncate_lossy(&raw, 300),
            });
        }
        Ok(serde_json::from_slice(&raw)?)
    }

    /// [`Self::decide`] plus a size/latency record. Never stores `state` contents.
    pub async fn decide_timed<S: Serialize>(
        &self,
        state: &S,
        questions: &BTreeMap<String, Question>,
        source: crate::DecideSource,
    ) -> (Result<Response, Error>, crate::DecideRecord) {
        let started = std::time::Instant::now();
        let state_bytes = crate::state_byte_len(state);
        let modes: Vec<String> = questions.values().map(|q| q.kind.clone()).collect();
        let question_count = questions.len() as u32;
        let result = self.decide(state, questions).await;
        let latency_ms = started.elapsed().as_millis() as u64;
        let record = match &result {
            Ok(res) => crate::DecideRecord::from_parts(
                source,
                latency_ms,
                true,
                None,
                question_count,
                modes,
                res.usage.as_ref(),
                state_bytes,
            ),
            Err(err) => crate::DecideRecord::from_parts(
                source,
                latency_ms,
                false,
                Some(err.kind()),
                question_count,
                modes,
                None,
                state_bytes,
            ),
        };
        (result, record)
    }
}

/// Hosted `jv_live_*` keys vs official TypeSafe keys (Rock `EndpointFor`).
pub fn endpoint_for(key: &str) -> String {
    if key.starts_with("jv_live_") {
        HOSTED_DECIDE_URL.to_owned()
    } else {
        OFFICIAL_DECIDE_URL.to_owned()
    }
}

fn truncate_lossy(bytes: &[u8], max_chars: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i == max_chars {
            break;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
