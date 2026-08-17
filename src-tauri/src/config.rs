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

const DEFAULT_SMALL_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_LARGE_MODEL: &str = "openai-codex-5.6";
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
}

#[derive(Clone, Deserialize, Serialize)]
struct ModelsConfig {
    // Field-level defaults are empty strings. An absent field is a "not
    // supplied" sentinel that apply_file skips, so built-in defaults and
    // earlier merge layers survive a partial models object.
    #[serde(default)]
    small: String,
    #[serde(default)]
    large: String,
}

impl Default for ModelsConfig {
    fn default() -> Self {
        ModelsConfig {
            small: DEFAULT_SMALL_MODEL.to_string(),
            large: DEFAULT_LARGE_MODEL.to_string(),
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
}

impl Default for MergedConfig {
    fn default() -> Self {
        MergedConfig {
            api_keys: HashMap::new(),
            small_model: DEFAULT_SMALL_MODEL.to_string(),
            large_model: DEFAULT_LARGE_MODEL.to_string(),
            global_loaded: false,
            project_loaded: false,
            project_path: None,
            error: None,
            debug: true,
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
        }
    }
}

pub(crate) struct ConfigState {
    inner: Mutex<MergedConfig>,
}

impl ConfigState {
    /// Current small model id from the merged config.
    /// Falls back to the built-in default when the lock is poisoned.
    pub(crate) fn small_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| guard.small_model.clone())
            .unwrap_or_else(|_| DEFAULT_SMALL_MODEL.to_string())
    }

    /// Current large model id from the merged config.
    /// Falls back to the built-in default when the lock is poisoned.
    pub(crate) fn large_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| guard.large_model.clone())
            .unwrap_or_else(|_| DEFAULT_LARGE_MODEL.to_string())
    }

    /// Current debug flag from the merged config. Debug events are emitted
    /// only when this is true. Falls back to true (enabled) when the lock is
    /// poisoned.
    pub(crate) fn debug(&self) -> bool {
        self.inner.lock().map(|m| m.debug).unwrap_or(true)
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
// cleared by omission. Empty model names are ignored so built-in defaults
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
}

// Compute the merged config: built-in defaults, then the global config, then
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
#[tauri::command]
pub(crate) fn load_project_config(
    root: Option<String>,
    state: tauri::State<ConfigState>,
) -> Result<ConfigStatus, String> {
    let root_path = root.as_deref().map(PathBuf::from);
    Ok(reload_config_state(&state, root_path.as_deref()))
}

#[cfg(test)]
mod tests;
