use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::*;
use crate::{choice_q, noul_q, score_q};

fn questions() -> BTreeMap<String, Question> {
    let mut qs = BTreeMap::new();
    qs.insert("ok".to_owned(), noul_q("is this true?"));
    qs
}

#[test]
fn endpoint_for_hosted_vs_official() {
    assert_eq!(endpoint_for("jv_live_abc"), HOSTED_DECIDE_URL);
    assert_eq!(endpoint_for("sk-official"), OFFICIAL_DECIDE_URL);
    assert_eq!(endpoint_for("jv_test_abc"), OFFICIAL_DECIDE_URL);
    assert_eq!(endpoint_for(""), OFFICIAL_DECIDE_URL);
}

#[test]
fn client_timeout_is_thirty_seconds() {
    let client = Client::new("k").expect("client");
    assert_eq!(client.timeout(), Duration::from_secs(30));
    assert_eq!(TIMEOUT, Duration::from_secs(30));
}

#[test]
fn from_resolved_defaults_nudge_off() {
    let settings = Settings::from_resolved("k", None, None).expect("key");
    assert!(!settings.nudge);
    assert_eq!(settings.nudge_every, 2);
    assert!(
        !settings.nudge_active(),
        "a key alone must not enable the nudge"
    );
    assert!(!settings.safety_check);
    assert_eq!(settings.risk_block, crate::DEFAULT_RISK_BLOCK);
    assert!(!settings.allow_destructive);
    assert!(
        !settings.safety_active(),
        "a key alone must not enable the safety check"
    );
    assert!(!settings.context_filter);
    assert!(
        !settings.context_filter_active(),
        "a key alone must not enable the context filter"
    );
}

#[test]
fn nudge_active_requires_key_flag_and_positive_every() {
    let mut settings = Settings::from_resolved("k", None, None).expect("key");
    settings.nudge = true;
    assert!(settings.nudge_active());
    settings.nudge_every = 0;
    assert!(!settings.nudge_active());
    settings.nudge_every = -1;
    assert!(!settings.nudge_active());
    settings.nudge_every = 2;
    settings.nudge = false;
    assert!(!settings.nudge_active());
}

#[test]
fn safety_active_requires_key_and_flag() {
    let mut settings = Settings::from_resolved("k", None, None).expect("key");
    assert!(!settings.safety_active());
    settings.safety_check = true;
    assert!(settings.safety_active());
    settings.risk_block = 0.0;
    assert_eq!(settings.risk_block_or_default(), crate::DEFAULT_RISK_BLOCK);
    settings.risk_block = 0.9;
    assert_eq!(settings.risk_block_or_default(), 0.9);
}

#[test]
fn context_filter_active_requires_key_and_flag() {
    let mut settings = Settings::from_resolved("k", None, None).expect("key");
    assert!(!settings.context_filter_active());
    settings.context_filter = true;
    assert!(settings.context_filter_active());
}

#[test]
fn helpers_set_wire_types() {
    let choice = choice_q("pick", [("a", "alpha"), ("b", "beta")]);
    assert_eq!(choice.kind, "choice");
    assert_eq!(choice.criteria, Some(json!({"a": "alpha", "b": "beta"})));
    let score = score_q("rate", ["low", "high"]);
    assert_eq!(score.kind, "score");
    assert_eq!(score.criteria, Some(json!(["low", "high"])));
    let noul = noul_q("yes?");
    assert_eq!(noul.kind, "noul");
    assert!(noul.criteria.is_none());
}

#[tokio::test]
async fn missing_key_is_error_and_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"model": "x", "answers": {}})),
        )
        .expect(0)
        .mount(&server)
        .await;

    let mut client = Client::new("").expect("empty key is allowed until decide");
    client.base_url = Some(server.uri());
    let err = client
        .decide(&json!({"k": 1}), &questions())
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MissingKey));

    let mut blank = Client::new("   ").expect("whitespace key");
    blank.base_url = Some(server.uri());
    let err = blank.decide(&json!({}), &questions()).await.unwrap_err();
    assert!(matches!(err, Error::MissingKey));
    assert_eq!(err.kind(), crate::DecideErrorKind::MissingKey);
}

#[tokio::test]
async fn posts_bearer_and_json_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("Authorization", "Bearer test-key"))
        .and(header("Content-Type", "application/json"))
        .and(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            body["model"] == "jev-latest"
                && body["state"] == json!({"file": "main.rs"})
                && body["questions"]["ok"]["type"] == "noul"
                && body["questions"]["ok"]["instructions"] == "is this true?"
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.x",
            "answers": {
                "ok": {"type": "noul", "noul": 0.91, "reason": "looks good"}
            },
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    let out = client
        .decide(&json!({"file": "main.rs"}), &questions())
        .await
        .expect("decide");
    assert_eq!(out.model, "jev-1.x");
    assert_eq!(
        out.answers.get("ok").and_then(|v| v.get("noul")),
        Some(&json!(0.91))
    );
    assert_eq!(out.usage.as_ref().and_then(|u| u.input_tokens), Some(3));
}

#[tokio::test]
async fn decide_timed_records_usage_and_sizes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.x",
            "answers": {
                "ok": {"type": "noul", "noul": 0.91}
            },
            "usage": {"input_tokens": 3, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    let (timed, rec) = client
        .decide_timed(
            &json!({"file": "main.rs"}),
            &questions(),
            crate::DecideSource::AskJev,
        )
        .await;
    assert!(timed.is_ok());
    assert!(rec.ok);
    assert_eq!(rec.source, crate::DecideSource::AskJev);
    assert_eq!(rec.input_tokens, Some(3));
    assert!(rec.state_bytes > 0);
    assert!(rec.error_kind.is_none());
}

#[tokio::test]
async fn status_300_plus_is_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("upstream down"))
        .mount(&server)
        .await;

    let mut client = Client::new("k").expect("client");
    client.base_url = Some(server.uri());
    match client.decide(&json!({}), &questions()).await {
        Err(Error::Http { status, body }) => {
            assert_eq!(status, 502);
            assert!(body.contains("upstream down"));
        }
        other => panic!("expected http error, got {other:?}"),
    }
    let err = client.decide(&json!({}), &questions()).await.unwrap_err();
    assert_eq!(err.kind(), crate::DecideErrorKind::Http);
}

#[tokio::test]
async fn oversize_body_is_error() {
    let server = MockServer::start().await;
    let huge = "x".repeat(RESPONSE_BODY_CAP + 1);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(huge))
        .mount(&server)
        .await;

    let mut client = Client::new("k").expect("client");
    client.base_url = Some(server.uri());
    let err = client.decide(&json!({}), &questions()).await.unwrap_err();
    assert!(matches!(err, Error::ResponseTooLarge));
}
