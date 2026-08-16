use super::*;

// A malformed apiKeys value must never surface its raw value through the
// config error path. The sanitized error keeps only the generic reason and
// the position.
#[test]
fn malformed_api_keys_error_does_not_leak_secret() {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "orange-yeoman-config-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before unix epoch")
            .as_nanos()
    ));
    std::fs::write(&path, r#"{"apiKeys":"sk-live-secret"}"#).expect("write temp config");
    let result = read_config_file(&path);
    let _ = std::fs::remove_file(&path);

    let err = match result {
        Ok(_) => panic!("malformed config must error"),
        Err(e) => e,
    };
    assert!(!err.contains("sk-live-secret"), "secret leaked: {err}");
    assert!(
        !err.contains("invalid type"),
        "raw serde text leaked: {err}"
    );
    assert!(
        err.contains("invalid config JSON"),
        "missing generic reason: {err}"
    );
    assert!(err.contains("line 1"), "missing line info: {err}");
}

// A partial models object must parse, and each missing field must fall
// back to the built-in default without clobbering earlier layers.
#[test]
fn partial_models_object_uses_builtin_defaults_for_missing_fields() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
        .expect("partial global models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.small_model, "global-small");
    assert_eq!(merged.large_model, DEFAULT_LARGE_MODEL);

    let repo: ConfigFile = serde_json::from_str(r#"{"models": {"large": "repo-large"}}"#)
        .expect("partial repo models must parse");
    apply_file(&mut merged, &repo);
    assert_eq!(merged.small_model, "global-small");
    assert_eq!(merged.large_model, "repo-large");
}
