// Orange Yeoman - Rust core. Filesystem access (list_dir, read_text_file), the
// dialog plugin (native folder picker), and the file watcher (notify +
// notify-debouncer-full) are wired here. The watcher emits Tauri events on the
// "watcher://change" channel with a { path, kind } payload where kind is
// "structure" (create/remove/rename) or "content" (.md data modify).

use notify::event::{EventKind, ModifyKind};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager};

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

const DEFAULT_SMALL_MODEL: &str = "deepseek-v4-flash";
const DEFAULT_LARGE_MODEL: &str = "openai-codex-5.6";
const CONFIG_FILE_NAME: &str = ".orange-yeoman.json";

// JSON shape of a config file (global or per-repository). API key values live
// only in Rust memory; ConfigStatus and Tauri events never carry them.
#[derive(Clone, Default, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(default, rename = "apiKeys")]
    api_keys: HashMap<String, String>,
    #[serde(default)]
    models: ModelsConfig,
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
struct ConfigStatus {
    global_loaded: bool,
    project_loaded: bool,
    project_path: Option<String>,
    small_model: String,
    large_model: String,
    configured_providers: Vec<String>,
    error: Option<String>,
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
        }
    }
}

struct ConfigState {
    inner: Mutex<MergedConfig>,
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
fn reload_config_state(state: &ConfigState, project_root: Option<&Path>) -> ConfigStatus {
    let merged = compute_merged(project_root);
    let status = ConfigStatus::from(&merged);
    if let Ok(mut guard) = state.inner.lock() {
        *guard = merged;
    }
    status
}

#[tauri::command]
fn list_dir(dir: String) -> Result<Vec<FileEntry>, String> {
    let mut entries: Vec<FileEntry> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some(FileEntry { name, path, is_dir })
        })
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[derive(Serialize, Clone)]
struct WatcherEvent {
    path: String,
    kind: String,
}

// The debounced watcher type. RecommendedCache is the platform default file ID
// cache (FileIdMap on macOS). The debouncer is Send + Sync, so it can live in
// Tauri state.
type Deb = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::RecommendedCache,
>;

struct WatcherState {
    debouncer: Mutex<Option<Deb>>,
}

impl Default for WatcherState {
    fn default() -> Self {
        WatcherState {
            debouncer: Mutex::new(None),
        }
    }
}

// Classify a notify event and emit it on "watcher://change" if it is relevant.
// structure: create/remove/rename. content: .md data modify. Everything else
// (non-.md data modify, metadata, access, dotfile paths BELOW the root) is
// skipped. The watched root's own basename is exempt from the dotfile filter
// so dotfile-named roots like ~/.notes still work.
fn classify_and_emit(app: &tauri::AppHandle, root: &Path, event: &notify::Event) {
    let Some(path) = event.paths.first() else {
        return;
    };

    // Repository config file: reload the merged config and emit a dedicated
    // event. This check runs BEFORE the dotfile filter because the file is
    // hidden by design. The payload is ConfigStatus, which never contains API
    // key values. Only the active watched root's own config file is considered.
    if path == &root.join(CONFIG_FILE_NAME) {
        let status = reload_config_state(&app.state::<ConfigState>(), Some(root));
        let _ = app.emit("config://changed", status);
        return;
    }

    // Dotfile filter: check only the path components BELOW the watched root.
    // This blocks .git/.obsidian noise while a root such as /Users/joe/.notes
    // (whose own basename starts with ".") still gets its events through.
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative
        .components()
        .any(|c| matches!(c, Component::Normal(seg) if seg.to_string_lossy().starts_with('.')))
    {
        return;
    }

    let kind = match &event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => Some("structure"),
        EventKind::Modify(ModifyKind::Name(_)) => Some("structure"),
        EventKind::Modify(ModifyKind::Data(_)) => {
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                Some("content")
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(kind) = kind {
        let _ = app.emit(
            "watcher://change",
            WatcherEvent {
                path: path.to_string_lossy().to_string(),
                kind: kind.to_string(),
            },
        );
    }
}

#[tauri::command]
fn start_watcher(
    root: String,
    app: tauri::AppHandle,
    state: tauri::State<WatcherState>,
) -> Result<(), String> {
    // Stop any existing watcher first. Dropping the old debouncer stops its fs watch.
    {
        *state.debouncer.lock().map_err(|e| e.to_string())? = None;
    }

    let root_path = PathBuf::from(&root);
    let meta = fs::metadata(&root_path).map_err(|e| format!("invalid root {root}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{root} is not a directory"));
    }

    let app_for_callback = app.clone();
    let watch_root = root_path.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                for debounced in events {
                    classify_and_emit(&app_for_callback, &watch_root, &debounced.event);
                }
            }
            Err(errors) => {
                eprintln!("watcher debounce errors: {errors:?}");
            }
        },
    )
    .map_err(|e| format!("failed to create debouncer: {e}"))?;

    debouncer
        .watch(&root_path, RecursiveMode::Recursive)
        .map_err(|e| format!("failed to watch {root}: {e}"))?;

    *state.debouncer.lock().map_err(|e| e.to_string())? = Some(debouncer);

    Ok(())
}

#[tauri::command]
fn stop_watcher(state: tauri::State<WatcherState>) -> Result<(), String> {
    *state.debouncer.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[tauri::command]
fn get_config_status(state: tauri::State<ConfigState>) -> ConfigStatus {
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
fn load_project_config(
    root: Option<String>,
    state: tauri::State<ConfigState>,
) -> Result<ConfigStatus, String> {
    let root_path = root.as_deref().map(PathBuf::from);
    Ok(reload_config_state(&state, root_path.as_deref()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(WatcherState::default())
        .manage(ConfigState::default())
        .setup(|app| {
            // Initialize the merged config with built-in defaults plus the
            // global config. No config file is created; missing files are valid.
            // Any error is stored in status and the app continues.
            reload_config_state(&app.state::<ConfigState>(), None);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_dir,
            read_text_file,
            start_watcher,
            stop_watcher,
            get_config_status,
            load_project_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
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
        assert!(!err.contains("invalid type"), "raw serde text leaked: {err}");
        assert!(err.contains("invalid config JSON"), "missing generic reason: {err}");
        assert!(err.contains("line 1"), "missing line info: {err}");
    }

    // A partial models object must parse, and each missing field must fall
    // back to the built-in default without clobbering earlier layers.
    #[test]
    fn partial_models_object_uses_builtin_defaults_for_missing_fields() {
        let global: ConfigFile =
            serde_json::from_str(r#"{"models": {"small": "global-small"}}"#)
                .expect("partial global models must parse");
        let mut merged = MergedConfig::default();
        apply_file(&mut merged, &global);
        assert_eq!(merged.small_model, "global-small");
        assert_eq!(merged.large_model, DEFAULT_LARGE_MODEL);

        let repo: ConfigFile =
            serde_json::from_str(r#"{"models": {"large": "repo-large"}}"#)
                .expect("partial repo models must parse");
        apply_file(&mut merged, &repo);
        assert_eq!(merged.small_model, "global-small");
        assert_eq!(merged.large_model, "repo-large");
    }
}
