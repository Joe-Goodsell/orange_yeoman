// File watcher built on notify + notify-debouncer-full. Events are classified
// by a pure function and emitted on the "watcher://change" channel with a
// { path, kind } payload where kind is "structure" (create/remove/rename) or
// "content" (.md data modify). Changes to the active root's .orange-yeoman.json
// reload the merged config and emit "config://changed".

use crate::config::{reload_config_state, ConfigState, CONFIG_FILE_NAME};
use notify::event::{EventKind, ModifyKind};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager};

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

pub(crate) struct WatcherState {
    debouncer: Mutex<Option<Deb>>,
}

impl Default for WatcherState {
    fn default() -> Self {
        WatcherState {
            debouncer: Mutex::new(None),
        }
    }
}

// Classified outcome of a notify event relative to the watched root.
#[derive(Debug, PartialEq, Eq)]
enum ClassifiedChange {
    Structure,
    Content,
    Config,
}

// Classify a notify event against the watched root. Returns the first event
// path together with its classification, or None for irrelevant events.
// structure: create/remove/rename. content: .md data modify. config: the active
// root's own .orange-yeoman.json. Everything else (non-.md data modify,
// metadata, access, dotfile paths BELOW the root) is skipped. The watched
// root's own basename is exempt from the dotfile filter so dotfile-named roots
// like ~/.notes still work.
fn classify_event(root: &Path, event: &notify::Event) -> Option<(PathBuf, ClassifiedChange)> {
    let path = event.paths.first()?;

    // Repository config file: a dedicated classification. This check runs
    // BEFORE the dotfile filter because the file is hidden by design. Only the
    // active watched root's own config file is considered.
    if path == &root.join(CONFIG_FILE_NAME) {
        return Some((path.clone(), ClassifiedChange::Config));
    }

    // Dotfile filter: check only the path components BELOW the watched root.
    // This blocks .git/.obsidian noise while a root such as /Users/joe/.notes
    // (whose own basename starts with ".") still gets its events through.
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative
        .components()
        .any(|c| matches!(c, Component::Normal(seg) if seg.to_string_lossy().starts_with('.')))
    {
        return None;
    }

    let change = match &event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => Some(ClassifiedChange::Structure),
        EventKind::Modify(ModifyKind::Name(_)) => Some(ClassifiedChange::Structure),
        EventKind::Modify(ModifyKind::Data(_)) => {
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                Some(ClassifiedChange::Content)
            } else {
                None
            }
        }
        _ => None,
    };

    change.map(|change| (path.clone(), change))
}

// Classify a notify event and emit it if it is relevant. The config payload is
// ConfigStatus, which never contains API key values.
fn classify_and_emit(app: &tauri::AppHandle, root: &Path, event: &notify::Event) {
    let Some((path, change)) = classify_event(root, event) else {
        return;
    };
    match change {
        ClassifiedChange::Config => {
            let status = reload_config_state(&app.state::<ConfigState>(), Some(root));
            let _ = app.emit("config://changed", status);
        }
        ClassifiedChange::Structure => emit_watcher_change(app, &path, "structure"),
        ClassifiedChange::Content => emit_watcher_change(app, &path, "content"),
    }
}

fn emit_watcher_change(app: &tauri::AppHandle, path: &Path, kind: &str) {
    let _ = app.emit(
        "watcher://change",
        WatcherEvent {
            path: path.to_string_lossy().to_string(),
            kind: kind.to_string(),
        },
    );
}

#[tauri::command]
pub(crate) fn start_watcher(
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
pub(crate) fn stop_watcher(state: tauri::State<WatcherState>) -> Result<(), String> {
    *state.debouncer.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[cfg(test)]
mod tests;
