//! Public-API tests for `/jev-setup` helpers.
//! Lives here so it can run without compiling the pre-existing
//! `tool_layer_images_bridge_tests` lib-test compile break.

use xai_grok_shell::util::config::{
    JevConfig, JevFlag, JevKeySource, JevSetupRequest, clear_api_key_at, collect_status,
    format_status, mask_key, parse_args, save_api_key_at, scrub_key, set_flag_at,
};

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
fn parse_args_debug_never_includes_raw_key() {
    match parse_args("set jv_live_abc --force").unwrap() {
        JevSetupRequest::Set {
            key: Some(k),
            force: true,
        } => assert_eq!(k.0, "jv_live_abc"),
        other => panic!("{other:?}"),
    }
    let debug = format!("{:?}", parse_args("set jv_live_abc").unwrap());
    assert!(!debug.contains("jv_live_abc"), "{debug}");
    assert_eq!(parse_args("stats").unwrap(), JevSetupRequest::Stats);
}

#[test]
fn toml_edit_preserves_comments_and_siblings() {
    let dir = tempfile::tempdir().unwrap();
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
