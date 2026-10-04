use std::collections::BTreeMap;

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

use super::*;
use crate::Client;

fn q_bool(name: &str, question: &str) -> Query {
    Query {
        name: name.to_owned(),
        question: question.to_owned(),
        mode: "boolean".to_owned(),
        options: BTreeMap::new(),
        levels: vec![],
    }
}

fn q_choice(name: &str, question: &str, options: &[(&str, &str)]) -> Query {
    Query {
        name: name.to_owned(),
        question: question.to_owned(),
        mode: "choice".to_owned(),
        options: options
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        levels: vec![],
    }
}

fn q_score(name: &str, question: &str, levels: &[&str]) -> Query {
    Query {
        name: name.to_owned(),
        question: question.to_owned(),
        mode: "score".to_owned(),
        options: BTreeMap::new(),
        levels: levels.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn no_value(answer: &Answer) {
    assert!(
        answer.value.is_none(),
        "must not fabricate a value, got {:?}",
        answer.value
    );
}

#[test]
fn aliases_map_to_boolean() {
    for mode in ["boolean", "noul", "yes", "yesno", "yes/no", "YES"] {
        let q = Query {
            name: "ok".into(),
            question: "yes?".into(),
            mode: mode.into(),
            options: BTreeMap::new(),
            levels: vec![],
        };
        assert_eq!(q.agent_mode(), MODE_BOOLEAN, "{mode}");
        validate_query(&q).expect("boolean aliases are valid");
    }
}

#[test]
fn validate_rejects_schema_errors() {
    assert_eq!(
        validate_queries(&[]).unwrap_err().0,
        "ask_jev needs at least one question"
    );
    assert_eq!(
        validate_queries(&[q_bool("", "q?")]).unwrap_err().0,
        "questions[0] needs a name"
    );
    assert_eq!(
        validate_queries(&[q_bool("a", "one"), q_bool("a", "two")])
            .unwrap_err()
            .0,
        "duplicate question name \"a\""
    );
    assert_eq!(
        validate_query(&q_bool("a", "  ")).unwrap_err().0,
        "question is required"
    );
    assert_eq!(
        validate_query(&q_choice("pick", "which?", &[]))
            .unwrap_err()
            .0,
        "choice \"pick\" needs an options list"
    );
    assert_eq!(
        validate_query(&q_score("rate", "how?", &["only"]))
            .unwrap_err()
            .0,
        "score \"rate\" needs a range or at least two levels"
    );
    let too_many_levels: Vec<String> = (0..11).map(|i| i.to_string()).collect();
    let mut long_score = q_score("rate", "how?", &[]);
    long_score.levels = too_many_levels;
    assert_eq!(
        validate_query(&long_score).unwrap_err().0,
        "score \"rate\" has 11 levels; Jev score allows 2-10"
    );
    let mut unknown = q_bool("a", "q?");
    unknown.mode = "maybe".into();
    assert_eq!(
        validate_query(&unknown).unwrap_err().0,
        "unknown mode \"maybe\" (want boolean, choice, or score)"
    );
}

#[tokio::test]
async fn schema_errors_send_no_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {}})))
        .expect(0)
        .mount(&server)
        .await;
    assert!(validate_queries(&[]).is_err());
    let _ = server.received_requests().await;
}

#[tokio::test]
async fn mixed_modes_one_batched_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(|req: &Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
            body["model"] == "jev-latest"
                && body["state"] == json!({"file": "main.rs"})
                && body["questions"]["ok"]["type"] == "noul"
                && body["questions"]["pick"]["type"] == "choice"
                && body["questions"]["rate"]["type"] == "score"
                && body["questions"].as_object().map(|m| m.len()) == Some(3)
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.x",
            "answers": {
                "ok": {"type": "noul", "noul": 0.8, "reason": "tests pass"},
                "pick": {
                    "type": "choice",
                    "choice": "a",
                    "confidence": 0.9,
                    "probabilities": {"a": 0.9, "b": 0.1},
                    "explanation": "a is the file"
                },
                "rate": {
                    "type": "score",
                    "score": 2.0,
                    "confidence": 0.7,
                    "legend": {"1": "low", "2": "high"},
                    "probabilities": {"1": 0.3, "2": 0.7}
                }
            },
            "usage": {"input_tokens": 11, "output_tokens": 4}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    let questions = [
        q_bool("ok", "is it fixed?"),
        q_choice("pick", "which file?", &[("a", "alpha"), ("b", "beta")]),
        q_score("rate", "how sure?", &["low", "high"]),
    ];
    validate_queries(&questions).unwrap();
    let result = ask(&client, &json!({"file": "main.rs"}), &questions).await;
    assert_eq!(result.source, "live");
    assert_eq!(result.model, "jev-1.x");
    assert!(result.error.is_empty());
    assert_eq!(result.answers["ok"].value, Some(json!(0.8)));
    assert_eq!(result.answers["ok"].reason, "tests pass");
    assert_eq!(result.answers["pick"].value, Some(json!("a")));
    assert_eq!(result.answers["pick"].reason, "a is the file");
    assert_eq!(result.answers["rate"].value, Some(json!(2.0)));
    assert_eq!(
        result.answers["rate"].legend.get("2").map(String::as_str),
        Some("high")
    );
    assert_eq!(result.usage.as_ref().and_then(|u| u.input_tokens), Some(11));
}

#[tokio::test]
async fn http_502_sets_failed_detail_and_no_value() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
        .expect(1)
        .mount(&server)
        .await;

    let mut client = Client::new("test-key").expect("client");
    client.base_url = Some(server.uri());
    let questions = [q_bool("ok", "yes?"), q_score("rate", "how?", &["a", "b"])];
    let result = ask(&client, &json!({}), &questions).await;
    assert!(!result.error.is_empty());
    assert!(result.source.is_empty());
    for name in ["ok", "rate"] {
        let a = &result.answers[name];
        assert_eq!(a.detail, FAILED_DETAIL);
        no_value(a);
    }
}

#[tokio::test]
async fn missing_named_answer_does_not_fabricate() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "jev-1.x",
            "answers": {
                "ok": {"type": "noul", "noul": 0.1}
            }
        })))
        .mount(&server)
        .await;

    let mut client = Client::new("k").expect("client");
    client.base_url = Some(server.uri());
    let questions = [q_bool("ok", "yes?"), q_bool("missing", "also?")];
    let result = ask(&client, &json!({}), &questions).await;
    assert_eq!(result.answers["ok"].value, Some(json!(0.1)));
    let missing = &result.answers["missing"];
    assert_eq!(missing.detail, MISSING_ANSWER_DETAIL);
    no_value(missing);
}

#[tokio::test]
async fn missing_key_is_failed_detail_without_http() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {}})))
        .expect(0)
        .mount(&server)
        .await;

    let mut client = Client::new("").expect("empty");
    client.base_url = Some(server.uri());
    let questions = [q_bool("ok", "yes?")];
    let result = ask(&client, &json!({}), &questions).await;
    assert_eq!(result.error, "jev key is not set");
    assert_eq!(result.answers["ok"].detail, FAILED_DETAIL);
    no_value(&result.answers["ok"]);
}
