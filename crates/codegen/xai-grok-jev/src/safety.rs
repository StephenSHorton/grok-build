//! Optional fail-open destructive-call check (Rock `Gates.Risk` without the hard gate).
//!
//! Callers must not invoke this unless [`Settings::safety_active`] is true. Jev faults
//! always allow. There is no offline policy and no hard-coded deny.

use std::collections::BTreeMap;

use crate::client::{Client, Settings};
use crate::types::{Question, decode_noul, noul_q};

/// Rock default `RiskBlock`.
pub const DEFAULT_RISK_BLOCK: f64 = 0.72;

/// Wire question name (Rock `Gates.Risk`).
pub const RISK_ANSWER_NAME: &str = "risk";

/// Rock: "Is this tool call destructive or hard to undo?"
pub const RISK_INSTRUCTIONS: &str = "Is this tool call destructive or hard to undo?";

/// Rock `clip(args, 1500)`.
pub const ARGS_CLIP_CHARS: usize = 1500;

#[derive(Debug, Clone, PartialEq)]
pub enum SafetyVerdict {
    Allow,
    Deny { noul: f64, threshold: f64 },
}

impl SafetyVerdict {
    pub fn is_deny(&self) -> bool {
        matches!(self, Self::Deny { .. })
    }

    pub fn deny_detail(&self) -> Option<String> {
        match self {
            Self::Allow => None,
            Self::Deny { noul, threshold } => Some(format!(
                "Jev safety check (noul {noul:.2} ≥ {threshold:.2})"
            )),
        }
    }
}

/// Named noul question used by the optional safety check.
pub fn risk_question() -> Question {
    noul_q(RISK_INSTRUCTIONS)
}

pub fn clip_args(args: &str) -> String {
    let mut out = String::new();
    for (i, ch) in args.chars().enumerate() {
        if i == ARGS_CLIP_CHARS {
            break;
        }
        out.push(ch);
    }
    out
}

/// Fail-open gate. Returns [`SafetyVerdict::Allow`] unless a live noul is at/above
/// `risk_block` and `allow_destructive` is false.
pub async fn risk_check(
    client: &Client,
    tool: &str,
    args: &str,
    risk_block: f64,
    allow_destructive: bool,
) -> SafetyVerdict {
    let threshold = if risk_block > 0.0 {
        risk_block
    } else {
        DEFAULT_RISK_BLOCK
    };
    let mut questions = BTreeMap::new();
    questions.insert(RISK_ANSWER_NAME.to_owned(), risk_question());
    let state = serde_json::json!({
        "tool": tool,
        "args": clip_args(args),
    });
    let Ok(response) = client.decide(&state, &questions).await else {
        return SafetyVerdict::Allow;
    };
    let Some(raw) = response.answers.get(RISK_ANSWER_NAME) else {
        return SafetyVerdict::Allow;
    };
    let Ok(noul) = decode_noul(raw) else {
        return SafetyVerdict::Allow;
    };
    if !allow_destructive && noul.noul >= threshold {
        SafetyVerdict::Deny {
            noul: noul.noul,
            threshold,
        }
    } else {
        SafetyVerdict::Allow
    }
}

/// No-op when the flag is off, no key, or the client is missing. Never fabricates a deny.
pub async fn maybe_risk_check(
    settings: Option<&Settings>,
    client: Option<&Client>,
    tool: &str,
    args: &str,
) -> SafetyVerdict {
    let Some(settings) = settings.filter(|s| s.safety_active()) else {
        return SafetyVerdict::Allow;
    };
    let Some(client) = client else {
        return SafetyVerdict::Allow;
    };
    risk_check(
        client,
        tool,
        args,
        settings.risk_block_or_default(),
        settings.allow_destructive,
    )
    .await
}

#[cfg(test)]
#[path = "safety_tests.rs"]
mod tests;
