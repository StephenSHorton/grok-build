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
