//! Reading, merging, and reloading the layered JSON configuration.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::model::{ConfigFile, ConfigStatus, MergedConfig};
use super::state::ConfigState;

pub(crate) const CONFIG_FILE_NAME: &str = ".orange-yeoman.json";

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
pub(crate) fn read_config_file(path: &Path) -> Result<Option<ConfigFile>, String> {
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
pub(crate) fn apply_file(merged: &mut MergedConfig, file: &ConfigFile) {
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
