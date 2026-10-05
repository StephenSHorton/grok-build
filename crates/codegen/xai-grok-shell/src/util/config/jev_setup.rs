//! `/jev-setup` helpers: status, masked keys, comment-preserving `[jev]` edits.
//!
//! Never log or return the full API key. Env `JEV_API_KEY` / `TYPESAFE_API_KEY`
//! win over `[jev].api_key`.

use std::path::Path;

pub use xai_grok_config_types::JevConfig;
use xai_grok_config_types::{JEV_API_KEY_ENV, TYPESAFE_API_KEY_ENV, jev_key, resolve_jev_key};

/// Where the live key came from. Env always beats the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JevKeySource {
    EnvJev,
    EnvTypesafe,
    File,
    None,
}

impl JevKeySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::EnvJev => "env JEV_API_KEY",
            Self::EnvTypesafe => "env TYPESAFE_API_KEY",
            Self::File => "file [jev].api_key",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JevFlag {
    Nudge,
    SafetyCheck,
    ContextFilter,
}

impl JevFlag {
    pub fn name(self) -> &'static str {
        match self {
            Self::Nudge => "nudge",
            Self::SafetyCheck => "safety_check",
            Self::ContextFilter => "context_filter",
        }
    }

    pub fn explain(self) -> &'static str {
        match self {
            Self::Nudge => {
                "default on with a key: after successful edits/shells, a reminder may suggest ask_jev (no extra Jev call)"
            }
            Self::SafetyCheck => {
                "a deny after bash/edit/write/MCP/apply_patch means Jev judged the call destructive"
            }
            Self::ContextFilter => "older tool results may be omitted from the next request",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        match name {
            "nudge" => Some(Self::Nudge),
            "safety" | "safety_check" => Some(Self::SafetyCheck),
            "filter" | "context_filter" => Some(Self::ContextFilter),
            _ => None,
        }
    }
}

/// Snapshot for `/jev-setup` status. Keys are always masked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevSetupStatus {
    pub enabled: bool,
    pub source: JevKeySource,
    pub masked_live_key: Option<String>,
    pub file_key_present: bool,
    pub masked_file_key: Option<String>,
    pub env_wins: bool,
    pub nudge: bool,
    pub safety_check: bool,
    pub context_filter: bool,
}

/// Owned key that `Debug`s as a mask so slash-action dumps cannot leak it.
#[derive(Clone, PartialEq, Eq)]
pub struct JevKeyArg(pub String);

impl std::fmt::Debug for JevKeyArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&mask_key(&self.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JevSetupRequest {
    Status,
    /// `key` is `None` when the TUI should prompt with masked input.
    Set {
        key: Option<JevKeyArg>,
        force: bool,
    },
    Off,
    Flag {
        flag: JevFlag,
        on: bool,
    },
    /// Re-apply the current file/env settings to this session.
    Apply,
    /// Session + all-time Decide metrics (`/jev-stats` is the dedicated command).
    Stats,
    Help,
}

/// Last-4-visible mask. Never returns the full key.
pub fn mask_key(key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return "(empty)".to_string();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 4 {
        return "••••".to_string();
    }
    let visible = chars.len() - 4;
    let last: String = chars.into_iter().skip(visible).collect();
    format!("{}{last}", "•".repeat(visible.min(8)))
}

/// Strip `key` from an error so HTTP bodies cannot echo a secret.
pub fn scrub_key(message: &str, key: &str) -> String {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return message.to_string();
    }
    message.replace(trimmed, &mask_key(trimmed))
}

fn nonempty_owned(value: Option<String>) -> Option<String> {
    value.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

/// Build status from already-read env + `[jev]` without re-reading process env
/// unless the caller passes `None` sentinels they loaded themselves.
pub fn collect_status(
    env_jev: Option<String>,
    env_typesafe: Option<String>,
    file: &JevConfig,
) -> JevSetupStatus {
    let env_jev = nonempty_owned(env_jev);
    let env_typesafe = nonempty_owned(env_typesafe);
    let file_key = file.file_api_key().map(str::to_owned);
    let source = if env_jev.is_some() {
        JevKeySource::EnvJev
    } else if env_typesafe.is_some() {
        JevKeySource::EnvTypesafe
    } else if file_key.is_some() {
        JevKeySource::File
    } else {
        JevKeySource::None
    };
    let live = resolve_jev_key(
        env_jev.as_deref(),
        env_typesafe.as_deref(),
        file_key.as_deref(),
    );
    JevSetupStatus {
        enabled: live.is_some(),
        source,
        masked_live_key: live.as_deref().map(mask_key),
        file_key_present: file_key.is_some(),
        masked_file_key: file_key.as_deref().map(mask_key),
        env_wins: matches!(source, JevKeySource::EnvJev | JevKeySource::EnvTypesafe),
        nudge: file.nudge,
        safety_check: file.safety_check,
        context_filter: file.context_filter,
    }
}

/// Status using the current process env and a file-layer `[jev]` table.
pub fn collect_status_from_file(file: &JevConfig) -> JevSetupStatus {
    collect_status(
        std::env::var(JEV_API_KEY_ENV).ok(),
        std::env::var(TYPESAFE_API_KEY_ENV).ok(),
        file,
    )
}

pub fn format_status(status: &JevSetupStatus) -> String {
    let mut lines = vec![format!(
        "Jev: {} (key from {})",
        if status.enabled { "on" } else { "off" },
        status.source.label()
    )];
    if let Some(masked) = &status.masked_live_key {
        lines.push(format!("Live key: {masked}"));
    }
    if status.file_key_present {
        if let Some(masked) = &status.masked_file_key {
            lines.push(format!("File key: {masked}"));
        }
    } else {
        lines.push("File key: (none)".to_string());
    }
    if status.env_wins {
        lines.push(
            "JEV_API_KEY / TYPESAFE_API_KEY is set and wins over [jev].api_key. Unset the env var to use the file key or to turn Jev off."
                .to_string(),
        );
    }
    lines.push(format!(
        "nudge: {} — {}",
        on_off(status.nudge),
        JevFlag::Nudge.explain()
    ));
    lines.push(format!(
        "safety_check: {} — {}",
        on_off(status.safety_check),
        JevFlag::SafetyCheck.explain()
    ));
    lines.push(format!(
        "context_filter: {} — {}",
        on_off(status.context_filter),
        JevFlag::ContextFilter.explain()
    ));
    lines.push(
        "When this session is idle, /jev-setup apply (or a successful set/off/flag) updates ask_jev and the <jev> prompt section without compacting history. If a turn is running, apply again when idle or start a new session."
            .to_string(),
    );
    lines.join("\n")
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub fn format_help() -> String {
    "\
/jev-setup              show status (key is always masked)
/jev-setup set          enter a key (TUI: masked prompt; ACP: /jev-setup set <key>)
/jev-setup set --force  save even if the live Decide ping fails
/jev-setup off          remove [jev].api_key (env still wins if set)
/jev-setup nudge on|off   default on with a key
/jev-setup safety on|off  default off
/jev-setup filter on|off  default off
/jev-setup apply        try to turn Jev on in this session
/jev-setup stats        Decide latency / tokens / outcomes (same as /jev-stats)"
        .to_string()
}

pub fn parse_args(args: &str) -> Result<JevSetupRequest, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed == "status" {
        return Ok(JevSetupRequest::Status);
    }
    if matches!(trimmed, "help" | "-h" | "--help") {
        return Ok(JevSetupRequest::Help);
    }
    if matches!(trimmed, "off" | "clear" | "disable" | "remove") {
        return Ok(JevSetupRequest::Off);
    }
    if trimmed == "apply" {
        return Ok(JevSetupRequest::Apply);
    }
    if matches!(trimmed, "stats" | "stat" | "metrics") {
        return Ok(JevSetupRequest::Stats);
    }
    let mut parts = trimmed.split_whitespace();
    let Some(head) = parts.next() else {
        return Ok(JevSetupRequest::Status);
    };
    if head == "set" {
        let rest: Vec<&str> = parts.collect();
        let force = rest.iter().any(|p| *p == "--force");
        let key = rest
            .into_iter()
            .filter(|p| *p != "--force")
            .collect::<Vec<_>>()
            .join(" ");
        let key = if key.is_empty() {
            None
        } else {
            Some(JevKeyArg(key))
        };
        return Ok(JevSetupRequest::Set { key, force });
    }
    if let Some(flag) = JevFlag::parse(head) {
        let on = match parts.next().map(|s| s.to_ascii_lowercase()) {
            Some(v) if matches!(v.as_str(), "on" | "true" | "1" | "yes") => true,
            Some(v) if matches!(v.as_str(), "off" | "false" | "0" | "no") => false,
            Some(_) => {
                return Err(format!("Usage: /jev-setup {} on|off", flag.name()));
            }
            None => {
                return Err(format!("Usage: /jev-setup {} on|off", flag.name()));
            }
        };
        if parts.next().is_some() {
            return Err(format!("Usage: /jev-setup {} on|off", flag.name()));
        }
        return Ok(JevSetupRequest::Flag { flag, on });
    }
    Err(format!(
        "Unknown /jev-setup argument. {}",
        format_help()
            .lines()
            .next()
            .unwrap_or("See /jev-setup help")
    ))
}

/// Tiny live Decide. Does not fabricate an answer; a transport/HTTP error is a failed ping.
pub async fn validate_key(api_key: &str) -> Result<(), String> {
    validate_key_recorded(api_key).await.0
}

/// [`validate_key`] plus a Decide record when HTTP was attempted.
pub async fn validate_key_recorded(
    api_key: &str,
) -> (Result<(), String>, Option<xai_grok_jev::DecideRecord>) {
    let key = api_key.trim();
    if key.is_empty() {
        return (Err("Key is empty.".to_string()), None);
    }
    let client = match xai_grok_jev::Client::new(key) {
        Ok(client) => client,
        Err(err) => return (Err(scrub_key(&err.to_string(), key)), None),
    };
    let mut questions = std::collections::BTreeMap::new();
    questions.insert(
        "ping".to_string(),
        xai_grok_jev::noul_q("Is 1 less than 2?"),
    );
    let (result, record) = client
        .decide_timed(
            &serde_json::json!({"setup":"ping"}),
            &questions,
            xai_grok_jev::DecideSource::Setup,
        )
        .await;
    let outcome = match result {
        Ok(_) => Ok(()),
        Err(err) => Err(format!(
            "Decide ping failed: {}",
            scrub_key(&err.to_string(), key)
        )),
    };
    (outcome, Some(record))
}

/// `{GROK_HOME}/jev_stats.jsonl`. The file is created only when a record is appended.
pub fn jev_stats_log_path() -> std::path::PathBuf {
    xai_dirs::resolve_grok_home()
        .unwrap_or_else(crate::util::grok_home::grok_home)
        .join(xai_grok_jev::STATS_LOG_NAME)
}

/// Append one Decide record to the rolling local log. No-op of contents; sizes only.
pub fn persist_jev_decide_record(record: &xai_grok_jev::DecideRecord) {
    let _ = xai_grok_jev::append_decide_log(&jev_stats_log_path(), record);
}

fn jev_table<'a>(doc: &'a mut toml_edit::DocumentMut) -> &'a mut toml_edit::Table {
    let item = doc
        .entry("jev")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    if !item.is_table() {
        *item = toml_edit::Item::Table(toml_edit::Table::new());
    }
    item.as_table_mut()
        .expect("jev entry is a table after the guard above")
}

/// Replace a `[jev]` value without dropping key prefix comments.
fn assign_jev_value(table: &mut toml_edit::Table, key: &str, value: impl Into<toml_edit::Value>) {
    let value = value.into();
    if let Some(existing) = table.get_mut(key).and_then(|item| item.as_value_mut()) {
        *existing = value;
        return;
    }
    table.insert(key, toml_edit::Item::Value(value));
}

/// Comment-preserving write of `[jev].api_key`. Creates `[jev]` when missing.
pub fn save_api_key_at(path: &Path, api_key: &str) -> Result<(), String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Refusing to save an empty key.".to_string());
    }
    edit_jev_document(path, |doc| {
        assign_jev_value(jev_table(doc), "api_key", key);
        Ok(())
    })
}

pub fn clear_api_key_at(path: &Path) -> Result<(), String> {
    edit_jev_document(path, |doc| {
        if let Some(table) = doc.get_mut("jev").and_then(|item| item.as_table_mut()) {
            table.remove("api_key");
            if table.is_empty() {
                doc.remove("jev");
            }
        }
        Ok(())
    })
}

pub fn set_flag_at(path: &Path, flag: JevFlag, on: bool) -> Result<(), String> {
    edit_jev_document(path, |doc| {
        assign_jev_value(jev_table(doc), flag.name(), on);
        Ok(())
    })
}

fn edit_jev_document(
    path: &Path,
    edit: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("create config dir: {err}"))?;
    }
    let original = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(format!("read {}: {err}", path.display())),
    };
    let mut doc: toml_edit::DocumentMut = if original.trim().is_empty() {
        toml_edit::DocumentMut::new()
    } else {
        original
            .parse()
            .map_err(|err| format!("refusing to rewrite unparseable {}: {err}", path.display()))?
    };
    edit(&mut doc)?;
    let updated = doc.to_string();
    crate::util::config::persist::atomic_replace_string(path, &updated)
        .map_err(|err| format!("write {}: {err}", path.display()))
}

/// Locked write of the user `config.toml` (`~/.grok` or `$GROK_HOME`).
pub async fn save_user_api_key(api_key: &str) -> Result<(), String> {
    let key = api_key.to_string();
    with_user_config(move |path| save_api_key_at(&path, &key)).await
}

pub async fn clear_user_api_key() -> Result<(), String> {
    with_user_config(|path| clear_api_key_at(&path)).await
}

pub async fn set_user_flag(flag: JevFlag, on: bool) -> Result<(), String> {
    with_user_config(move |path| set_flag_at(&path, flag, on)).await
}

async fn with_user_config(
    rewrite: impl FnOnce(std::path::PathBuf) -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    let guard = crate::util::config::persist::lock_config_writes()
        .await
        .map_err(|err| format!("lock config.toml: {err}"))?;
    guard
        .run_blocking(move || rewrite(crate::util::config::mcp::user_config_path()))
        .await
        .map_err(|err| format!("config write task: {err}"))?
}

/// True when a process env key is the live one (file edits cannot turn Jev off).
pub fn env_key_is_live() -> bool {
    jev_key(None).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mask_key_hides_all_but_last_four() {
        assert_eq!(mask_key("jv_live_abcdefgh"), "••••••••efgh");
        assert_eq!(mask_key("abcd"), "••••");
        assert_eq!(mask_key("   "), "(empty)");
        assert!(!mask_key("jv_live_secret_key_value").contains("secret"));
    }

    #[test]
    fn scrub_key_replaces_full_secret() {
        let key = "jv_live_supersecret";
        let msg = format!("http 401: bad token {key} trailing");
        let scrubbed = scrub_key(&msg, key);
        assert!(!scrubbed.contains("supersecret"));
        assert!(scrubbed.contains(&mask_key(key)));
    }

    #[test]
    fn collect_status_env_wins_over_file() {
        let file = JevConfig {
            api_key: Some("filekey_xxxx".into()),
            nudge: true,
            ..JevConfig::default()
        };
        let status = collect_status(Some("envkey_yyyy".into()), None, &file);
        assert!(status.enabled);
        assert_eq!(status.source, JevKeySource::EnvJev);
        assert!(status.env_wins);
        let masked = mask_key("envkey_yyyy");
        assert_eq!(status.masked_live_key.as_deref(), Some(masked.as_str()));
        assert!(status.nudge);
        let text = format_status(&status);
        assert!(text.contains("env JEV_API_KEY"));
        assert!(!text.contains("envkey_yyyy"));
        assert!(!text.contains("filekey_xxxx"));
    }

    #[test]
    fn collect_status_file_when_env_blank() {
        let file = JevConfig {
            api_key: Some("filekey_zzzz".into()),
            ..JevConfig::default()
        };
        let status = collect_status(Some("   ".into()), None, &file);
        assert_eq!(status.source, JevKeySource::File);
        assert!(!status.env_wins);
    }

    #[test]
    fn parse_args_covers_verbs() {
        assert_eq!(parse_args("").unwrap(), JevSetupRequest::Status);
        assert_eq!(parse_args("stats").unwrap(), JevSetupRequest::Stats);
        assert_eq!(parse_args("off").unwrap(), JevSetupRequest::Off);
        assert_eq!(
            parse_args("set").unwrap(),
            JevSetupRequest::Set {
                key: None,
                force: false
            }
        );
        assert_eq!(
            parse_args("set --force").unwrap(),
            JevSetupRequest::Set {
                key: None,
                force: true
            }
        );
        match parse_args("set jv_live_abc --force").unwrap() {
            JevSetupRequest::Set {
                key: Some(k),
                force: true,
            } => assert_eq!(k.0, "jv_live_abc"),
            other => panic!("{other:?}"),
        }
        let debug = format!("{:?}", parse_args("set jv_live_abc").unwrap());
        assert!(!debug.contains("jv_live_abc"), "{debug}");
        assert_eq!(
            parse_args("nudge on").unwrap(),
            JevSetupRequest::Flag {
                flag: JevFlag::Nudge,
                on: true
            }
        );
        assert_eq!(
            parse_args("safety off").unwrap(),
            JevSetupRequest::Flag {
                flag: JevFlag::SafetyCheck,
                on: false
            }
        );
        assert!(parse_args("nudge").is_err());
        assert!(parse_args("wat").is_err());
    }

    #[test]
    fn toml_edit_preserves_comments_and_siblings() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "# keep me\n[ui]\ncompact_mode = false\n\n[jev]\n# file key\napi_key = \"old\"\n",
        )
        .unwrap();
        save_api_key_at(&path, "newkey_abcd").unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("# keep me"), "{body}");
        assert!(body.contains("# file key"), "{body}");
        assert!(body.contains("compact_mode = false"), "{body}");
        assert!(body.contains("newkey_abcd"), "{body}");
        set_flag_at(&path, JevFlag::Nudge, true).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("nudge = true"), "{body}");
        assert!(body.contains("# keep me"), "{body}");
        clear_api_key_at(&path).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(!body.contains("newkey_abcd"), "{body}");
        assert!(body.contains("nudge = true"), "{body}");
    }

    #[test]
    fn format_status_never_includes_raw_key() {
        let file = JevConfig {
            api_key: Some("jv_live_should_not_appear".into()),
            ..JevConfig::default()
        };
        let text = format_status(&collect_status(None, None, &file));
        assert!(!text.contains("should_not_appear"));
        assert!(text.contains("••••"));
    }
}
