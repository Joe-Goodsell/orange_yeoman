// Orange Yeoman - Rust core. Filesystem access (list_dir, read_text_file), the
// dialog plugin (native folder picker), and the file watcher (notify +
// notify-debouncer-full) are wired here. The watcher emits Tauri events on the
// "watcher://change" channel with a { path, kind } payload where kind is
// "structure" (create/remove/rename) or "content" (.md data modify).

use notify::event::{EventKind, ModifyKind};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Serialize;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::Emitter;

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(WatcherState::default())
        .invoke_handler(tauri::generate_handler![
            list_dir,
            read_text_file,
            start_watcher,
            stop_watcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
