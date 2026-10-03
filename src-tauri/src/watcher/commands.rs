//! Tauri commands and the shared watcher state.

use super::snapshots::BlockSnapshot;
use super::sync::classify_and_emit;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

// The debounced watcher type. RecommendedCache is the platform default file ID
// cache (FileIdMap on macOS). The debouncer is Send + Sync, so it can live in
// Tauri state.
type Deb = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::RecommendedCache,
>;

pub(crate) struct WatcherState {
    debouncer: Mutex<Option<Deb>>,
    // Block snapshots per watched file, used to emit block-level diff events
    // for content changes when the debug config flag is on.
    pub(crate) block_cache: Mutex<HashMap<PathBuf, Vec<BlockSnapshot>>>,
}

impl Default for WatcherState {
    fn default() -> Self {
        WatcherState {
            debouncer: Mutex::new(None),
            block_cache: Mutex::new(HashMap::new()),
        }
    }
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

    // A new watch session starts from a fresh baseline: drop block snapshots
    // cached by the previous session so the first content change does not diff
    // against a pre-stop snapshot.
    state
        .block_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();

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
                tracing::error!(category = "watcher", "debounce errors: {:?}", errors);
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
    // A new watch session starts from a fresh baseline: drop block snapshots
    // cached by the stopped session.
    state
        .block_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    Ok(())
}