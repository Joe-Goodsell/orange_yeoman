//! Tauri commands exposing the merged config to the frontend.

use std::collections::HashMap;
use std::path::PathBuf;

use super::load::reload_config_state;
use super::model::{ConfigStatus, MergedConfig, ModelSpec};
use super::state::ConfigState;
use super::validate::{
    unconfigured_provider_message, validate_model_strings, validate_structured_model,
};

#[tauri::command]
pub(crate) fn get_config_status(state: tauri::State<ConfigState>) -> ConfigStatus {
    match state.inner.lock() {
        Ok(guard) => ConfigStatus::from(&*guard),
        Err(_) => {
            let fallback = ConfigStatus::from(&MergedConfig::default());
            ConfigStatus {
                error: Some("config state lock is poisoned".to_string()),
                ..fallback
            }
        }
    }
}

// Load (or clear) the repository config for the given root and refresh the
// merged config. root = None clears the project portion of the config state.
// The provider is re-selected from the mockLlm flag so a reload can switch
// between the mock and the real provider. Tauri injects the state parameters;
// the frontend IPC signature is unchanged.
#[tauri::command]
pub(crate) fn load_project_config(
    root: Option<String>,
    state: tauri::State<ConfigState>,
    llm_state: tauri::State<'_, crate::llm::LlmState>,
) -> Result<ConfigStatus, String> {
    let root_path = root.as_deref().map(PathBuf::from);
    let status = reload_config_state(&state, root_path.as_deref());
    llm_state.set_provider(crate::llm::select_provider(state.mock_llm()));
    Ok(status)
}

// Validate the configured small and large models. Structured { provider, id }
// entries are checked against their named provider's /models endpoint only.
// Legacy plain-string entries keep the old union behavior across all
// configured providers. The config is snapshotted and the lock is dropped
// before any network I/O. Returns the current status when the models are
// valid; otherwise returns a user-facing error string. Each provider fetch
// failure is logged to stderr for developers only; the user message stays
// focused on model validity.
#[tauri::command]
pub(crate) async fn validate_models(
    state: tauri::State<'_, ConfigState>,
) -> Result<ConfigStatus, String> {
    // Snapshot the config under the lock, then drop the guard before network
    // I/O. A poisoned lock is a hard error here: validation needs real values.
    let snapshot = {
        let guard = match state.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("config state lock is poisoned".to_string()),
        };
        MergedConfig {
            api_keys: guard.api_keys.clone(),
            small: guard.small.clone(),
            large: guard.large.clone(),
            global_loaded: guard.global_loaded,
            project_loaded: guard.project_loaded,
            project_path: guard.project_path.clone(),
            error: guard.error.clone(),
            debug: guard.debug,
            mock_llm: guard.mock_llm,
        }
    };

    // Mock mode does not require valid models: the mock provider ignores the
    // configured model ids and performs no network I/O, so any model string
    // (or placeholder id) works. Skip validation entirely in mock mode.
    if snapshot.mock_llm {
        return Ok(ConfigStatus::from(&snapshot));
    }

    let api_keys = &snapshot.api_keys;
    let mut messages: Vec<String> = Vec::new();

    // Structured entries, in slot order. Each carries its named provider, so
    // it validates against that provider's model list only.
    let mut structured_entries: Vec<(&str, &str, &str)> = Vec::new();
    if let Some(ModelSpec::Provider { provider, id }) = &snapshot.small {
        if !id.is_empty() {
            structured_entries.push(("small", provider.as_str(), id.as_str()));
        }
    }
    if let Some(ModelSpec::Provider { provider, id }) = &snapshot.large {
        if !id.is_empty() {
            structured_entries.push(("large", provider.as_str(), id.as_str()));
        }
    }

    // Fetch each named provider at most once, even when both slots name it.
    let mut fetched: HashMap<&str, Result<Vec<String>, String>> = HashMap::new();
    for &(slot, provider, id) in &structured_entries {
        let has_key = api_keys.get(provider).map(|v| !v.is_empty()).unwrap_or(false);
        if !has_key {
            messages.push(unconfigured_provider_message(slot, provider));
            continue;
        }
        if !fetched.contains_key(provider) {
            let key = &api_keys[provider];
            match crate::llm::fetch_available_models(provider, key).await {
                Ok(ids) => {
                    fetched.insert(provider, Ok(ids));
                }
                Err(e) => {
                    eprintln!("[validate_models] provider {provider} fetch failed: {e}");
                    fetched.insert(provider, Err(e));
                }
            }
        }
        match &fetched[provider] {
            Ok(list) => messages.extend(validate_structured_model(slot, provider, id, Some(list))),
            Err(_) => messages.extend(validate_structured_model(slot, provider, id, None)),
        }
    }

    // Legacy entries (plain strings): validate against the union of all
    // configured providers' model lists. The union fetch runs only when at
    // least one legacy slot holds a non-empty id.
    let legacy_small: String = match &snapshot.small {
        Some(ModelSpec::Id(s)) => s.clone(),
        _ => String::new(),
    };
    let legacy_large: String = match &snapshot.large {
        Some(ModelSpec::Id(s)) => s.clone(),
        _ => String::new(),
    };
    let union_needed = !legacy_small.is_empty() || !legacy_large.is_empty();

    let configured: Vec<(&String, &String)> = api_keys
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .collect();

    // Preserve the legacy early return: no provider to validate against and
    // no structured entry that gives another validation path.
    if configured.is_empty() && structured_entries.is_empty() {
        let message = "No configured providers available to validate models".to_string();
        eprintln!("[validate_models] {message}");
        return Err(message);
    }

    if union_needed && !configured.is_empty() {
        // Query every configured provider. Successful providers contribute
        // their model ids to the union; failed providers log to stderr only.
        let mut union: Vec<String> = Vec::new();
        let mut successes = 0usize;
        let mut failures: Vec<String> = Vec::new();
        for (provider, key) in configured {
            match crate::llm::fetch_available_models(provider, key).await {
                Ok(ids) => {
                    successes += 1;
                    union.extend(ids);
                }
                Err(e) => {
                    eprintln!("[validate_models] provider {provider} fetch failed: {e}");
                    failures.push(format!("provider {provider}: {e}"));
                }
            }
        }
        union.sort();
        union.dedup();

        if successes == 0 {
            // No provider answered at all. There is no validity signal, so
            // the failure details are the only useful content for the user.
            let mut message = "All configured providers failed to validate models".to_string();
            if !failures.is_empty() {
                message.push_str("; ");
                message.push_str(&failures.join("; "));
            }
            eprintln!("[validate_models] {message}");
            return Err(message);
        }

        let mut legacy_messages = validate_model_strings(&legacy_small, &legacy_large, &union);
        // A structured slot in a mixed config is validated against its named
        // provider only; it must never be reported as "not configured" by the
        // legacy path.
        if matches!(&snapshot.small, Some(ModelSpec::Provider { .. })) {
            legacy_messages.retain(|m| m != "No small model configured");
        }
        if matches!(&snapshot.large, Some(ModelSpec::Provider { .. })) {
            legacy_messages.retain(|m| m != "No large model configured");
        }
        messages.extend(legacy_messages);
    }

    if !messages.is_empty() {
        let joined = messages.join("; ");
        eprintln!("[validate_models] {joined}");
        return Err(joined);
    }

    Ok(ConfigStatus::from(&snapshot))
}