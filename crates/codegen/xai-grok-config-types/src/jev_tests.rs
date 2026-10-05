use super::*;

#[test]
fn empty_env_and_empty_file_are_disabled() {
    assert_eq!(resolve_jev_key(None, None, None), None);
}

#[test]
fn file_key_only_enables() {
    assert_eq!(
        resolve_jev_key(None, None, Some("file-key")),
        Some("file-key".to_owned())
    );
}

#[test]
fn empty_string_tiers_do_not_count() {
    assert_eq!(
        resolve_jev_key(Some(""), Some("  "), Some("file-key")),
        Some("file-key".to_owned())
    );
    assert_eq!(resolve_jev_key(Some(""), Some(""), Some("")), None);
    assert_eq!(resolve_jev_key(Some("   "), Some("\t"), Some("\n")), None);
}

#[test]
fn jev_api_key_overrides_file_and_typesafe() {
    assert_eq!(
        resolve_jev_key(Some("jev-key"), Some("typesafe-key"), Some("file-key")),
        Some("jev-key".to_owned())
    );
}

#[test]
fn typesafe_used_when_jev_unset() {
    assert_eq!(
        resolve_jev_key(None, Some("typesafe-key"), Some("file-key")),
        Some("typesafe-key".to_owned())
    );
    assert_eq!(
        resolve_jev_key(Some(""), Some("typesafe-key"), Some("file-key")),
        Some("typesafe-key".to_owned())
    );
}

#[test]
fn default_nudge_on_other_feature_flags_off() {
    let cfg = JevConfig::default();
    assert!(cfg.api_key.is_none());
    assert!(cfg.base_url.is_none());
    assert!(cfg.model.is_none());
    assert!(cfg.nudge);
    assert_eq!(cfg.nudge_every, 2);
    assert!(!cfg.safety_check);
    assert!(!cfg.context_filter);
    assert!(cfg.route);
    assert!(cfg.risk_block.is_none());
    assert!(!cfg.allow_destructive);
}

#[test]
fn toml_round_trip_keeps_file_key_and_defaults() {
    let parsed: JevConfig = toml::from_str("api_key = \"from-file\"\n").unwrap();
    assert_eq!(parsed.api_key.as_deref(), Some("from-file"));
    assert!(parsed.nudge, "omitted nudge defaults on");
    assert_eq!(parsed.nudge_every, 2);
    assert!(!parsed.safety_check);
    assert!(!parsed.context_filter);
    assert!(parsed.route, "omitted route defaults on");
    let off: JevConfig = toml::from_str("api_key = \"from-file\"\nnudge = false\n").unwrap();
    assert!(!off.nudge);
}
