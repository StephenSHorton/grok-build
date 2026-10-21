//! Compile-time selection of the authority this build talks to.
//!
//! One implementation compiles, so the trait is a checklist: a backend that forgets a decision fails to build.
use crate::flow::StderrCallback;
use crate::refresh::{DiagnosticUploader, TokenRefresher};
use crate::{AuthManager, AuthUrlInfo, GrokAuth, GrokComConfig, LoginTransportOverride};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
mod grok;
/// The inputs of one login attempt.
pub struct LoginRequest<'a> {
    pub auth_manager: &'a Arc<AuthManager>,
    pub grok_com_config: &'a GrokComConfig,
    /// `[grok_com_config] login_device_flow` config tier, resolved by the caller (no longer a `GrokComConfig` field).
    pub config_device_flow: Option<bool>,
    pub reauth: bool,
    pub force_interactive: bool,
    pub on_stderr: Option<StderrCallback>,
    pub url_tx: Option<Rc<RefCell<Option<oneshot::Sender<AuthUrlInfo>>>>>,
    pub code_rx: Option<mpsc::Receiver<String>>,
    pub login_override: LoginTransportOverride,
}
/// `?Send`: `url_tx` is an `Rc`, so a login future can never cross threads.
#[async_trait::async_trait(?Send)]
pub trait AuthBackend {
    /// Key under which this backend owns its entry in auth.json.
    fn scope_key(&self, config: &GrokComConfig) -> String;
    /// Older scope keys this backend minted, and so may adopt from and tidy, most recent first.
    fn inherited_scopes(&self) -> &'static [&'static str];
    /// Whether this backend minted the credential, which the scope key alone cannot establish.
    fn owns(&self, auth: &GrokAuth) -> bool;
    /// Whether this backend's session token may be sent to `url`.
    /// A model entry carries its own base URL, so without this a poisoned or hand-edited entry aims the bearer anywhere.
    fn may_receive_session(&self, url: &str) -> bool;
    /// The host to name when telling the user whose session they hold.
    fn login_host(&self, config: &GrokComConfig) -> String;
    /// Whether xAI issued this backend's credentials and may therefore receive them.
    /// Gates every request that carries the bearer to an xAI host, and every xAI-only policy.
    fn is_xai_authority(&self) -> bool;
    /// Obtain a credential; the flag reports whether a login actually ran.
    async fn login(&self, req: LoginRequest<'_>) -> anyhow::Result<(GrokAuth, bool)>;
    /// The renewal authority for the credentials this backend mints.
    fn refresher(
        &self,
        manager: Arc<AuthManager>,
        auth_provider_command: Option<String>,
        diagnostic_uploader: Option<DiagnosticUploader>,
    ) -> Arc<dyn TokenRefresher>;
}
pub type ActiveAuthBackend = grok::GrokAuthBackend;
/// Reports a URL the way a user says it, without the scheme.
pub fn host_of(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .to_owned()
}
/// Hosts of model vendors other than xAI; an xAI credential (session token or `XAI_API_KEY`) is never sent to them.
/// A denylist, not an allowlist: customer gateways must keep receiving the session (see `GrokAuthBackend::may_receive_session`).
const THIRD_PARTY_AI_DOMAINS: &[&str] = &[
    "anthropic.com",
    "claude.ai",
    "openai.com",
    "openrouter.ai",
    "mistral.ai",
    "groq.com",
    "together.xyz",
    "together.ai",
    "fireworks.ai",
    "deepseek.com",
    "cohere.com",
    "cohere.ai",
    "perplexity.ai",
    "generativelanguage.googleapis.com",
];
/// Whether `url` points at another model vendor's API (the host or any subdomain of a [`THIRD_PARTY_AI_DOMAINS`] entry).
pub fn is_third_party_ai_host(url: &str) -> bool {
    let Some(host) = url::Url::parse(url).ok().and_then(|u| {
        u.host_str()
            .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
    }) else {
        return false;
    };
    THIRD_PARTY_AI_DOMAINS.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn third_party_ai_hosts_match_domain_and_subdomains_only() {
        assert!(is_third_party_ai_host("https://api.anthropic.com/v1"));
        assert!(is_third_party_ai_host("https://API.Anthropic.com./v1"));
        assert!(is_third_party_ai_host("https://api.openai.com/v1"));
        assert!(is_third_party_ai_host(
            "https://generativelanguage.googleapis.com/v1beta/openai"
        ));
        assert!(!is_third_party_ai_host("https://notanthropic.com/v1"));
        assert!(!is_third_party_ai_host(
            "https://anthropic.com.evil.test/v1"
        ));
        assert!(!is_third_party_ai_host("https://api.x.ai/v1"));
        assert!(!is_third_party_ai_host(
            "https://cli-chat-proxy.grok.com/v1"
        ));
        assert!(!is_third_party_ai_host("https://gateway.corp.example/v1"));
        assert!(!is_third_party_ai_host("https://storage.googleapis.com/v1"));
        assert!(!is_third_party_ai_host("not a url"));
    }
    #[test]
    fn host_of_drops_the_scheme_and_leaves_the_rest_alone() {
        assert_eq!(host_of("https://example.test"), "example.test");
        assert_eq!(host_of("http://localhost:8080"), "localhost:8080");
        assert_eq!(host_of("grok.com"), "grok.com");
    }
}
