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

// A partial models object must parse, and each missing field must stay empty
// (unset) without clobbering earlier merge layers.
#[test]
fn partial_models_object_leaves_missing_fields_empty() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
        .expect("partial global models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.small_model, "global-small");
    assert_eq!(merged.large_model, "");

    let repo: ConfigFile = serde_json::from_str(r#"{"models": {"large": "repo-large"}}"#)
        .expect("partial repo models must parse");
    apply_file(&mut merged, &repo);
    assert_eq!(merged.small_model, "global-small");
    assert_eq!(merged.large_model, "repo-large");
}

// The pure validation helper renders exact messages: empty models get a
// "not configured" message, unknown models get the sorted available list.
#[test]
fn validate_model_strings_formats_messages() {
    // Both models present and valid: no messages.
    let valid = validate_model_strings(
        "gpt-4o",
        "gpt-4o-mini",
        &["gpt-4o".to_string(), "gpt-4o-mini".to_string()],
    );
    assert!(valid.is_empty(), "valid models must produce no messages");

    // Unknown small model: exact message with the available list.
    let invalid = validate_model_strings("bogus", "gpt-4o", &["gpt-4o".to_string()]);
    assert_eq!(
        invalid,
        vec!["Invalid model bogus. Available models: gpt-4o".to_string()]
    );

    // Empty small model: exact "not configured" message.
    let missing = validate_model_strings("", "gpt-4o", &["gpt-4o".to_string()]);
    assert_eq!(missing, vec!["No small model configured".to_string()]);

    // The available list renders sorted even when passed unsorted.
    let unsorted = validate_model_strings(
        "bogus",
        "gpt-4o",
        &["gpt-4o-mini".to_string(), "gpt-4o".to_string()],
    );
    assert_eq!(
        unsorted,
        vec!["Invalid model bogus. Available models: gpt-4o, gpt-4o-mini".to_string()]
    );

    // An empty available list renders the "(none)" placeholder with no
    // trailing space after the colon.
    let empty_available = validate_model_strings("bogus", "", &[]);
    assert_eq!(
        empty_available,
        vec![
            "Invalid model bogus. Available models: (none)".to_string(),
            "No large model configured".to_string(),
        ]
    );
    assert!(
        !empty_available[0].ends_with(' '),
        "message must not end with a trailing space: {}",
        empty_available[0]
    );

    // The large-model message uses the same placeholder when the list is empty.
    let empty_available_large = validate_model_strings("", "bogus", &[]);
    assert_eq!(
        empty_available_large,
        vec![
            "No small model configured".to_string(),
            "Invalid model bogus. Available models: (none)".to_string(),
        ]
    );
}

// An absent debug key in a project config must not clobber an explicit
// false from the global config. Option<bool> with serde(default) is the
// "not supplied" sentinel that apply_file skips.
#[test]
fn absent_project_debug_preserves_global_false() {
    let global: ConfigFile = serde_json::from_str(r#"{"debug": false}"#)
        .expect("global debug false must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.debug, false);

    let project: ConfigFile = serde_json::from_str(r#"{}"#)
        .expect("absent debug key must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.debug, false);
}

// A project debug:false must override a global debug:true. Project wins
// because it is applied after the global file.
#[test]
fn project_debug_false_overrides_global_true() {
    let global: ConfigFile = serde_json::from_str(r#"{"debug": true}"#)
        .expect("global debug true must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);

    let project: ConfigFile = serde_json::from_str(r#"{"debug": false}"#)
        .expect("project debug false must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.debug, false);
}

// A project debug:true must override a global debug:false.
#[test]
fn project_debug_true_overrides_global_false() {
    let global: ConfigFile = serde_json::from_str(r#"{"debug": false}"#)
        .expect("global debug false must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);

    let project: ConfigFile = serde_json::from_str(r#"{"debug": true}"#)
        .expect("project debug true must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.debug, true);
}

// Debug defaults to true when no config file supplies it.
#[test]
fn default_debug_is_true_without_config() {
    let merged = MergedConfig::default();
    assert_eq!(merged.debug, true);
}

// ConfigStatus carries the merged debug flag to the frontend.
#[test]
fn config_status_carries_merged_debug_false() {
    let mut merged = MergedConfig::default();
    let global: ConfigFile = serde_json::from_str(r#"{"debug": false}"#)
        .expect("global debug false must parse");
    apply_file(&mut merged, &global);

    let status = ConfigStatus::from(&merged);
    assert_eq!(status.debug, false);
}

#[test]
fn config_status_carries_merged_debug_true() {
    let merged = MergedConfig::default();
    let status = ConfigStatus::from(&merged);
    assert_eq!(status.debug, true);
}

// A poisoned lock must fall back to true: debug events stay enabled rather
// than silently disabled after a panic while the lock is held.
#[test]
fn poisoned_lock_debug_falls_back_to_true() {
    let state = ConfigState::default();
    {
        let mut guard = state.inner.lock().expect("config lock");
        guard.debug = false;
    }

    let result = std::panic::catch_unwind(|| {
        let _guard = state.inner.lock().expect("config lock");
        panic!("poison the config lock");
    });
    assert!(result.is_err(), "catch_unwind must observe the panic");

    assert_eq!(state.debug(), true);
}
