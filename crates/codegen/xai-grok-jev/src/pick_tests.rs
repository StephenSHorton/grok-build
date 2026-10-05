use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::client::{Client, Settings};

fn enabled() -> Settings {
    let mut settings = Settings::from_resolved("test-key", None, None).expect("key");
    settings.file_pick = true;
    settings
}

fn body(choice: &str) -> serde_json::Value {
    json!({
        "model": "jev-1.x",
        "answers": {
            "pick": { "type": "choice", "choice": choice, "confidence": 0.9 }
        }
    })
}

async fn client_on(server: &MockServer) -> Client {
    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    client
}

#[test]
fn candidates_need_two_and_cap_at_eight() {
    assert!(candidates(["only"]).is_empty());
    assert_eq!(candidates(["a", "b"]).len(), 2);
    let many: Vec<String> = (0..20).map(|i| format!("f{i}")).collect();
    assert_eq!(candidates(many).len(), MAX_CANDIDATES);
}

#[tokio::test]
async fn flag_off_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body("src/a.rs")))
        .expect(0)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let settings = Settings::from_resolved("test-key", None, None).expect("key");
    assert!(!settings.file_pick_active());
    let verdict = maybe_pick_file(
        Some(&settings),
        Some(&client),
        &["src/a.rs".into(), "src/b.rs".into()],
    )
    .await;
    assert_eq!(verdict, FilePickVerdict::NoPick);
}

#[tokio::test]
async fn live_known_path_is_pick() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body("src/billing.rs")))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_pick_file_recorded(
        Some(&enabled()),
        Some(&client),
        &["src/a.rs".into(), "src/billing.rs".into()],
    )
    .await;
    assert_eq!(verdict, FilePickVerdict::Pick("src/billing.rs".into()));
    let record = record.expect("decide ran");
    assert_eq!(record.source, crate::DecideSource::FilePick);
    assert!(record.ok);
}

#[tokio::test]
async fn live_none_or_unknown_is_nopick() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(NONE_LABEL)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_pick_file(
        Some(&enabled()),
        Some(&client),
        &["src/a.rs".into(), "src/b.rs".into()],
    )
    .await;
    assert_eq!(verdict, FilePickVerdict::NoPick);
}

#[tokio::test]
async fn http_fault_is_nopick() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_pick_file_recorded(
        Some(&enabled()),
        Some(&client),
        &["src/a.rs".into(), "src/b.rs".into()],
    )
    .await;
    assert_eq!(verdict, FilePickVerdict::NoPick);
    assert!(!record.expect("recorded").ok);
}

#[test]
fn reminder_keeps_other_hits() {
    let text = reminder_text("src/billing.rs");
    assert!(text.contains("`src/billing.rs`"));
    assert!(text.contains("hint, not a filter"));
}
