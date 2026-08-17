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
