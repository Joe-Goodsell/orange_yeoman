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

#[derive(Clone, Deserialize, Serialize)]
struct ModelsConfig {
    // Field-level defaults are empty strings. An absent field is a "not
    // supplied" sentinel that apply_file skips, so earlier merge layers
    // survive a partial models object. Models are never defaulted: an unset
    // model stays empty until a config file supplies it.
    #[serde(default)]
    small: String,
    #[serde(default)]
    large: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        ModelsConfig {
            small: String::new(),
            large: String::new(),
        }
    }
}

// Safe, frontend-visible view of the merged config. Contains no API key values.
#[derive(Clone, Serialize)]
pub(crate) struct ConfigStatus {
    global_loaded: bool,
    project_loaded: bool,
    project_path: Option<String>,
    small_model: String,
    large_model: String,
    configured_providers: Vec<String>,
    error: Option<String>,
    debug: bool,
    mock_llm: bool,
}

// In-memory merged config held in Tauri state. API keys never leave this struct.
struct MergedConfig {
    api_keys: HashMap<String, String>,
    small_model: String,
    large_model: String,
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
            small_model: String::new(),
            large_model: String::new(),
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
            small_model: m.small_model.clone(),
            large_model: m.large_model.clone(),
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
    /// Current small model id from the merged config.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn small_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| guard.small_model.clone())
            .unwrap_or_else(|_| String::new())
    }

    /// Current large model id from the merged config.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn large_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| guard.large_model.clone())
            .unwrap_or_else(|_| String::new())
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

fn global_config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library/Application Support/Orange Yeoman/config.json"))
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
// cleared by omission. Empty model names are ignored so earlier merge layers
// survive.
fn apply_file(merged: &mut MergedConfig, file: &ConfigFile) {
    for (provider, value) in &file.api_keys {
        merged.api_keys.insert(provider.clone(), value.clone());
    }
    if !file.models.small.is_empty() {
        merged.small_model = file.models.small.clone();
    }
    if !file.models.large.is_empty() {
        merged.large_model = file.models.large.clone();
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

// Validate the configured small and large model strings against each
// configured provider's /models endpoint. The config is snapshotted and the
// lock is dropped before any network I/O. Returns the current status when
// both models are valid; otherwise returns a user-facing error string. Each
// provider fetch failure is logged to stderr for developers only; the user
// message stays focused on model validity.
#[tauri::command]
pub(crate) async fn validate_models(
    state: tauri::State<'_, ConfigState>,
) -> Result<ConfigStatus, String> {
    // Snapshot the config under the lock, then drop the guard before network
    // I/O. A poisoned lock is a hard error here: validation needs real values.
    let (api_keys, small, large, snapshot) = {
        let guard = match state.inner.lock() {
            Ok(guard) => guard,
            Err(_) => return Err("config state lock is poisoned".to_string()),
        };
        let snapshot = MergedConfig {
            api_keys: guard.api_keys.clone(),
            small_model: guard.small_model.clone(),
            large_model: guard.large_model.clone(),
            global_loaded: guard.global_loaded,
            project_loaded: guard.project_loaded,
            project_path: guard.project_path.clone(),
            error: guard.error.clone(),
            debug: guard.debug,
            mock_llm: guard.mock_llm,
        };
        (
            guard.api_keys.clone(),
            guard.small_model.clone(),
            guard.large_model.clone(),
            snapshot,
        )
    };

    let configured: Vec<(&String, &String)> = api_keys
        .iter()
        .filter(|(_, value)| !value.is_empty())
        .collect();

    if configured.is_empty() {
        let message = "No configured providers available to validate models".to_string();
        eprintln!("[validate_models] {message}");
        return Err(message);
    }

    // Query every configured provider. Successful providers contribute their
    // model ids to the union; failed providers log to stderr only.
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
        // No provider answered at all. There is no validity signal, so the
        // failure details are the only useful content for the user.
        let mut message = "All configured providers failed to validate models".to_string();
        if !failures.is_empty() {
            message.push_str("; ");
            message.push_str(&failures.join("; "));
        }
        eprintln!("[validate_models] {message}");
        return Err(message);
    }

    let messages = validate_model_strings(&small, &large, &union);
    if !messages.is_empty() {
        let joined = messages.join("; ");
        eprintln!("[validate_models] {joined}");
        return Err(joined);
    }

    Ok(ConfigStatus::from(&snapshot))
}

#[cfg(test)]
mod tests;
