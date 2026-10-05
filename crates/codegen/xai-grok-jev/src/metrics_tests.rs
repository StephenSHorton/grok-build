use super::*;
use crate::types::Usage;

fn rec(source: DecideSource, ok: bool, latency: u64) -> DecideRecord {
    DecideRecord {
        ts_ms: 1,
        source,
        latency_ms: latency,
        ok,
        error_kind: if ok {
            None
        } else {
            Some(DecideErrorKind::Http)
        },
        question_count: 1,
        modes: vec!["noul".into()],
        input_tokens: Some(10),
        output_tokens: Some(2),
        state_bytes: 16,
        safety: None,
        filter: None,
        snippet_chars: None,
    }
}

#[test]
fn percentile_empty_and_single() {
    assert_eq!(percentile_ms(&[], 0.5), None);
    assert_eq!(percentile_ms(&[42], 0.5), Some(42));
    assert_eq!(percentile_ms(&[42], 0.95), Some(42));
}

#[test]
fn percentile_p50_p95() {
    let xs: Vec<u64> = (1..=20).collect();
    assert_eq!(percentile_ms(&xs, 0.50), Some(10));
    assert_eq!(percentile_ms(&xs, 0.95), Some(19));
}

#[test]
fn accumulator_tracks_source_tokens_and_failures() {
    let mut acc = StatsAccumulator::default();
    acc.add(&rec(DecideSource::AskJev, true, 100));
    acc.add(&rec(DecideSource::AskJev, true, 200));
    let mut fail = rec(DecideSource::Safety, false, 400);
    fail.input_tokens = None;
    fail.output_tokens = None;
    fail.safety = Some(SafetyOutcome::Allowed { noul: None });
    acc.add(&fail);
    let mut deny = rec(DecideSource::Safety, true, 150);
    deny.safety = Some(SafetyOutcome::Denied { noul: 0.9 });
    acc.add(&deny);
    let mut drop = rec(DecideSource::Filter, true, 80);
    drop.filter = Some(FilterOutcome::Dropped);
    drop.snippet_chars = Some(1200);
    acc.add(&drop);

    assert_eq!(acc.total.calls, 5);
    assert_eq!(acc.total.fail, 1);
    assert_eq!(acc.total.safety_deny, 1);
    assert_eq!(acc.total.filter_dropped, 1);
    assert_eq!(acc.total.filter_saved_chars, 1200);
    assert_eq!(acc.by_source.get(&DecideSource::AskJev).unwrap().ok, 2);
    assert_eq!(acc.total.input_tokens, 40);
    assert!(acc.total.fail_rate() > 0.0);
}

#[test]
fn nudge_follow_through_within_window() {
    let mut acc = StatsAccumulator::default();
    acc.record_nudge_shown();
    acc.observe_tool_call("bash");
    acc.observe_tool_call("read_file");
    acc.observe_tool_call("ask_jev");
    assert_eq!(acc.nudges_shown, 1);
    assert_eq!(acc.nudges_followed, 1);
    assert_eq!(acc.nudge_pending, 0);

    acc.record_nudge_shown();
    for _ in 0..NUDGE_FOLLOW_WINDOW {
        acc.observe_tool_call("bash");
    }
    acc.observe_tool_call("ask_jev");
    assert_eq!(
        acc.nudges_followed, 1,
        "ask_jev after the window does not count"
    );
}

#[test]
fn format_report_off_empty() {
    let text = format_stats_report(&StatsAccumulator::default(), None, false);
    assert!(text.contains("Jev is off"));
    assert!(!text.contains("api_key"));
}

#[test]
fn sample_stats_output_shape() {
    let mut session = StatsAccumulator::default();
    for (i, lat) in [180u64, 142, 210, 190, 165, 400].into_iter().enumerate() {
        let mut r = rec(DecideSource::AskJev, i != 5, lat);
        if i == 5 {
            r.input_tokens = None;
            r.output_tokens = None;
        }
        session.add(&r);
    }
    for lat in [90u64, 120, 410] {
        let mut r = rec(DecideSource::Safety, true, lat);
        r.safety = Some(SafetyOutcome::Allowed { noul: Some(0.2) });
        session.add(&r);
    }
    let mut deny = rec(DecideSource::Safety, true, 150);
    deny.safety = Some(SafetyOutcome::Denied { noul: 0.88 });
    session.add(&deny);
    let mut drop = rec(DecideSource::Filter, true, 80);
    drop.filter = Some(FilterOutcome::Dropped);
    drop.snippet_chars = Some(1200);
    session.add(&drop);
    let mut keep = rec(DecideSource::Filter, false, 95);
    keep.filter = Some(FilterOutcome::Kept);
    keep.snippet_chars = Some(400);
    session.add(&keep);
    session.add(&rec(DecideSource::Setup, true, 60));
    session.nudges_shown = 2;
    session.nudges_followed = 1;

    let text = format_stats_report(&session, Some(&session), true);
    assert!(text.contains("Jev stats (this session)"));
    assert!(text.contains("By source"));
    assert!(text.contains("ask_jev"));
    assert!(text.contains("safety"));
    assert!(text.contains("filter"));
    assert!(text.contains("setup"));
    assert!(text.contains("Nudges: 2 shown, 1 followed"));
    assert!(text.contains("All-time (local log)"));
    assert!(!text.contains("jv_live_"));
    eprintln!("--- /jev-stats sample ---\n{text}\n--- end ---");
}

#[test]
fn format_report_session_and_all_time() {
    let mut session = StatsAccumulator::default();
    session.add(&rec(DecideSource::AskJev, true, 142));
    let mut drop = rec(DecideSource::Filter, true, 80);
    drop.filter = Some(FilterOutcome::Dropped);
    drop.snippet_chars = Some(1200);
    session.add(&drop);
    session.record_nudge_shown();
    session.observe_tool_call("ask_jev");

    let text = format_stats_report(&session, Some(&session), true);
    assert!(text.contains("Jev stats (this session)"));
    assert!(text.contains("ask_jev"));
    assert!(text.contains("filter"));
    assert!(text.contains("dropped 1"));
    assert!(text.contains("Nudges: 1 shown, 1 followed"));
    assert!(text.contains("All-time (local log)"));
    assert!(!text.contains("jv_live_"));
    assert!(!text.contains("secret"));
}

#[test]
fn jsonl_roundtrip_and_cap_never_stores_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STATS_LOG_NAME);
    let mut rec = rec(DecideSource::Setup, true, 12);
    rec.input_tokens = Some(3);
    rec.output_tokens = Some(1);
    append_decide_log(&path, &rec).unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("\"source\":\"setup\""));
    assert!(body.contains("state_bytes"));
    assert!(!body.contains("\"state\":"));
    assert!(!body.contains("api_key"));
    assert!(!body.to_ascii_lowercase().contains("bearer"));

    for i in 0..30 {
        let mut extra = rec.clone();
        extra.latency_ms = i;
        append_decide_log(&path, &extra).unwrap();
    }
    // Tiny cap forces a rewrite that keeps the newest lines only.
    trim_jsonl(&path, 80, 3).unwrap();
    let kept = std::fs::read_to_string(&path).unwrap();
    assert!(kept.lines().count() <= 3);

    let loaded = load_all_time(&path).expect("loaded");
    assert!(loaded.total.calls <= 3);
}

#[test]
fn metrics_persist_only_when_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jev_stats.jsonl");
    let mut m = JevMetrics::session_only();
    m.record(rec(DecideSource::AskJev, true, 10));
    assert!(!path.exists(), "session-only must not create a file");

    let mut m = JevMetrics::with_persist(&path);
    m.record(rec(DecideSource::AskJev, true, 10));
    assert!(path.exists());
    m.set_persist_enabled(false);
    let len = std::fs::metadata(&path).unwrap().len();
    m.record(rec(DecideSource::Safety, true, 11));
    assert_eq!(
        std::fs::metadata(&path).unwrap().len(),
        len,
        "disabled persist must not append"
    );
    assert_eq!(m.session().total.calls, 2);
}

#[test]
fn record_from_parts_copies_usage_not_contents() {
    let usage = Usage {
        input_tokens: Some(8),
        output_tokens: Some(1),
    };
    let rec = DecideRecord::from_parts(
        DecideSource::AskJev,
        5,
        true,
        None,
        2,
        vec!["boolean".into(), "choice".into()],
        Some(&usage),
        32,
    );
    let json = serde_json::to_string(&rec).unwrap();
    assert!(json.contains("\"input_tokens\":8"));
    assert!(!json.contains("\"questions\""));
    assert!(!json.contains("\"state\":"));
    assert_eq!(rec.state_bytes, 32);
}
