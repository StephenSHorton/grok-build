//! Local Ollama discovery: probe a running daemon, merge pulled models into the
//! catalog, and pick a usage-exhausted fallback.
//!
//! Catalog keys are `ollama/<tag>` so they never collide with xAI slugs. The
//! routing slug sent to `/v1/chat/completions` is the bare Ollama tag.

use std::num::NonZeroU64;
use std::time::Duration;

use indexmap::IndexMap;

use super::config::{Config, DEFAULT_AGENT_TYPE, ModelEntry, ModelInfo, ModelsConfig};
use crate::sampling::ApiBackend;
use xai_grok_sampler::AuthScheme;

/// Prefix for auto-discovered catalog keys (`ollama/qwen3.8:27b`).
pub const CATALOG_PREFIX: &str = "ollama/";
/// ACP `ModelInfo.meta.provider` value for discovered entries.
pub const ACP_PROVIDER: &str = "ollama";
/// ACP meta key set on the model chosen as the usage-exhausted fallback.
pub const ACP_FALLBACK_META: &str = "ollamaFallback";
/// ACP meta key naming the provider (`"ollama"`).
pub const ACP_PROVIDER_META: &str = "provider";

const DEFAULT_HOST: &str = "http://127.0.0.1:11434";
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
/// Used only when neither the model nor `OLLAMA_CONTEXT_LENGTH` / config advertises a window.
const DEFAULT_CONTEXT_CAP: u64 = 32_768;
const DUMMY_API_KEY: &str = "ollama";

#[derive(Clone, Debug, Default)]
pub(crate) struct OllamaCache {
    pub models: Vec<DiscoveredOllamaModel>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredOllamaModel {
    pub name: String,
    pub size_bytes: u64,
    pub context_length: Option<u64>,
    pub capabilities: Vec<String>,
    pub parameter_size: Option<String>,
}

impl DiscoveredOllamaModel {
    pub fn catalog_key(&self) -> String {
        catalog_key(&self.name)
    }

    fn is_chat_model(&self) -> bool {
        if self.capabilities.is_empty() {
            return !looks_like_embed_only(&self.name);
        }
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("completion"))
    }

    fn supports_tools(&self) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("tools"))
    }
}

pub fn catalog_key(tag: &str) -> String {
    format!("{CATALOG_PREFIX}{tag}")
}

pub fn is_ollama_catalog_key(key: &str) -> bool {
    key.starts_with(CATALOG_PREFIX)
}

fn looks_like_embed_only(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("embed") || n.contains("nomic-embed")
}

/// `GROK_OLLAMA_DISCOVERY` > `[models].ollama_discovery` > on in production, off in tests.
pub fn discovery_enabled(models: &ModelsConfig) -> bool {
    if let Some(v) = xai_grok_env::env_bool("GROK_OLLAMA_DISCOVERY") {
        return v;
    }
    if let Some(v) = models.ollama_discovery {
        return v;
    }
    !cfg!(test)
}

/// Whether usage-exhausted failover is on. `off`/`false`/`0` disables; any other
/// non-empty pin (or unset) enables it when discovered models exist.
pub fn fallback_enabled(models: &ModelsConfig) -> bool {
    match fallback_pin(models) {
        FallbackPin::Off => false,
        FallbackPin::Auto | FallbackPin::Named(_) => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FallbackPin {
    Off,
    Auto,
    Named(String),
}

fn fallback_pin(models: &ModelsConfig) -> FallbackPin {
    let raw = xai_grok_env::env_string("GROK_OLLAMA_FALLBACK")
        .or_else(|| models.fallback_on_usage_exhausted.clone());
    match raw.as_deref().map(str::trim) {
        None | Some("") => FallbackPin::Auto,
        Some(v) if is_off_token(v) => FallbackPin::Off,
        Some(v) if is_on_token(v) => FallbackPin::Auto,
        Some(v) => FallbackPin::Named(v.to_owned()),
    }
}

fn is_off_token(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off" | "disabled" | "none"
    )
}

fn is_on_token(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enabled" | "auto"
    )
}

pub fn resolve_host(models: &ModelsConfig) -> String {
    let raw = xai_grok_env::env_string("GROK_OLLAMA_HOST")
        .or_else(|| xai_grok_env::env_string("OLLAMA_HOST"))
        .or_else(|| models.ollama_host.clone())
        .unwrap_or_else(|| DEFAULT_HOST.to_owned());
    normalize_host(&raw)
}

fn normalize_host(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return trimmed.to_owned();
    }
    format!("http://{trimmed}")
}

fn native_host(host: &str) -> String {
    host.trim_end_matches('/')
        .trim_end_matches("/v1")
        .to_owned()
}

fn openai_base(host: &str) -> String {
    let native = native_host(host);
    format!("{native}/v1")
}

fn parse_context_tokens(raw: &str) -> Option<u64> {
    let n: u64 = raw.trim().parse().ok()?;
    (n > 0).then_some(n)
}

/// Runtime num_ctx: grok-specific override, then config, then `OLLAMA_CONTEXT_LENGTH`,
/// then the macOS user-domain value Ollama.app reads via launchctl.
pub fn runtime_context_window(models: &ModelsConfig) -> Option<u64> {
    if let Some(v) = xai_grok_env::env_string("GROK_OLLAMA_CONTEXT")
        .or_else(|| xai_grok_env::env_string("GROK_OLLAMA_CONTEXT_LENGTH"))
        .as_deref()
        .and_then(parse_context_tokens)
    {
        return Some(v);
    }
    if let Some(v) = models.ollama_context_window.filter(|n| *n > 0) {
        return Some(v);
    }
    // Prefer the user-domain value Ollama.app actually runs with (may differ from a
    // stale OLLAMA_CONTEXT_LENGTH inherited by a long-lived grok process).
    if let Some(v) = macos_launchctl_context() {
        return Some(v);
    }
    xai_grok_env::env_string("OLLAMA_CONTEXT_LENGTH")
        .as_deref()
        .and_then(parse_context_tokens)
}

#[cfg(target_os = "macos")]
fn macos_launchctl_context() -> Option<u64> {
    let output = std::process::Command::new("launchctl")
        .args(["getenv", "OLLAMA_CONTEXT_LENGTH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_context_tokens(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "macos"))]
fn macos_launchctl_context() -> Option<u64> {
    None
}

/// Catalog window: min(model max, runtime num_ctx). No silent 32k cap when either is known.
pub fn effective_context_window(advertised: Option<u64>, runtime: Option<u64>) -> u64 {
    let advertised = advertised.filter(|n| *n > 0);
    let runtime = runtime.filter(|n| *n > 0);
    match (advertised, runtime) {
        (Some(a), Some(r)) => a.min(r),
        (Some(a), None) => a,
        (None, Some(r)) => r,
        (None, None) => DEFAULT_CONTEXT_CAP,
    }
}

fn context_window_for(model: &DiscoveredOllamaModel, models: &ModelsConfig) -> NonZeroU64 {
    let tokens = effective_context_window(model.context_length, runtime_context_window(models));
    NonZeroU64::new(tokens.max(1)).unwrap_or(NonZeroU64::MIN)
}

#[derive(serde::Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(serde::Deserialize)]
struct TagModel {
    #[serde(default)]
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    details: Option<TagDetails>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(serde::Deserialize, Default)]
struct TagDetails {
    #[serde(default)]
    parameter_size: Option<String>,
    #[serde(default)]
    context_length: Option<u64>,
}

pub fn probe(host: &str) -> Vec<DiscoveredOllamaModel> {
    #[cfg(test)]
    if let Some(override_models) = test_probe_override() {
        return override_models;
    }
    probe_http(host)
}

fn probe_http(host: &str) -> Vec<DiscoveredOllamaModel> {
    let url = format!("{}/api/tags", native_host(host));
    let client = match reqwest::blocking::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "ollama: failed to build probe client");
            return Vec::new();
        }
    };
    let response = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, url = %url, "ollama: daemon not reachable");
            return Vec::new();
        }
    };
    if !response.status().is_success() {
        tracing::debug!(status = %response.status(), url = %url, "ollama: tags probe failed");
        return Vec::new();
    }
    let parsed: TagsResponse = match response.json() {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(error = %e, "ollama: tags JSON parse failed");
            return Vec::new();
        }
    };
    parsed
        .models
        .into_iter()
        .filter(|m| !m.name.trim().is_empty())
        .map(|m| {
            let details = m.details.unwrap_or_default();
            DiscoveredOllamaModel {
                name: m.name,
                size_bytes: m.size,
                context_length: details.context_length,
                capabilities: m.capabilities,
                parameter_size: details.parameter_size,
            }
        })
        .filter(|m| m.is_chat_model())
        .collect()
}

pub fn pick_fallback(
    discovered: &[DiscoveredOllamaModel],
    models: &ModelsConfig,
    catalog: &IndexMap<String, ModelEntry>,
) -> Option<String> {
    if !fallback_enabled(models) {
        return None;
    }
    let chat: Vec<&DiscoveredOllamaModel> =
        discovered.iter().filter(|m| m.is_chat_model()).collect();
    if chat.is_empty() {
        return None;
    }
    match fallback_pin(models) {
        FallbackPin::Off => None,
        FallbackPin::Named(pin) => {
            resolve_pin(&pin, &chat, catalog).or_else(|| auto_pick(&chat).map(|m| m.catalog_key()))
        }
        FallbackPin::Auto => auto_pick(&chat).map(|m| m.catalog_key()),
    }
}

fn resolve_pin<'a>(
    pin: &str,
    chat: &[&'a DiscoveredOllamaModel],
    catalog: &IndexMap<String, ModelEntry>,
) -> Option<String> {
    if catalog.contains_key(pin) {
        return Some(pin.to_owned());
    }
    let prefixed = catalog_key(pin);
    if catalog.contains_key(&prefixed)
        || chat
            .iter()
            .any(|m| m.name == pin || m.catalog_key() == prefixed)
    {
        return Some(prefixed);
    }
    chat.iter()
        .find(|m| m.name == pin || m.name.ends_with(&format!(":{pin}")) || m.name.ends_with(pin))
        .map(|m| m.catalog_key())
}

fn auto_pick<'a>(chat: &[&'a DiscoveredOllamaModel]) -> Option<&'a DiscoveredOllamaModel> {
    chat.iter().copied().max_by(|a, b| {
        a.supports_tools()
            .cmp(&b.supports_tools())
            .then(a.size_bytes.cmp(&b.size_bytes))
            .then(
                a.context_length
                    .unwrap_or(0)
                    .cmp(&b.context_length.unwrap_or(0)),
            )
            .then(a.name.cmp(&b.name))
    })
}

/// Insert discovered models that are not already in the catalog. User `[model.*]`
/// entries win. Stamps `is_usage_fallback` on the chosen fallback.
pub fn merge_into_catalog(
    catalog: &mut IndexMap<String, ModelEntry>,
    discovered: &[DiscoveredOllamaModel],
    cfg: &Config,
) -> usize {
    let host = resolve_host(&cfg.models);
    let base_url = openai_base(&host);
    let mut added = 0;
    for model in discovered.iter().filter(|m| m.is_chat_model()) {
        let key = model.catalog_key();
        if let Some(entry) = catalog.get_mut(&key) {
            if entry.info.is_local_ollama {
                entry.info.context_window = context_window_for(model, &cfg.models);
            }
            continue;
        }
        if catalog_has_same_ollama_tag(catalog, &model.name, &base_url) {
            continue;
        }
        catalog.insert(key, discovered_entry(model, &base_url, &cfg.models));
        added += 1;
    }
    stamp_fallback(catalog, discovered, &cfg.models);
    if added > 0 {
        tracing::info!(count = added, "ollama: merged local models into catalog");
    }
    added
}

fn catalog_has_same_ollama_tag(
    catalog: &IndexMap<String, ModelEntry>,
    tag: &str,
    openai_base: &str,
) -> bool {
    catalog.values().any(|entry| {
        entry.info.model == tag
            && (is_ollama_catalog_key(entry.info.id.as_deref().unwrap_or(""))
                || entry.info.base_url.trim_end_matches('/') == openai_base.trim_end_matches('/')
                || entry.info.base_url.contains(":11434"))
    })
}

fn stamp_fallback(
    catalog: &mut IndexMap<String, ModelEntry>,
    discovered: &[DiscoveredOllamaModel],
    models: &ModelsConfig,
) {
    for entry in catalog.values_mut() {
        entry.info.is_usage_fallback = false;
    }
    let Some(key) = pick_fallback(discovered, models, catalog) else {
        return;
    };
    if let Some(entry) = catalog.get_mut(&key) {
        entry.info.is_usage_fallback = true;
        entry.info.is_local_ollama = true;
    }
}

fn discovered_entry(
    model: &DiscoveredOllamaModel,
    base_url: &str,
    models: &ModelsConfig,
) -> ModelEntry {
    let ctx = context_window_for(model, models);
    let mut description = format!("Local Ollama model {}", model.name);
    if let Some(params) = &model.parameter_size {
        description.push_str(&format!(" ({params})"));
    }
    if model.supports_tools() {
        description.push_str(" · tools");
    }
    let mut info = ModelInfo::fallback(&model.name);
    info.id = Some(model.catalog_key());
    info.model = model.name.clone();
    info.name = Some(format!("{} (Ollama)", model.name));
    info.description = Some(description);
    info.base_url = base_url.to_owned();
    info.api_backend = ApiBackend::ChatCompletions;
    info.auth_scheme = AuthScheme::Bearer;
    info.context_window = ctx;
    info.agent_type = DEFAULT_AGENT_TYPE.to_owned();
    info.stream_tool_calls = Some(false);
    info.supports_backend_search = false;
    info.supported_in_api = true;
    info.hidden = false;
    info.user_selectable = true;
    info.is_local_ollama = true;
    info.is_usage_fallback = false;
    ModelEntry {
        info,
        mtls_cert_dir: None,
        api_key: Some(DUMMY_API_KEY.to_owned()),
        env_key: None,
        auth_provider: None,
        api_base_url: None,
    }
}

#[cfg(test)]
std::thread_local! {
    static PROBE_OVERRIDE: std::cell::RefCell<Option<Vec<DiscoveredOllamaModel>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn test_probe_override() -> Option<Vec<DiscoveredOllamaModel>> {
    PROBE_OVERRIDE.with(|slot| slot.borrow().clone())
}

#[cfg(test)]
pub(crate) fn set_probe_override(models: Option<Vec<DiscoveredOllamaModel>>) {
    PROBE_OVERRIDE.with(|slot| *slot.borrow_mut() = models);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::config::Config;

    fn chat(name: &str, size: u64, tools: bool) -> DiscoveredOllamaModel {
        DiscoveredOllamaModel {
            name: name.to_owned(),
            size_bytes: size,
            context_length: Some(32_768),
            capabilities: if tools {
                vec!["completion".into(), "tools".into()]
            } else {
                vec!["completion".into()]
            },
            parameter_size: Some("7B".into()),
        }
    }

    #[test]
    fn catalog_key_prefixes_tag() {
        assert_eq!(catalog_key("qwen3.8:27b"), "ollama/qwen3.8:27b");
        assert!(is_ollama_catalog_key("ollama/qwen3.8:27b"));
        assert!(!is_ollama_catalog_key("grok-4.6"));
    }

    #[test]
    fn embed_models_are_skipped() {
        let embed = DiscoveredOllamaModel {
            name: "nomic-embed-text".into(),
            size_bytes: 1,
            context_length: None,
            capabilities: vec![],
            parameter_size: None,
        };
        assert!(!embed.is_chat_model());
        let tagged = DiscoveredOllamaModel {
            name: "mxbai-embed-large".into(),
            size_bytes: 1,
            context_length: None,
            capabilities: vec!["embedding".into()],
            parameter_size: None,
        };
        assert!(!tagged.is_chat_model());
    }

    #[test]
    fn auto_pick_prefers_tools_then_size() {
        let models = [
            chat("small", 1_000, true),
            chat("big-no-tools", 9_000, false),
            chat("big-tools", 8_000, true),
        ];
        let refs: Vec<&DiscoveredOllamaModel> = models.iter().collect();
        assert_eq!(auto_pick(&refs).map(|m| m.name.as_str()), Some("big-tools"));
    }

    #[test]
    fn merge_inserts_and_stamps_fallback() {
        let mut catalog = IndexMap::new();
        let cfg = Config::default();
        let discovered = vec![chat("qwen3.8:27b", 17_000, true), chat("tiny", 100, false)];
        let added = merge_into_catalog(&mut catalog, &discovered, &cfg);
        assert_eq!(added, 2);
        let qwen = catalog.get("ollama/qwen3.8:27b").expect("qwen entry");
        assert_eq!(qwen.info.model, "qwen3.8:27b");
        assert_eq!(qwen.info.base_url, "http://127.0.0.1:11434/v1");
        assert_eq!(qwen.api_key.as_deref(), Some("ollama"));
        assert!(qwen.info.is_usage_fallback);
        assert!(
            !catalog
                .get("ollama/tiny")
                .expect("tiny")
                .info
                .is_usage_fallback
        );
    }

    #[test]
    fn merge_does_not_clobber_user_entry() {
        let mut catalog = IndexMap::new();
        let cfg = Config::default();
        catalog.insert(
            "ollama/qwen3.8:27b".into(),
            discovered_entry(
                &chat("qwen3.8:27b", 1, true),
                "http://custom:11434/v1",
                &cfg.models,
            ),
        );
        if let Some(entry) = catalog.get_mut("ollama/qwen3.8:27b") {
            entry.info.name = Some("Mine".into());
        }
        let added = merge_into_catalog(&mut catalog, &[chat("qwen3.8:27b", 99, true)], &cfg);
        assert_eq!(added, 0);
        assert_eq!(
            catalog
                .get("ollama/qwen3.8:27b")
                .expect("kept")
                .info
                .name
                .as_deref(),
            Some("Mine")
        );
    }

    #[test]
    fn context_window_uses_runtime_capped_by_model() {
        assert_eq!(
            effective_context_window(Some(262_144), Some(131_072)),
            131_072
        );
        assert_eq!(effective_context_window(Some(8_192), Some(131_072)), 8_192);
        assert_eq!(effective_context_window(Some(262_144), None), 262_144);
        assert_eq!(effective_context_window(None, Some(131_072)), 131_072);
        assert_eq!(effective_context_window(None, None), DEFAULT_CONTEXT_CAP);
    }

    #[test]
    fn merge_restamps_context_on_discovered_entries() {
        let mut catalog = IndexMap::new();
        let mut cfg = Config::default();
        cfg.models.ollama_context_window = Some(131_072);
        let model = DiscoveredOllamaModel {
            name: "qwen3.8:27b".into(),
            size_bytes: 17_000,
            context_length: Some(262_144),
            capabilities: vec!["completion".into(), "tools".into()],
            parameter_size: Some("27.3B".into()),
        };
        assert_eq!(merge_into_catalog(&mut catalog, &[model.clone()], &cfg), 1);
        assert_eq!(
            catalog
                .get("ollama/qwen3.8:27b")
                .expect("inserted")
                .info
                .context_window
                .get(),
            131_072
        );
        cfg.models.ollama_context_window = Some(65_536);
        assert_eq!(merge_into_catalog(&mut catalog, &[model], &cfg), 0);
        assert_eq!(
            catalog
                .get("ollama/qwen3.8:27b")
                .expect("updated")
                .info
                .context_window
                .get(),
            65_536
        );
    }

    #[test]
    fn host_normalization() {
        assert_eq!(normalize_host("127.0.0.1:11434"), "http://127.0.0.1:11434");
        assert_eq!(
            openai_base("http://127.0.0.1:11434/v1/"),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            native_host("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434"
        );
    }

    #[test]
    fn parse_tags_json() {
        let raw = r#"{
            "models": [{
                "name": "qwen3.8:27b",
                "size": 17741872154,
                "details": {"parameter_size": "27.3B", "context_length": 262144},
                "capabilities": ["completion", "vision", "tools", "thinking"]
            }]
        }"#;
        let parsed: TagsResponse = serde_json::from_str(raw).unwrap();
        let first = parsed.models.first().expect("one model");
        assert_eq!(first.name, "qwen3.8:27b");
        assert_eq!(
            first.details.as_ref().and_then(|d| d.context_length),
            Some(262144)
        );
    }
}
