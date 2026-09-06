//! The JSON and in-memory config data structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// JSON shape of a config file (global or per-repository). API key values live
// only in Rust memory; ConfigStatus and Tauri events never carry them.
#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct ConfigFile {
    #[serde(default, rename = "apiKeys")]
    pub(crate) api_keys: HashMap<String, String>,
    #[serde(default)]
    pub(crate) models: ModelsConfig,
    // Absent in a config file means "not supplied": apply_file skips None so
    // an explicit false in the global config survives a partial project file.
    #[serde(default)]
    pub(crate) debug: Option<bool>,
    // Same "not supplied" sentinel as debug: an absent mockLlm key is skipped
    // by apply_file, so an explicit false in the global config survives a
    // partial project file.
    #[serde(default, rename = "mockLlm")]
    pub(crate) mock_llm: Option<bool>,
}

// One configured model. Accepts the structured form { provider, id } and,
// for backward compatibility, the legacy plain-string form (model id only,
// provider unresolved).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum ModelSpec {
    Id(String),
    Provider { provider: String, id: String },
}

impl ModelSpec {
    /// The model id, whichever form is used.
    pub(crate) fn id(&self) -> &str {
        match self {
            ModelSpec::Id(s) => s,
            ModelSpec::Provider { id, .. } => id,
        }
    }

    /// The named provider, when the structured form is used.
    pub(crate) fn provider(&self) -> Option<&str> {
        match self {
            ModelSpec::Id(_) => None,
            ModelSpec::Provider { provider, .. } => Some(provider),
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct ModelsConfig {
    // Absent = "not supplied": apply_file skips None so earlier merge
    // layers survive a partial models object. Models are never defaulted: an
    // unset model stays None until a config file supplies it.
    #[serde(default)]
    pub(crate) small: Option<ModelSpec>,
    #[serde(default)]
    pub(crate) large: Option<ModelSpec>,
}

// Safe, frontend-visible view of the merged config. Contains no API key values.
#[derive(Clone, Serialize)]
pub(crate) struct ConfigStatus {
    pub(crate) global_loaded: bool,
    pub(crate) project_loaded: bool,
    pub(crate) project_path: Option<String>,
    pub(crate) small_model: String,
    pub(crate) large_model: String,
    // None when the model came from the legacy plain-string form (provider
    // unresolved) or is not configured at all.
    pub(crate) small_provider: Option<String>,
    pub(crate) large_provider: Option<String>,
    pub(crate) configured_providers: Vec<String>,
    pub(crate) error: Option<String>,
    pub(crate) debug: bool,
    pub(crate) mock_llm: bool,
}

// In-memory merged config held in Tauri state. API keys never leave this struct.
pub(crate) struct MergedConfig {
    pub(crate) api_keys: HashMap<String, String>,
    pub(crate) small: Option<ModelSpec>,
    pub(crate) large: Option<ModelSpec>,
    pub(crate) global_loaded: bool,
    pub(crate) project_loaded: bool,
    pub(crate) project_path: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) debug: bool,
    pub(crate) mock_llm: bool,
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
