// Layered JSON configuration. A global config file plus an optional
// per-repository .orange-yeoman.json are merged into an in-memory MergedConfig.
// API key values live only in Rust memory: the frontend-visible ConfigStatus and
// the "config://changed" Tauri event never carry them.

mod commands;
mod load;
mod model;
mod state;
mod validate;

pub(crate) use commands::{
    get_config_status, load_project_config, validate_models,
    __cmd__get_config_status, __cmd__load_project_config, __cmd__validate_models,
    __tauri_command_name_get_config_status, __tauri_command_name_load_project_config,
    __tauri_command_name_validate_models,
};
pub(crate) use load::{app_data_dir, reload_config_state, CONFIG_FILE_NAME};
pub(crate) use state::ConfigState;

// Test-only re-exports. The sibling submodules import their items directly;
// the root re-exports these solely so config/tests.rs can keep `use super::*;`.
#[cfg(test)]
pub(crate) use load::{apply_file, read_config_file};
#[cfg(test)]
pub(crate) use model::{ConfigFile, ConfigStatus, MergedConfig, ModelSpec};
#[cfg(test)]
pub(crate) use validate::{
    unconfigured_provider_message, validate_model_strings, validate_structured_model,
};

#[cfg(test)]
mod tests;