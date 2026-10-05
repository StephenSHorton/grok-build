//! Decide-call metrics. Sizes and outcomes only — never keys or state contents.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::types::Usage;

/// JSONL filename under `GROK_HOME`. Created only after the first recorded Decide.
pub const STATS_LOG_NAME: &str = "jev_stats.jsonl";

/// Soft cap for the rolling local log. Oldest lines are dropped when exceeded.
pub const STATS_LOG_MAX_BYTES: u64 = 256 * 1024;

/// Soft line cap paired with [`STATS_LOG_MAX_BYTES`].
pub const STATS_LOG_MAX_LINES: usize = 2000;

/// Tool calls watched after a nudge for an `ask_jev` follow-through.
pub const NUDGE_FOLLOW_WINDOW: u32 = 5;

/// In-memory latency samples kept per accumulator (session or all-time load).
const LATENCY_CAP: usize = 1024;

/// Who issued the Decide call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecideSource {
    AskJev,
    Safety,
    Filter,
    Setup,
}

impl DecideSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::AskJev => "ask_jev",
            Self::Safety => "safety",
            Self::Filter => "filter",
            Self::Setup => "setup",
        }
    }
}

/// Transport / decode failure class. Bodies and keys are never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecideErrorKind {
    MissingKey,
    Http,
    ResponseTooLarge,
    Timeout,
    Request,
    Decode,
}

impl DecideErrorKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::MissingKey => "missing_key",
            Self::Http => "http",
            Self::ResponseTooLarge => "response_too_large",
            Self::Timeout => "timeout",
            Self::Request => "request",
            Self::Decode => "decode",
        }
    }
}

/// Live safety outcome. Fail-open Allow has `noul: None` when Jev did not answer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyOutcome {
    Allowed { noul: Option<f64> },
    Denied { noul: f64 },
}

/// Live filter outcome. Dropped items report the original snippet size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOutcome {
    Kept,
    Dropped,
}

/// One Decide attempt. No keys, no state payload — sizes only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecideRecord {
    pub ts_ms: u64,
    pub source: DecideSource,
    pub latency_ms: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<DecideErrorKind>,
    pub question_count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<i64>,
    pub state_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<SafetyOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<FilterOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet_chars: Option<usize>,
}

impl DecideRecord {
    pub fn from_parts(
        source: DecideSource,
        latency_ms: u64,
        ok: bool,
        error_kind: Option<DecideErrorKind>,
        question_count: u32,
        modes: Vec<String>,
        usage: Option<&Usage>,
        state_bytes: usize,
    ) -> Self {
        Self {
            ts_ms: unix_ms(),
            source,
            latency_ms,
            ok,
            error_kind,
            question_count,
            modes,
            input_tokens: usage.and_then(|u| u.input_tokens),
            output_tokens: usage.and_then(|u| u.output_tokens),
            state_bytes,
            safety: None,
            filter: None,
            snippet_chars: None,
        }
    }

    pub fn with_safety(mut self, safety: SafetyOutcome) -> Self {
        self.safety = Some(safety);
        self
    }

    pub fn with_filter(mut self, filter: FilterOutcome, snippet_chars: usize) -> Self {
        self.filter = Some(filter);
        self.snippet_chars = Some(snippet_chars);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SourceStats {
    pub calls: u32,
    pub ok: u32,
    pub fail: u32,
    pub latencies_ms: Vec<u64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub usage_calls: u32,
    pub safety_deny: u32,
    pub filter_dropped: u32,
    pub filter_saved_chars: u64,
    pub errors: BTreeMap<DecideErrorKind, u32>,
}

impl SourceStats {
    pub fn add(&mut self, rec: &DecideRecord) {
        self.calls = self.calls.saturating_add(1);
        if rec.ok {
            self.ok = self.ok.saturating_add(1);
        } else {
            self.fail = self.fail.saturating_add(1);
            if let Some(kind) = rec.error_kind {
                *self.errors.entry(kind).or_insert(0) += 1;
            }
        }
        push_capped(&mut self.latencies_ms, rec.latency_ms);
        if rec.input_tokens.is_some() || rec.output_tokens.is_some() {
            self.usage_calls = self.usage_calls.saturating_add(1);
            self.input_tokens = self
                .input_tokens
                .saturating_add(rec.input_tokens.unwrap_or(0));
            self.output_tokens = self
                .output_tokens
                .saturating_add(rec.output_tokens.unwrap_or(0));
        }
        if matches!(rec.safety, Some(SafetyOutcome::Denied { .. })) {
            self.safety_deny = self.safety_deny.saturating_add(1);
        }
        if rec.filter == Some(FilterOutcome::Dropped) {
            self.filter_dropped = self.filter_dropped.saturating_add(1);
            self.filter_saved_chars = self
                .filter_saved_chars
                .saturating_add(rec.snippet_chars.unwrap_or(0) as u64);
        }
    }

    pub fn fail_rate(&self) -> f64 {
        if self.calls == 0 {
            0.0
        } else {
            f64::from(self.fail) / f64::from(self.calls)
        }
    }

    pub fn p50(&self) -> Option<u64> {
        percentile_ms(&self.latencies_ms, 0.50)
    }

    pub fn p95(&self) -> Option<u64> {
        percentile_ms(&self.latencies_ms, 0.95)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsAccumulator {
    pub total: SourceStats,
    pub by_source: BTreeMap<DecideSource, SourceStats>,
    pub nudges_shown: u32,
    pub nudges_followed: u32,
    pub nudge_pending: u32,
}

impl StatsAccumulator {
    pub fn add(&mut self, rec: &DecideRecord) {
        self.total.add(rec);
        self.by_source.entry(rec.source).or_default().add(rec);
    }

    pub fn record_nudge_shown(&mut self) {
        self.nudges_shown = self.nudges_shown.saturating_add(1);
        self.nudge_pending = NUDGE_FOLLOW_WINDOW;
    }

    pub fn observe_tool_call(&mut self, name: &str) {
        if self.nudge_pending == 0 {
            return;
        }
        if name == "ask_jev" {
            self.nudges_followed = self.nudges_followed.saturating_add(1);
            self.nudge_pending = 0;
        } else {
            self.nudge_pending = self.nudge_pending.saturating_sub(1);
        }
    }

    pub fn has_nudge_window(&self) -> bool {
        self.nudge_pending > 0
    }

    pub fn is_empty(&self) -> bool {
        self.total.calls == 0 && self.nudges_shown == 0
    }
}

/// Session store. Persist path is set only while Jev is on; the first [`Self::record`]
/// creates the JSONL. Absent persist path ⇒ no files.
#[derive(Debug, Clone)]
pub struct JevMetrics {
    session: StatsAccumulator,
    persist_path: Option<PathBuf>,
    log_path: Option<PathBuf>,
}

impl JevMetrics {
    pub fn session_only() -> Self {
        Self {
            session: StatsAccumulator::default(),
            persist_path: None,
            log_path: None,
        }
    }

    pub fn with_persist(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            session: StatsAccumulator::default(),
            persist_path: Some(path.clone()),
            log_path: Some(path),
        }
    }

    pub fn set_persist_enabled(&mut self, enabled: bool) {
        if enabled {
            if self.persist_path.is_none() {
                self.persist_path = self.log_path.clone();
            }
        } else {
            self.persist_path = None;
        }
    }

    pub fn set_log_path(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.log_path = Some(path.clone());
        if self.persist_path.is_some() {
            self.persist_path = Some(path);
        }
    }

    pub fn persist_enabled(&self) -> bool {
        self.persist_path.is_some()
    }

    pub fn session(&self) -> &StatsAccumulator {
        &self.session
    }

    pub fn record(&mut self, rec: DecideRecord) {
        self.session.add(&rec);
        if let Some(path) = &self.persist_path {
            let _ = append_decide_log(path, &rec);
        }
    }

    pub fn record_nudge_shown(&mut self) {
        self.session.record_nudge_shown();
    }

    pub fn observe_tool_call(&mut self, name: &str) {
        self.session.observe_tool_call(name);
    }

    pub fn format_report(&self, jev_on: bool) -> String {
        let all_time = self
            .log_path
            .as_deref()
            .and_then(load_all_time)
            .filter(|a| !a.is_empty());
        format_stats_report(&self.session, all_time.as_ref(), jev_on)
    }
}

/// `{grok_home}/jev_stats.jsonl`.
pub fn stats_log_path(grok_home: &Path) -> PathBuf {
    grok_home.join(STATS_LOG_NAME)
}

/// Append one JSONL record and trim if the file is over the cap. No-op of contents.
pub fn append_decide_log(path: &Path, record: &DecideRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(record)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    drop(file);
    trim_jsonl(path, STATS_LOG_MAX_BYTES, STATS_LOG_MAX_LINES)
}

/// Load every JSONL record into an accumulator. Missing file ⇒ `None`.
pub fn load_all_time(path: &Path) -> Option<StatsAccumulator> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut body = String::new();
    file.read_to_string(&mut body).ok()?;
    if body.trim().is_empty() {
        return None;
    }
    let mut acc = StatsAccumulator::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(rec) = serde_json::from_str::<DecideRecord>(line) {
            acc.add(&rec);
        }
    }
    Some(acc)
}

pub fn format_stats_report(
    session: &StatsAccumulator,
    all_time: Option<&StatsAccumulator>,
    jev_on: bool,
) -> String {
    let mut lines = Vec::new();
    if session.is_empty() {
        if jev_on {
            lines.push("Jev is on. No Decide calls this session yet.".to_owned());
        } else {
            lines.push(
                "Jev is off. No Decide calls this session. Metrics are not collected while Jev is off."
                    .to_owned(),
            );
        }
    } else {
        lines.push("Jev stats (this session)".to_owned());
        push_summary(&mut lines, session);
        lines.push("By source".to_owned());
        for source in [
            DecideSource::AskJev,
            DecideSource::Safety,
            DecideSource::Filter,
            DecideSource::Setup,
        ] {
            if let Some(stats) = session.by_source.get(&source) {
                lines.push(format_source_line(source, stats));
            }
        }
        if session.nudges_shown > 0 || session.nudges_followed > 0 {
            lines.push(format!(
                "Nudges: {} shown, {} followed by ask_jev within {NUDGE_FOLLOW_WINDOW} tool calls",
                session.nudges_shown, session.nudges_followed
            ));
        }
    }
    if let Some(all) = all_time.filter(|a| !a.is_empty()) {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push("All-time (local log)".to_owned());
        push_all_time(&mut lines, all);
    }
    lines.join("\n")
}

fn push_summary(lines: &mut Vec<String>, acc: &StatsAccumulator) {
    let t = &acc.total;
    let fail_pct = t.fail_rate() * 100.0;
    lines.push(format!(
        "Calls: {}  ok: {}  failed: {}  ({fail_pct:.1}% fail)",
        t.calls, t.ok, t.fail
    ));
    match (t.p50(), t.p95()) {
        (Some(p50), Some(p95)) => {
            lines.push(format!("Latency: p50 {p50}ms  p95 {p95}ms"));
        }
        (Some(p50), None) => lines.push(format!("Latency: p50 {p50}ms")),
        _ => {}
    }
    if t.usage_calls > 0 {
        lines.push(format!(
            "Tokens: in {}  out {}  (usage on {}/{} calls)",
            t.input_tokens, t.output_tokens, t.usage_calls, t.calls
        ));
    } else if t.calls > 0 {
        lines.push("Tokens: (usage not present on these calls)".to_owned());
    }
    if !t.errors.is_empty() {
        let kinds: Vec<String> = t
            .errors
            .iter()
            .map(|(k, n)| format!("{} {n}", k.label()))
            .collect();
        lines.push(format!("Failures: {}", kinds.join(", ")));
    }
}

fn push_all_time(lines: &mut Vec<String>, acc: &StatsAccumulator) {
    let t = &acc.total;
    let fail_pct = t.fail_rate() * 100.0;
    let mut call = format!("  Calls: {}  fail {fail_pct:.1}%", t.calls);
    if let (Some(p50), Some(p95)) = (t.p50(), t.p95()) {
        call.push_str(&format!("  p50 {p50}ms  p95 {p95}ms"));
    }
    lines.push(call);
    if t.usage_calls > 0 {
        lines.push(format!(
            "  Tokens: in {}  out {}",
            t.input_tokens, t.output_tokens
        ));
    }
    let filter = acc.by_source.get(&DecideSource::Filter);
    if let Some(filter) = filter
        && (filter.filter_dropped > 0 || filter.calls > 0)
    {
        lines.push(format!(
            "  Filter dropped: {}  saved ~{}",
            filter.filter_dropped,
            format_saved(filter.filter_saved_chars)
        ));
    }
    let deny = acc
        .by_source
        .get(&DecideSource::Safety)
        .map(|s| s.safety_deny)
        .unwrap_or(0);
    if deny > 0 {
        lines.push(format!("  Safety denials: {deny}"));
    }
    if acc.nudges_shown > 0 {
        lines.push(format!(
            "  Nudges: {} shown, {} followed",
            acc.nudges_shown, acc.nudges_followed
        ));
    }
}

fn format_source_line(source: DecideSource, stats: &SourceStats) -> String {
    let mut line = format!(
        "  {:<14} {:>3}  ok {:>3}  fail {:>3}",
        source.label(),
        stats.calls,
        stats.ok,
        stats.fail
    );
    if let Some(p50) = stats.p50() {
        line.push_str(&format!("  p50 {p50}ms"));
    }
    match source {
        DecideSource::Safety if stats.safety_deny > 0 || stats.calls > 0 => {
            line.push_str(&format!("  deny {}", stats.safety_deny));
        }
        DecideSource::Filter if stats.filter_dropped > 0 || stats.calls > 0 => {
            line.push_str(&format!(
                "  dropped {}  saved ~{}",
                stats.filter_dropped,
                format_saved(stats.filter_saved_chars)
            ));
        }
        _ => {}
    }
    line
}

fn format_saved(chars: u64) -> String {
    let tok = chars / 4;
    if chars >= 1000 {
        format!("{:.1}k chars (~{tok} tok)", chars as f64 / 1000.0)
    } else {
        format!("{chars} chars (~{tok} tok)")
    }
}

pub fn percentile_ms(latencies: &[u64], p: f64) -> Option<u64> {
    if latencies.is_empty() {
        return None;
    }
    let mut sorted = latencies.to_vec();
    sorted.sort_unstable();
    let rank = ((sorted.len() as f64) * p).ceil() as usize;
    let idx = rank.clamp(1, sorted.len()).saturating_sub(1);
    sorted.get(idx).copied()
}

fn push_capped(buf: &mut Vec<u64>, value: u64) {
    if buf.len() >= LATENCY_CAP {
        buf.remove(0);
    }
    buf.push(value);
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn trim_jsonl(path: &Path, max_bytes: u64, max_lines: usize) -> std::io::Result<()> {
    let meta = std::fs::metadata(path)?;
    let body = std::fs::read_to_string(path)?;
    let line_count = body.lines().count();
    if meta.len() <= max_bytes && line_count <= max_lines {
        return Ok(());
    }
    let lines: Vec<&str> = body.lines().filter(|l| !l.is_empty()).collect();
    let keep = max_lines.min(lines.len());
    let start = lines.len().saturating_sub(keep);
    let kept = lines.get(start..).unwrap_or(&[]);
    let mut out = kept.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(path, out)
}

/// JSON byte length of `state` for the record. Never stores the value.
pub fn state_byte_len<S: Serialize>(state: &S) -> usize {
    serde_json::to_vec(state).map(|b| b.len()).unwrap_or(0)
}

#[cfg(test)]
#[path = "metrics_tests.rs"]
mod tests;
