//! `ask_jev` — typed boolean / choice / score questions via Jev.
//!
//! Registered only when the session builder is given [`JevSettings`]. Read-only
//! and allowed in plan mode. A failed Decide is an observation: the tool
//! returns JSON with `error` / `detail` and no fabricated `value`.

use std::collections::BTreeMap;

use xai_grok_jev::{AskResult, Query, ask_failed, ask_recorded, validate_queries};

pub use xai_grok_jev::{
    Client as JevClient, DecideRecord, JevMetrics, SafetyVerdict, Settings as JevSettings,
    maybe_keep_snippet, maybe_keep_snippet_recorded, maybe_risk_check, maybe_risk_check_recorded,
};

use crate::types::output::ToolOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::tool::{ToolKind, ToolNamespace};

pub const ASK_JEV_TOOL_NAME: &str = "ask_jev";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AskJevQuestion {
    #[schemars(description = "Unique name for this question in the result map.")]
    pub name: String,
    #[schemars(description = "The question to ask. Becomes Jev instructions.")]
    pub question: String,
    #[schemars(
        description = "boolean | choice | score. Aliases noul / yes / yesno / yes/no map to boolean (wire noul)."
    )]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Choice labels → meanings. Required for mode=choice, at most 255.")]
    pub options: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(description = "Score level descriptions. Required for mode=score, 2–10 items.")]
    pub levels: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AskJevInput {
    #[schemars(description = "Facts the questions are about. String or JSON object.")]
    pub state: serde_json::Value,
    #[schemars(description = "One or more questions, sent as a single Decide call.")]
    pub questions: Vec<AskJevQuestion>,
}

fn to_queries(input: &AskJevInput) -> Vec<Query> {
    input
        .questions
        .iter()
        .map(|q| Query {
            name: q.name.clone(),
            question: q.question.clone(),
            mode: q.mode.clone(),
            options: q.options.clone().unwrap_or_default(),
            levels: q.levels.clone().unwrap_or_default(),
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct AskJevTool;

impl crate::types::tool_metadata::ToolMetadata for AskJevTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Other
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Ask Jev, a typed judge, one or more boolean/choice/score questions about `state` in a single call. \
Boolean is a noul float (no yes/no threshold). Choice needs `options` (label → meaning). \
Score needs `levels` (2–10 descriptions). On failure the result has error/detail and no value — do not invent an answer. \
The turn continues after a failed call."
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for AskJevTool {
    type Args = AskJevInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new(ASK_JEV_TOOL_NAME).expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            ASK_JEV_TOOL_NAME,
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.ask_jev", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: AskJevInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        let queries = to_queries(&input);
        if let Err(err) = validate_queries(&queries) {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                err.to_string(),
            ));
        }

        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let client = {
            let res = resources.lock().await;
            res.get::<xai_grok_jev::Client>().cloned()
        };

        let result = match client {
            Some(client) => {
                let (result, record) = ask_recorded(&client, &input.state, &queries).await;
                {
                    let mut res = resources.lock().await;
                    if let Some(metrics) = res.get_mut::<JevMetrics>() {
                        metrics.record(record);
                    }
                }
                result
            }
            None => ask_failed(&queries, "jev client is not wired"),
        };
        Ok(ask_result_output(result))
    }
}

fn ask_result_output(result: AskResult) -> ToolOutput {
    ToolOutput::Dynamic(
        serde_json::to_value(&result)
            .unwrap_or_else(|_| serde_json::json!({"error": "failed to encode ask_jev result"}))
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::resources::Resources;
    use crate::types::tool_metadata::test_ctx_with_call_id;
    use serde_json::json;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use xai_grok_jev::{FAILED_DETAIL, MISSING_ANSWER_DETAIL};
    use xai_grok_permission_rules::types::AccessKind;

    fn q(name: &str, question: &str, mode: &str) -> AskJevQuestion {
        AskJevQuestion {
            name: name.into(),
            question: question.into(),
            mode: mode.into(),
            options: None,
            levels: None,
        }
    }

    fn dynamic_value(output: ToolOutput) -> serde_json::Value {
        match output {
            ToolOutput::Dynamic(d) => d.value,
            other => panic!("expected Dynamic output, got {other:?}"),
        }
    }

    #[test]
    fn metadata_is_read_only_other() {
        let tool = AskJevTool;
        assert_eq!(
            xai_tool_runtime::Tool::id(&tool).as_str(),
            ASK_JEV_TOOL_NAME
        );
        assert_eq!(
            crate::types::tool_metadata::ToolMetadata::kind(&tool),
            ToolKind::Other
        );
        assert!(crate::types::tool_metadata::ToolMetadata::is_read_only(
            &tool
        ));
        assert!(xai_tool_runtime::Tool::capabilities(&tool).is_read_only);
    }

    #[test]
    fn input_maps_to_read_access() {
        let input = crate::types::ToolInput::AskJev(AskJevInput {
            state: json!({"k": 1}),
            questions: vec![q("ok", "yes?", "boolean")],
        });
        assert!(matches!(AccessKind::from(&input), AccessKind::Read(None)));
    }

    #[tokio::test]
    async fn schema_error_is_invalid_args_and_sends_no_http() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"answers": {}})))
            .expect(0)
            .mount(&server)
            .await;

        let mut client = xai_grok_jev::Client::new("k").unwrap();
        client.base_url = Some(server.uri());
        let mut resources = Resources::new();
        resources.insert(client);
        let err = xai_tool_runtime::Tool::run(
            &AskJevTool,
            test_ctx_with_call_id(resources.into_shared(), "t"),
            AskJevInput {
                state: json!({}),
                questions: vec![],
            },
        )
        .await
        .expect_err("empty questions are invalid args");
        assert!(
            err.to_string()
                .contains("ask_jev needs at least one question"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn missing_client_is_observation_not_tool_error() {
        let resources = Resources::new();
        let output = xai_tool_runtime::Tool::run(
            &AskJevTool,
            test_ctx_with_call_id(resources.into_shared(), "t"),
            AskJevInput {
                state: json!({}),
                questions: vec![q("ok", "yes?", "boolean")],
            },
        )
        .await
        .expect("missing client must not abort the turn");
        let value = dynamic_value(output);
        assert_eq!(value["error"], "jev client is not wired");
        assert_eq!(value["answers"]["ok"]["detail"], FAILED_DETAIL);
        assert!(value["answers"]["ok"].get("value").is_none());
    }

    #[tokio::test]
    async fn live_mixed_and_http_failure() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.x",
                "answers": {
                    "ok": {"type": "noul", "noul": 0.4, "reason": "maybe"},
                    "pick": {"type": "choice", "choice": "a", "confidence": 0.5}
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = xai_grok_jev::Client::new("k").unwrap();
        client.base_url = Some(server.uri());
        let mut resources = Resources::new();
        resources.insert(client);
        let output = xai_tool_runtime::Tool::run(
            &AskJevTool,
            test_ctx_with_call_id(resources.into_shared(), "t"),
            AskJevInput {
                state: json!({"file": "a.rs"}),
                questions: vec![
                    q("ok", "fixed?", "boolean"),
                    AskJevQuestion {
                        name: "pick".into(),
                        question: "which?".into(),
                        mode: "choice".into(),
                        options: Some(BTreeMap::from([
                            ("a".into(), "alpha".into()),
                            ("b".into(), "beta".into()),
                        ])),
                        levels: None,
                    },
                    q("gone", "missing?", "noul"),
                ],
            },
        )
        .await
        .expect("live call");
        let value = dynamic_value(output);
        assert_eq!(value["source"], "live");
        assert_eq!(value["answers"]["ok"]["value"], json!(0.4));
        assert_eq!(value["answers"]["pick"]["value"], json!("a"));
        assert_eq!(value["answers"]["gone"]["detail"], MISSING_ANSWER_DETAIL);
        assert!(value["answers"]["gone"].get("value").is_none());

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502).set_body_string("no"))
            .expect(1)
            .mount(&server)
            .await;
        let mut client = xai_grok_jev::Client::new("k").unwrap();
        client.base_url = Some(server.uri());
        let mut resources = Resources::new();
        resources.insert(client);
        let output = xai_tool_runtime::Tool::run(
            &AskJevTool,
            test_ctx_with_call_id(resources.into_shared(), "t"),
            AskJevInput {
                state: json!({}),
                questions: vec![q("ok", "yes?", "boolean")],
            },
        )
        .await
        .expect("502 is an observation");
        let value = dynamic_value(output);
        assert!(!value["error"].as_str().unwrap_or("").is_empty());
        assert_eq!(value["answers"]["ok"]["detail"], FAILED_DETAIL);
        assert!(value["answers"]["ok"].get("value").is_none());
    }

    #[tokio::test]
    async fn live_call_records_metrics_without_changing_payload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "model": "jev-1.x",
                "answers": {
                    "ok": {"type": "noul", "noul": 0.4}
                },
                "usage": {"input_tokens": 5, "output_tokens": 1}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut client = xai_grok_jev::Client::new("k").unwrap();
        client.base_url = Some(server.uri());
        let mut resources = Resources::new();
        resources.insert(client);
        resources.insert(xai_grok_jev::JevMetrics::session_only());
        let shared = resources.into_shared();
        let output = xai_tool_runtime::Tool::run(
            &AskJevTool,
            test_ctx_with_call_id(shared.clone(), "t"),
            AskJevInput {
                state: json!({"file": "a.rs"}),
                questions: vec![q("ok", "fixed?", "boolean")],
            },
        )
        .await
        .expect("live call");
        let value = dynamic_value(output);
        assert_eq!(value["source"], "live");
        assert!(value.get("latency_ms").is_none());
        let guard = shared.lock().await;
        let metrics = guard.get::<xai_grok_jev::JevMetrics>().expect("metrics");
        assert_eq!(metrics.session().total.calls, 1);
        assert_eq!(metrics.session().total.ok, 1);
        assert_eq!(metrics.session().total.input_tokens, 5);
        assert_eq!(
            metrics
                .session()
                .by_source
                .get(&xai_grok_jev::DecideSource::AskJev)
                .map(|s| s.calls),
            Some(1)
        );
    }
}
