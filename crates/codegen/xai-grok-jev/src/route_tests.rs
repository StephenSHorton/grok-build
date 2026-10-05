use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::client::{Client, Settings};

fn enabled_route() -> Settings {
    Settings::from_resolved("test-key", None, None).expect("key")
}

fn body(noul: f64, action: &str) -> serde_json::Value {
    json!({
        "model": "jev-1.x",
        "answers": {
            "trivial": { "type": "noul", "noul": noul },
            "action": { "type": "choice", "choice": action, "confidence": 0.9 }
        }
    })
}

async fn client_on(server: &MockServer) -> Client {
    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    client
}

#[test]
fn long_or_empty_or_image_skips() {
    assert!(should_skip_route("", false));
    assert!(should_skip_route("   ", false));
    assert!(should_skip_route("short", true));
    assert!(should_skip_route(&"x".repeat(QUERY_MAX_CHARS + 1), false));
    assert!(!should_skip_route("What is the git status?", false));
}

#[tokio::test]
async fn flag_off_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99, ACTION_NO_TOOLS)))
        .expect(0)
        .mount(&server)
        .await;

    let client = client_on(&server).await;
    let mut settings = enabled_route();
    settings.route = false;
    assert!(!settings.route_active());
    let verdict = maybe_route(
        Some(&settings),
        Some(&client),
        "What is the git status?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
}

#[tokio::test]
async fn no_settings_or_client_is_full_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99, ACTION_NO_TOOLS)))
        .expect(0)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    assert_eq!(
        maybe_route(None, Some(&client), "What time is it?", false).await,
        RouteVerdict::FullAgent
    );
    assert_eq!(
        maybe_route(Some(&enabled_route()), None, "What time is it?", false).await,
        RouteVerdict::FullAgent
    );
}

#[tokio::test]
async fn long_prompt_sends_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99, ACTION_NO_TOOLS)))
        .expect(0)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_route(
        Some(&enabled_route()),
        Some(&client),
        &"please fix this bug ".repeat(40),
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
}

#[tokio::test]
async fn live_no_tools_and_high_noul_omits_tools() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.91, ACTION_NO_TOOLS)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_route_recorded(
        Some(&enabled_route()),
        Some(&client),
        "What is the git status?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::NoTools { noul: 0.91 });
    assert!(verdict.omits_tools());
    let record = record.expect("decide ran");
    assert_eq!(record.source, crate::DecideSource::Route);
    assert!(record.ok);
}

#[tokio::test]
async fn live_no_tools_but_low_noul_is_full_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.40, ACTION_NO_TOOLS)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_route(
        Some(&enabled_route()),
        Some(&client),
        "What is the git status?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
}

#[tokio::test]
async fn live_full_agent_choice_is_full_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(0.99, ACTION_FULL_AGENT)))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_route(
        Some(&enabled_route()),
        Some(&client),
        "What is the git status?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
}

#[tokio::test]
async fn http_fault_is_full_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let (verdict, record) = maybe_route_recorded(
        Some(&enabled_route()),
        Some(&client),
        "What time is it?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
    let record = record.expect("fault is recorded");
    assert!(!record.ok);
    assert_eq!(record.source, crate::DecideSource::Route);
}

#[tokio::test]
async fn missing_answers_are_full_agent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.x",
            "answers": {}
        })))
        .expect(1)
        .mount(&server)
        .await;
    let client = client_on(&server).await;
    let verdict = maybe_route(
        Some(&enabled_route()),
        Some(&client),
        "What time is it?",
        false,
    )
    .await;
    assert_eq!(verdict, RouteVerdict::FullAgent);
}
