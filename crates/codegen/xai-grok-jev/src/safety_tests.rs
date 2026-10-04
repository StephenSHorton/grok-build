use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::client::{Client, Settings};

fn enabled_safety() -> Settings {
    let mut settings = Settings::from_resolved("test-key", None, None).expect("key");
    settings.safety_check = true;
    settings
}

fn noul_body(noul: f64) -> serde_json::Value {
    json!({
        "model": "jev-1.x",
        "answers": {
            "risk": { "type": "noul", "noul": noul }
        }
    })
}

async fn client_on(server: &MockServer) -> Client {
    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    client
}

#[tokio::test]
async fn flag_off_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.99)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let settings = Settings::from_resolved("test-key", None, None).expect("key");
    assert!(!settings.safety_active());
    let verdict = maybe_risk_check(
        Some(&settings),
        Some(&client),
        "run_terminal_command",
        r#"{"command":"rm -rf /"}"#,
    )
    .await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn no_settings_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.99)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let verdict = maybe_risk_check(None, Some(&client), "bash", "rm -rf /").await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn missing_client_is_allow() {
    let settings = enabled_safety();
    let verdict = maybe_risk_check(Some(&settings), None, "bash", "rm -rf /").await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn live_noul_above_threshold_denies() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.95)))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let verdict = maybe_risk_check(
        Some(&enabled_safety()),
        Some(&client),
        "run_terminal_command",
        r#"{"command":"rm -rf /tmp/x"}"#,
    )
    .await;
    assert_eq!(
        verdict,
        SafetyVerdict::Deny {
            noul: 0.95,
            threshold: DEFAULT_RISK_BLOCK
        }
    );
    assert_eq!(
        verdict.deny_detail().as_deref(),
        Some("Jev safety check (noul 0.95 ≥ 0.72)")
    );
}

#[tokio::test]
async fn live_noul_below_threshold_allows() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.20)))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let verdict = maybe_risk_check(
        Some(&enabled_safety()),
        Some(&client),
        "search_replace",
        "x",
    )
    .await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn http_502_allows() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("upstream down"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let verdict = maybe_risk_check(Some(&enabled_safety()), Some(&client), "bash", "rm").await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn missing_answer_allows() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"model": "jev-1.x", "answers": {}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let verdict = maybe_risk_check(Some(&enabled_safety()), Some(&client), "bash", "rm").await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[tokio::test]
async fn allow_destructive_does_not_deny() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.99)))
        .expect(1)
        .mount(&server)
        .await;

    let mut settings = enabled_safety();
    settings.allow_destructive = true;
    let client = client_on(&server).await;
    let verdict = maybe_risk_check(Some(&settings), Some(&client), "bash", "rm -rf /").await;
    assert_eq!(verdict, SafetyVerdict::Allow);
}

#[test]
fn clip_args_stays_under_budget() {
    let long = "x".repeat(ARGS_CLIP_CHARS + 20);
    assert_eq!(clip_args(&long).chars().count(), ARGS_CLIP_CHARS);
    assert_eq!(clip_args("short"), "short");
}

#[test]
fn risk_question_is_noul() {
    let q = risk_question();
    assert_eq!(q.kind, "noul");
    assert_eq!(q.instructions, RISK_INSTRUCTIONS);
    assert!(q.criteria.is_none());
}
