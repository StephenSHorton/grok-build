//! Optional Jev / TypeSafe judge settings.
//!
//! No key ⇒ `jev_enabled()` is false. Env wins over `[jev].api_key`. Empty strings do not count.
//! `GROK_CONFIG` overlays cannot inject a key: `[jev]` is not on `OVERLAY_ALLOW_PATHS`.

use serde::{Deserialize, Serialize};

/// Env var for a hosted or official Jev/TypeSafe key. Highest precedence.
pub const JEV_API_KEY_ENV: &str = "JEV_API_KEY";

/// Fallback env var (official TypeSafe keys). Used only when [`JEV_API_KEY_ENV`] is unset or blank.
pub const TYPESAFE_API_KEY_ENV: &str = "TYPESAFE_API_KEY";

fn default_nudge_every() -> i32 {
    2
}

fn default_nudge() -> bool {
    true
}

fn default_route() -> bool {
    true
}

/// `[jev]` in config.toml. Nudge and route default on; `safety_check` and `context_filter` stay off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JevConfig {
    /// File-layer key. Env `JEV_API_KEY` / `TYPESAFE_API_KEY` override this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Optional Decide URL override (Rock: `EndpointFor` then `Client.BaseURL`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Optional model override. The client default is `jev-latest`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional self-validation nudge after a successful mutate. Default on when a key is present.
    #[serde(default = "default_nudge")]
    pub nudge: bool,
    /// Emit the nudge every N successful mutates. `-1` disables. Default 2.
    #[serde(default = "default_nudge_every")]
    pub nudge_every: i32,
    /// Optional fail-open destructive-call check. Default off.
    #[serde(default)]
    pub safety_check: bool,
    /// Optional fail-open context filter. Default off.
    #[serde(default)]
    pub context_filter: bool,
    /// Optional fail-open trivial-request router before the first sample. Default on with a key.
    #[serde(default = "default_route")]
    pub route: bool,
    /// Optional fail-open file pick after grep / list_dir. Default off.
    #[serde(default)]
    pub file_pick: bool,
    /// Optional fail-open done check after a successful mutate. Default off.
    #[serde(default)]
    pub done_check: bool,
    /// Noul threshold for an optional safety deny. `None` means the later-slice default (0.72).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_block: Option<f64>,
    /// When true, a live safety noul at/above `risk_block` does not deny. Default false.
    #[serde(default)]
    pub allow_destructive: bool,
}

impl Default for JevConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            model: None,
            nudge: true,
            nudge_every: default_nudge_every(),
            safety_check: false,
            context_filter: false,
            route: true,
            file_pick: false,
            done_check: false,
            risk_block: None,
            allow_destructive: false,
        }
    }
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|s| !s.is_empty())
}

fn env_nonempty(name: &str) -> Option<String> {
    nonempty(std::env::var(name).ok().as_deref()).map(str::to_owned)
}

/// Resolve a Jev key from already-read tiers. First non-blank wins:
/// `JEV_API_KEY` → `TYPESAFE_API_KEY` → `[jev].api_key`.
pub fn resolve_jev_key(
    jev_api_key: Option<&str>,
    typesafe_api_key: Option<&str>,
    file_api_key: Option<&str>,
) -> Option<String> {
    nonempty(jev_api_key)
        .or_else(|| nonempty(typesafe_api_key))
        .or_else(|| nonempty(file_api_key))
        .map(str::to_owned)
}

/// Process env (`JEV_API_KEY`, then `TYPESAFE_API_KEY`) then the file-layer key.
pub fn jev_key(file_api_key: Option<&str>) -> Option<String> {
    resolve_jev_key(
        env_nonempty(JEV_API_KEY_ENV).as_deref(),
        env_nonempty(TYPESAFE_API_KEY_ENV).as_deref(),
        file_api_key,
    )
}

/// True when [`jev_key`] resolves a non-empty key.
pub fn jev_enabled(file_api_key: Option<&str>) -> bool {
    jev_key(file_api_key).is_some()
}

impl JevConfig {
    /// File-layer key after trimming. Empty / whitespace is unset.
    pub fn file_api_key(&self) -> Option<&str> {
        nonempty(self.api_key.as_deref())
    }

    /// Env, then this table's `api_key`.
    pub fn resolved_key(&self) -> Option<String> {
        jev_key(self.api_key.as_deref())
    }

    /// Whether a usable key is present (env or file).
    pub fn enabled(&self) -> bool {
        jev_enabled(self.api_key.as_deref())
    }
}

#[cfg(test)]
#[path = "jev_tests.rs"]
mod tests;
