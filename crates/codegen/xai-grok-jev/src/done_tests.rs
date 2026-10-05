use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::client::{Client, Settings};

fn enabled() -> Settings {
    let mut settings = Settings::from_resolved("test-key", None, None).expect("key");
    settings.done_check = true;
    settings
}

fn body(noul: f64) -> serde_json::Value {
    json!({
        "model": "jev-1.x",
        "answers": {
            "done": { "type": "noul", "noul": noul }
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
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99)))
        .expect(0)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let settings = Settings::from_resolved("test-key", None, None).expect("key");
    assert!(!settings.done_check_active());
    let verdict = maybe_done_check(
        Some(&settings),
        Some(&client),
        "make the test pass",
        "test ok",
    )
    .await;
    assert_eq!(verdict, DoneVerdict::Continue);
}

#[tokio::test]
async fn empty_evidence_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99)))
        .expect(0)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_done_check(Some(&enabled()), Some(&client), "fix it", "   ").await;
    assert_eq!(verdict, DoneVerdict::Continue);
}

#[tokio::test]
async fn live_high_noul_ends() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.95)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_done_check_recorded(
        Some(&enabled()),
        Some(&client),
        "make the test pass",
        "1 passed",
    )
    .await;
    assert_eq!(verdict, DoneVerdict::End { noul: 0.95 });
    assert!(verdict.ends_turn());
    let record = record.expect("decide ran");
    assert_eq!(record.source, crate::DecideSource::DoneCheck);
    assert!(record.ok);
}

#[tokio::test]
async fn live_low_noul_continues() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.70)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_done_check(
        Some(&enabled()),
        Some(&client),
        "make the test pass",
        "1 passed",
    )
    .await;
    assert_eq!(verdict, DoneVerdict::Continue);
}

#[tokio::test]
async fn http_fault_continues() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_done_check_recorded(
        Some(&enabled()),
        Some(&client),
        "make the test pass",
        "1 passed",
    )
    .await;
    assert_eq!(verdict, DoneVerdict::Continue);
    assert!(!record.expect("recorded").ok);
}
