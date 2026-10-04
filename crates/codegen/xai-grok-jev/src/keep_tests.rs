use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::client::{Client, Settings};

fn enabled_filter() -> Settings {
    let mut settings = Settings::from_resolved("test-key", None, None).expect("key");
    settings.context_filter = true;
    settings
}

fn noul_body(noul: f64) -> serde_json::Value {
    json!({
        "model": "jev-1.x",
        "answers": {
            "keep": { "type": "noul", "noul": noul }
        }
    })
}

async fn client_on(server: &MockServer) -> Client {
    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    client
}

#[tokio::test]
async fn flag_off_sends_no_http_and_keeps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.01)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let settings = Settings::from_resolved("test-key", None, None).expect("key");
    assert!(!settings.context_filter_active());
    assert!(maybe_keep_snippet(Some(&settings), Some(&client), "q", "old tool output").await);
}

#[tokio::test]
async fn no_settings_sends_no_http_and_keeps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.01)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(maybe_keep_snippet(None, Some(&client), "q", "old").await);
}

#[tokio::test]
async fn missing_client_keeps() {
    assert!(maybe_keep_snippet(Some(&enabled_filter()), None, "q", "old").await);
}

#[tokio::test]
async fn empty_snippet_keeps_without_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.01)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(keep_snippet(&client, "q", "   ", DEFAULT_MIN_CONFIDENCE).await);
}

#[tokio::test]
async fn live_keep_false_drops() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.10)))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(
        !maybe_keep_snippet(
            Some(&enabled_filter()),
            Some(&client),
            "the bug",
            "unrelated logs"
        )
        .await
    );
}

#[tokio::test]
async fn live_keep_true_keeps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(noul_body(0.91)))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(
        maybe_keep_snippet(
            Some(&enabled_filter()),
            Some(&client),
            "the bug",
            "stack trace"
        )
        .await
    );
}

#[tokio::test]
async fn http_502_keeps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("upstream down"))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(maybe_keep_snippet(Some(&enabled_filter()), Some(&client), "q", "old").await);
}

#[tokio::test]
async fn missing_answer_keeps() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"model": "jev-1.x", "answers": {}})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    assert!(maybe_keep_snippet(Some(&enabled_filter()), Some(&client), "q", "old").await);
}

#[test]
fn clips_stay_under_jev_budget() {
    let query = "q".repeat(QUERY_CLIP_CHARS + 50);
    let snippet = "s".repeat(SNIPPET_CLIP_CHARS + 50);
    assert_eq!(
        clip_text(&query, QUERY_CLIP_CHARS).chars().count(),
        QUERY_CLIP_CHARS
    );
    assert_eq!(
        clip_text(&snippet, SNIPPET_CLIP_CHARS).chars().count(),
        SNIPPET_CLIP_CHARS
    );
    assert!(QUERY_CLIP_CHARS + SNIPPET_CLIP_CHARS < 32_000);
}
