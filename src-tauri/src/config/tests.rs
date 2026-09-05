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

// A partial models object must parse, and each missing field must stay unset
// (None) without clobbering earlier merge layers.
#[test]
fn partial_models_object_leaves_missing_fields_empty() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
        .expect("partial global models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.small, Some(ModelSpec::Id("global-small".to_string())));
    assert_eq!(merged.large, None);

    let repo: ConfigFile = serde_json::from_str(r#"{"models": {"large": "repo-large"}}"#)
        .expect("partial repo models must parse");
    apply_file(&mut merged, &repo);
    assert_eq!(merged.small, Some(ModelSpec::Id("global-small".to_string())));
    assert_eq!(merged.large, Some(ModelSpec::Id("repo-large".to_string())));
}

// An empty legacy plain-string model is skipped by apply_file, so it never
// clobbers an earlier merge layer for the same key.
#[test]
fn empty_legacy_model_does_not_clobber_earlier_layer() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
        .expect("global model must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.small, Some(ModelSpec::Id("global-small".to_string())));

    let repo: ConfigFile = serde_json::from_str(r#"{"models": {"small": ""}}"#)
        .expect("empty model must parse");
    apply_file(&mut merged, &repo);
    assert_eq!(merged.small, Some(ModelSpec::Id("global-small".to_string())));
}

// The structured { provider, id } form parses into a provider-bound entry.
#[test]
fn structured_models_parse() {
    let file: ConfigFile = serde_json::from_str(
        r#"{"models": {"small": {"provider": "deepseek", "id": "deepseek-v4-flash"}, "large": {"provider": "openai", "id": "openai-codex-5.6"}}}"#,
    )
    .expect("structured models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &file);
    assert_eq!(
        merged.small,
        Some(ModelSpec::Provider {
            provider: "deepseek".to_string(),
            id: "deepseek-v4-flash".to_string(),
        })
    );
    assert_eq!(
        merged.large,
        Some(ModelSpec::Provider {
            provider: "openai".to_string(),
            id: "openai-codex-5.6".to_string(),
        })
    );
}

// One slot structured and one slot legacy parse together; each keeps its own
// form.
#[test]
fn mixed_models_parse() {
    let file: ConfigFile = serde_json::from_str(
        r#"{"models": {"small": {"provider": "deepseek", "id": "deepseek-v4-flash"}, "large": "gpt-4o"}}"#,
    )
    .expect("mixed models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &file);
    assert_eq!(
        merged.small,
        Some(ModelSpec::Provider {
            provider: "deepseek".to_string(),
            id: "deepseek-v4-flash".to_string(),
        })
    );
    assert_eq!(merged.large, Some(ModelSpec::Id("gpt-4o".to_string())));
}

// A project structured model overrides a global legacy model for the same
// key; the project file is applied after the global file.
#[test]
fn project_structured_overrides_global_legacy() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "gpt-4o"}}"#)
        .expect("global legacy must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.small, Some(ModelSpec::Id("gpt-4o".to_string())));

    let project: ConfigFile = serde_json::from_str(
        r#"{"models": {"small": {"provider": "deepseek", "id": "deepseek-v4-flash"}}}"#,
    )
    .expect("project structured must parse");
    apply_file(&mut merged, &project);
    assert_eq!(
        merged.small,
        Some(ModelSpec::Provider {
            provider: "deepseek".to_string(),
            id: "deepseek-v4-flash".to_string(),
        })
    );
}

// An absent models key in a project file preserves the global entry for both
// slots.
#[test]
fn absent_models_key_preserves_global_entry() {
    let global: ConfigFile = serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
        .expect("global model must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);

    let project: ConfigFile = serde_json::from_str(r#"{"debug": true}"#)
        .expect("absent models key must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.small, Some(ModelSpec::Id("global-small".to_string())));
    assert_eq!(merged.large, None);
}

// ConfigStatus exposes the resolved id for both forms and the provider only
// for the structured form.
#[test]
fn config_status_resolves_ids_and_providers() {
    let file: ConfigFile = serde_json::from_str(
        r#"{"models": {"small": {"provider": "deepseek", "id": "deepseek-v4-flash"}, "large": "gpt-4o"}}"#,
    )
    .expect("mixed models must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &file);
    let status = ConfigStatus::from(&merged);
    assert_eq!(status.small_model, "deepseek-v4-flash");
    assert_eq!(status.large_model, "gpt-4o");
    assert_eq!(status.small_provider, Some("deepseek".to_string()));
    assert_eq!(status.large_provider, None);

    // An unset slot resolves to an empty id and no provider.
    let empty = ConfigStatus::from(&MergedConfig::default());
    assert_eq!(empty.small_model, "");
    assert_eq!(empty.large_model, "");
    assert_eq!(empty.small_provider, None);
    assert_eq!(empty.large_provider, None);
}

// The ConfigState accessors resolve the id for both forms and report the
// provider only for the structured form.
#[test]
fn config_state_accessors_resolve_both_forms() {
    let state = ConfigState::default();
    {
        let mut guard = state.inner.lock().expect("config lock");
        guard.small = Some(ModelSpec::Provider {
            provider: "deepseek".to_string(),
            id: "deepseek-v4-flash".to_string(),
        });
        guard.large = Some(ModelSpec::Id("gpt-4o".to_string()));
    }
    assert_eq!(state.small_model(), "deepseek-v4-flash");
    assert_eq!(state.large_model(), "gpt-4o");
    assert_eq!(state.small_provider(), Some("deepseek".to_string()));
    assert_eq!(state.large_provider(), None);
}

// The structured validation helper renders exact messages: an invalid id
// lists the available models, an unavailable list produces a safe "did not
// respond" message, and an unconfigured provider produces the slot-labeled
// API key message.
#[test]
fn validate_structured_model_formats_messages() {
    // Valid id: no messages.
    let valid = validate_structured_model(
        "small",
        "openai",
        "gpt-4o",
        Some(&["gpt-4o".to_string(), "gpt-4o-mini".to_string()]),
    );
    assert!(valid.is_empty(), "valid id must produce no messages");

    // Unknown id: exact message with the sorted available list.
    let invalid = validate_structured_model(
        "small",
        "openai",
        "bogus",
        Some(&["gpt-4o-mini".to_string(), "gpt-4o".to_string()]),
    );
    assert_eq!(
        invalid,
        vec!["Invalid model bogus for provider openai. Available models: gpt-4o, gpt-4o-mini"
            .to_string()]
    );

    // Unavailable list (fetch failed): safe message, no raw error.
    let failed = validate_structured_model("large", "deepseek", "deepseek-v4-flash", None);
    assert_eq!(
        failed,
        vec!["could not check model deepseek-v4-flash: provider deepseek did not respond"
            .to_string()]
    );

    // Empty available list renders the "(none)" placeholder.
    let empty = validate_structured_model("small", "openai", "bogus", Some(&[]));
    assert_eq!(
        empty,
        vec!["Invalid model bogus for provider openai. Available models: (none)".to_string()]
    );

    // The unconfigured-provider message labels the slot.
    assert_eq!(
        unconfigured_provider_message("small", "deepseek"),
        "provider deepseek has no API key configured for the small model"
    );
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

// A poisoned lock must fail closed: the stored debug value is read from the
// recovered guard, so debug:false stays disabled after a panic while the lock
// is held.
#[test]
fn poisoned_lock_debug_fails_closed() {
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

    assert_eq!(state.debug(), false);
}

// mock_llm defaults to true when no config file supplies it, so a fresh
// install works without API keys.
#[test]
fn default_mock_llm_is_true_without_config() {
    let merged = MergedConfig::default();
    assert_eq!(merged.mock_llm, true);
}

// An absent mock_llm key in a project config must not clobber an explicit
// false from the global config. Option<bool> with serde(default) is the
// "not supplied" sentinel that apply_file skips.
#[test]
fn absent_project_mock_llm_preserves_global_false() {
    let global: ConfigFile = serde_json::from_str(r#"{"mockLlm": false}"#)
        .expect("global mockLlm false must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);
    assert_eq!(merged.mock_llm, false);

    let project: ConfigFile = serde_json::from_str(r#"{}"#)
        .expect("absent mockLlm key must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.mock_llm, false);
}

// A project mockLlm:false must override a global mockLlm:true.
#[test]
fn project_mock_llm_false_overrides_global_true() {
    let global: ConfigFile = serde_json::from_str(r#"{"mockLlm": true}"#)
        .expect("global mockLlm true must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);

    let project: ConfigFile = serde_json::from_str(r#"{"mockLlm": false}"#)
        .expect("project mockLlm false must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.mock_llm, false);
}

// A project mockLlm:true must override a global mockLlm:false.
#[test]
fn project_mock_llm_true_overrides_global_false() {
    let global: ConfigFile = serde_json::from_str(r#"{"mockLlm": false}"#)
        .expect("global mockLlm false must parse");
    let mut merged = MergedConfig::default();
    apply_file(&mut merged, &global);

    let project: ConfigFile = serde_json::from_str(r#"{"mockLlm": true}"#)
        .expect("project mockLlm true must parse");
    apply_file(&mut merged, &project);
    assert_eq!(merged.mock_llm, true);
}

// ConfigStatus carries the merged mock_llm flag to the frontend.
#[test]
fn config_status_carries_merged_mock_llm_false() {
    let mut merged = MergedConfig::default();
    let global: ConfigFile = serde_json::from_str(r#"{"mockLlm": false}"#)
        .expect("global mockLlm false must parse");
    apply_file(&mut merged, &global);

    let status = ConfigStatus::from(&merged);
    assert_eq!(status.mock_llm, false);
}

#[test]
fn config_status_carries_merged_mock_llm_true() {
    let merged = MergedConfig::default();
    let status = ConfigStatus::from(&merged);
    assert_eq!(status.mock_llm, true);
}

// A poisoned lock must fail closed: the stored mock_llm value is read
// from the recovered guard, so mock_llm:false stays false after a panic
// while the lock is held.
#[test]
fn poisoned_lock_mock_llm_fails_closed() {
    let state = ConfigState::default();
    {
        let mut guard = state.inner.lock().expect("config lock");
        guard.mock_llm = false;
    }

    let result = std::panic::catch_unwind(|| {
        let _guard = state.inner.lock().expect("config lock");
        panic!("poison the config lock");
    });
    assert!(result.is_err(), "catch_unwind must observe the panic");

    assert_eq!(state.mock_llm(), false);
}
