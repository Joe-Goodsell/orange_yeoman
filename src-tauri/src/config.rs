// Layered JSON configuration. A global config file plus an optional
// per-repository .orange-yeoman.json are merged into an in-memory MergedConfig.
// API key values live only in Rust memory: the frontend-visible ConfigStatus and
// the "config://changed" Tauri event never carry them.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub(crate) const CONFIG_FILE_NAME: &str = ".orange-yeoman.json";

// JSON shape of a config file (global or per-repository). API key values live
// only in Rust memory; ConfigStatus and Tauri events never carry them.
#[derive(Clone, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default, rename = "apiKeys")]
    api_keys: HashMap<String, String>,
    #[serde(default)]
    models: ModelsConfig,
    // Absent in a config file means "not supplied": apply_file skips None so
    // an explicit false in the global config survives a partial project file.
    #[serde(default)]
    debug: Option<bool>,
    // Same "not supplied" sentinel as debug: an absent mockLlm key is skipped
    // by apply_file, so an explicit false in the global config survives a
    // partial project file.
    #[serde(default, rename = "mockLlm")]
    mock_llm: Option<bool>,
}

// One configured model. Accepts the structured form { provider, id } and,
// for backward compatibility, the legacy plain-string form (model id only,
// provider unresolved).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
enum ModelSpec {
    Id(String),
    Provider { provider: String, id: String },
}

impl ModelSpec {
    /// The model id, whichever form is used.
    fn id(&self) -> &str {
        match self {
            ModelSpec::Id(s) => s,
            ModelSpec::Provider { id, .. } => id,
        }
    }

    /// The named provider, when the structured form is used.
    fn provider(&self) -> Option<&str> {
        match self {
            ModelSpec::Id(_) => None,
            ModelSpec::Provider { provider, .. } => Some(provider),
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct ModelsConfig {
    // Absent = "not supplied": apply_file skips None so earlier merge
    // layers survive a partial models object. Models are never defaulted: an
    // unset model stays None until a config file supplies it.
    #[serde(default)]
    small: Option<ModelSpec>,
    #[serde(default)]
    large: Option<ModelSpec>,
}

// Safe, frontend-visible view of the merged config. Contains no API key values.
#[derive(Clone, Serialize)]
pub(crate) struct ConfigStatus {
    global_loaded: bool,
    project_loaded: bool,
    project_path: Option<String>,
    small_model: String,
    large_model: String,
    // None when the model came from the legacy plain-string form (provider
    // unresolved) or is not configured at all.
    small_provider: Option<String>,
    large_provider: Option<String>,
    configured_providers: Vec<String>,
    error: Option<String>,
    debug: bool,
    mock_llm: bool,
}

// In-memory merged config held in Tauri state. API keys never leave this struct.
struct MergedConfig {
    api_keys: HashMap<String, String>,
    small: Option<ModelSpec>,
    large: Option<ModelSpec>,
    global_loaded: bool,
    project_loaded: bool,
    project_path: Option<String>,
    error: Option<String>,
    debug: bool,
    mock_llm: bool,
}

impl Default for MergedConfig {
    fn default() -> Self {
        MergedConfig {
            api_keys: HashMap::new(),
            small: None,
            large: None,
            global_loaded: false,
            project_loaded: false,
            project_path: None,
            error: None,
            debug: true,
            mock_llm: true,
        }
    }
}

impl From<&MergedConfig> for ConfigStatus {
    fn from(m: &MergedConfig) -> Self {
        let mut configured_providers: Vec<String> = m
            .api_keys
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(provider, _)| provider.clone())
            .collect();
        configured_providers.sort();
        ConfigStatus {
            global_loaded: m.global_loaded,
            project_loaded: m.project_loaded,
            project_path: m.project_path.clone(),
            small_model: m
                .small
                .as_ref()
                .map(|spec| spec.id().to_string())
                .unwrap_or_default(),
            large_model: m
                .large
                .as_ref()
                .map(|spec| spec.id().to_string())
                .unwrap_or_default(),
            small_provider: m.small.as_ref().and_then(|s| s.provider().map(str::to_string)),
            large_provider: m.large.as_ref().and_then(|s| s.provider().map(str::to_string)),
            configured_providers,
            error: m.error.clone(),
            debug: m.debug,
            mock_llm: m.mock_llm,
        }
    }
}

pub(crate) struct ConfigState {
    inner: Mutex<MergedConfig>,
}

impl ConfigState {
    /// Current small model id from the merged config. The resolved id is
    /// returned for both the structured and the legacy form.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn small_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| {
                guard
                    .small
                    .as_ref()
                    .map(|spec| spec.id().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| String::new())
    }

    /// Current large model id from the merged config. The resolved id is
    /// returned for both the structured and the legacy form.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn large_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| {
                guard
                    .large
                    .as_ref()
                    .map(|spec| spec.id().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| String::new())
    }

    /// Provider named by the current small model, when the structured form is
    /// used. None for the legacy plain-string form and for an unset slot.
    /// Falls back to None when the lock is poisoned.
    pub(crate) fn small_provider(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.small.as_ref().and_then(|s| s.provider().map(str::to_string)))
    }

    /// Provider named by the current large model, when the structured form is
    /// used. None for the legacy plain-string form and for an unset slot.
    /// Falls back to None when the lock is poisoned.
    pub(crate) fn large_provider(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.large.as_ref().and_then(|s| s.provider().map(str::to_string)))
    }

    /// Current debug flag from the merged config. Debug events are emitted
    /// only when this is true. Recovers the stored value when the lock is
    /// poisoned instead of failing open.
    pub(crate) fn debug(&self) -> bool {
        self.inner
            .lock()
            .map(|m| m.debug)
            .unwrap_or_else(|e| e.into_inner().debug)
    }

    /// Current mock_llm flag from the merged config. Recovers the stored
    /// value when the lock is poisoned instead of failing open.
    pub(crate) fn mock_llm(&self) -> bool {
        self.inner
            .lock()
            .map(|m| m.mock_llm)
            .unwrap_or_else(|e| e.into_inner().mock_llm)
    }
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState {
            inner: Mutex::new(MergedConfig::default()),
        }
    }
}

// Resolve the app's data directory. macOS: ~/Library/Application Support/Orange
// Yeoman. The global config file and the concept store DB share this directory.
pub(crate) fn app_data_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library/Application Support/Orange Yeoman"))
}

fn global_config_path() -> Option<PathBuf> {
    app_data_dir().map(|dir| dir.join("config.json"))
}

// Sanitize a JSON parse error into a safe message. Raw serde error text can
// embed the offending JSON value (for example an apiKeys string such as
// "sk-live-secret"), so it must never reach ConfigStatus or the frontend. Only
// a generic reason and the position survive.
fn sanitize_parse_error(path: &Path, e: &serde_json::Error) -> String {
    format!(
        "invalid config JSON at {}: line {}, column {}",
        path.display(),
        e.line(),
        e.column()
    )
}

// Read and parse a config file. Missing files are valid (None). Invalid JSON is
// an error. All parse errors are sanitized before returning, so no API key
// material or raw serde text can leak into the merged config status.
fn read_config_file(path: &Path) -> Result<Option<ConfigFile>, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| sanitize_parse_error(path, &e)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read config {}: {e}", path.display())),
    }
}

// Apply one config file onto the merged config. Later files override earlier
// ones by key. API keys are inserted or replaced per provider and are never
// cleared by omission. Model entries are skipped when absent (None) or when
// their resolved id is empty, so earlier merge layers survive a partial
// models object.
fn apply_file(merged: &mut MergedConfig, file: &ConfigFile) {
    for (provider, value) in &file.api_keys {
        merged.api_keys.insert(provider.clone(), value.clone());
    }
    if let Some(small) = &file.models.small {
        if !small.id().is_empty() {
            merged.small = Some(small.clone());
        }
    }
    if let Some(large) = &file.models.large {
        if !large.id().is_empty() {
            merged.large = Some(large.clone());
        }
    }
    if let Some(debug) = file.debug {
        merged.debug = debug;
    }
    if let Some(mock_llm) = file.mock_llm {
        merged.mock_llm = mock_llm;
    }
}

// Compute the merged config: empty defaults, then the global config, then
// the repository config. Errors are collected into the status; loading always
// continues with the last valid/default values.
fn compute_merged(project_root: Option<&Path>) -> MergedConfig {
    let mut merged = MergedConfig::default();
    let mut errors: Vec<String> = Vec::new();

    match global_config_path() {
        Some(path) => match read_config_file(&path) {
            Ok(Some(file)) => {
                apply_file(&mut merged, &file);
                merged.global_loaded = true;
            }
            Ok(None) => {}
            Err(e) => errors.push(format!("global config: {e}")),
        },
        None => errors.push("cannot resolve home directory for global config".to_string()),
    }

    if let Some(root) = project_root {
        let path = root.join(CONFIG_FILE_NAME);
        merged.project_path = Some(root.to_string_lossy().to_string());
        match read_config_file(&path) {
            Ok(Some(file)) => {
                apply_file(&mut merged, &file);
                merged.project_loaded = true;
            }
            Ok(None) => {}
            Err(e) => errors.push(format!("project config: {e}")),
        }
    }

    merged.error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };
    merged
}

// Reload the merged config into state and return the safe status view. File
// I/O happens before the lock is taken, so the lock is held only for the final
// write.
pub(crate) fn reload_config_state(
    state: &ConfigState,
    project_root: Option<&Path>,
) -> ConfigStatus {
    let merged = compute_merged(project_root);
    let status = ConfigStatus::from(&merged);
    if let Ok(mut guard) = state.inner.lock() {
        *guard = merged;
    }
    status
}

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

// Validate two configured model strings against the available-models union
// fetched from a provider. Returns one message per problem; an empty result
// means both models are valid. The available list is sorted and comma-joined
// in each message. This function is pure so the message format is testable
// without network I/O.
fn validate_model_strings(small: &str, large: &str, available: &[String]) -> Vec<String> {
    let mut sorted: Vec<&str> = available.iter().map(String::as_str).collect();
    sorted.sort();
    sorted.dedup();
    let list = if sorted.is_empty() {
        "(none)".to_string()
    } else {
        sorted.join(", ")
    };

    let mut messages = Vec::new();
    if small.is_empty() {
        messages.push("No small model configured".to_string());
    } else if !sorted.contains(&small) {
        messages.push(format!("Invalid model {small}. Available models: {list}"));
    }
    if large.is_empty() {
        messages.push("No large model configured".to_string());
    } else if !sorted.contains(&large) {
        messages.push(format!("Invalid model {large}. Available models: {list}"));
    }
    messages
}

// Build the user-facing messages for one structured { provider, id } entry.
// `available` is the provider's fetched model list: None when the list is
// unavailable (fetch failed), Some(list) when the fetch succeeded. This
// function is pure so the message format is testable without network I/O.
fn validate_structured_model(
    _slot: &str,
    provider: &str,
    id: &str,
    available: Option<&[String]>,
) -> Vec<String> {
    match available {
        None => vec![format!(
            "could not check model {id}: provider {provider} did not respond"
        )],
        Some(list) => {
            let mut sorted: Vec<&str> = list.iter().map(String::as_str).collect();
            sorted.sort();
            sorted.dedup();
            let joined = if sorted.is_empty() {
                "(none)".to_string()
            } else {
                sorted.join(", ")
            };
            if sorted.contains(&id) {
                Vec::new()
            } else {
                vec![format!(
                    "Invalid model {id} for provider {provider}. Available models: {joined}"
                )]
            }
        }
    }
}

// Message for a structured entry whose named provider has no API key
// configured. The slot (small/large) is labeled so the message identifies
// which model slot the problem belongs to.
fn unconfigured_provider_message(slot: &str, provider: &str) -> String {
    format!("provider {provider} has no API key configured for the {slot} model")
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

#[cfg(test)]
mod tests;
