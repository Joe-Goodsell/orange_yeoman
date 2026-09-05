// File watcher built on notify + notify-debouncer-full. Events are classified
// by a pure function and emitted on the "watcher://change" channel with a
// { path, kind } payload where kind is "structure" (create/remove/rename) or
// "content" (.md data modify). Changes to the active root's .orange-yeoman.json
// reload the merged config and emit "config://changed".
//
// Content, create, remove, and rename events for .md paths additionally spawn
// the incremental block pipeline (parse -> diff -> apply) on a blocking task,
// so the debouncer callback itself stays fast. Processing is diff-based and
// idempotent: duplicate events for the same path converge to a no-op.

use crate::config::{reload_config_state, ConfigState, CONFIG_FILE_NAME};
use crate::store::{process_file_content, remove_all_blocks_for_path, relink_blocks, StoreState};
use notify::event::{EventKind, ModifyKind, RenameMode};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Serialize;
use std::fs;
use std::io::ErrorKind;
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

/// A classified outcome of a notify event relative to the watched root. Each
/// variant carries the path(s) the follow-up work needs. Rename carries both
/// ends when notify-debouncer-full matched the pair (paths[0] = from,
/// paths[1] = to); a rename whose sides it could not match arrives as a
/// single-path Structure.
#[derive(Debug, PartialEq, Eq)]
enum ClassifiedChange {
    Create(PathBuf),
    Remove(PathBuf),
    Structure(PathBuf),
    Content(PathBuf),
    Config,
    Rename { from: PathBuf, to: PathBuf },
}

// Classify a notify event against the watched root. Returns the classification
// for relevant events, or None for irrelevant ones.
// create/remove/rename: structure. .md data modify: content. the active root's
// own .orange-yeoman.json: config. Everything else (non-.md data modify,
// metadata, access, dotfile paths BELOW the root) is skipped. The watched
// root's own basename is exempt from the dotfile filter so dotfile-named roots
// like ~/.notes still work.
//
// Rename shapes (verified against notify-debouncer-full 0.7.0): it normalizes
// raw RenameMode::Any events into From/To/Both before the callback by an
// exists() check plus file-id/inode matching, so a both-sides rename on any
// platform reaches the Both branch with paths [from, to]. The single-path
// fallback below handles the sides it could not match.
fn classify_event(root: &Path, event: &notify::Event) -> Option<ClassifiedChange> {
    let path = event.paths.first()?;

    // Repository config file: a dedicated classification. This check runs
    // BEFORE the dotfile filter because the file is hidden by design. Only the
    // active watched root's own config file is considered.
    if path == &root.join(CONFIG_FILE_NAME) {
        return Some(ClassifiedChange::Config);
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
        EventKind::Create(_) => Some(ClassifiedChange::Create(path.clone())),
        EventKind::Remove(_) => Some(ClassifiedChange::Remove(path.clone())),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let from = event.paths.first()?;
            let to = event.paths.get(1)?;
            Some(ClassifiedChange::Rename {
                from: from.clone(),
                to: to.clone(),
            })
        }
        // The from side of a rename with no known target: treat as removal.
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            Some(ClassifiedChange::Remove(path.clone()))
        }
        // The to side of a rename with no known source: treat as creation.
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            Some(ClassifiedChange::Create(path.clone()))
        }
        // Single-path rename with no matched side: direction unknown, so the
        // fallback (drop stale rows, re-process) handles it.
        EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => {
            Some(ClassifiedChange::Structure(path.clone()))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Other)) => None,
        EventKind::Modify(ModifyKind::Data(_)) => {
            if is_markdown(path) {
                Some(ClassifiedChange::Content(path.clone()))
            } else {
                None
            }
        }
        _ => None,
    };

    change
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

// Classify a notify event, emit it, and spawn any follow-up processing. The
// config payload is ConfigStatus, which never contains API key values.
fn classify_and_emit(app: &tauri::AppHandle, root: &Path, event: &notify::Event) {
    let Some(change) = classify_event(root, event) else {
        return;
    };
    match &change {
        ClassifiedChange::Config => {
            let config_state = app.state::<ConfigState>();
            let status = reload_config_state(&config_state, Some(root));
            let llm_state = app.state::<crate::llm::LlmState>();
            llm_state.set_provider(crate::llm::select_provider(config_state.mock_llm()));
            let _ = app.emit("config://changed", status);
        }
        ClassifiedChange::Create(path) => {
            emit_watcher_change(app, path, "structure");
            if is_markdown(path) {
                spawn_content_processing(app, path.clone());
            }
        }
        ClassifiedChange::Remove(path) => {
            emit_watcher_change(app, path, "structure");
            if is_markdown(path) {
                spawn_removal(app, path.clone());
            }
        }
        ClassifiedChange::Structure(path) => {
            emit_watcher_change(app, path, "structure");
            if is_markdown(path) {
                spawn_rename_fallback(app, path.clone());
            }
        }
        ClassifiedChange::Rename { from, to } => {
            emit_watcher_change(app, from, "structure");
            if is_markdown(from) && is_markdown(to) {
                // Both ends known and both .md: re-link blocks, concepts stay.
                spawn_relink(app, from.clone(), to.clone());
            } else {
                if is_markdown(from) {
                    spawn_removal(app, from.clone());
                }
                if is_markdown(to) {
                    spawn_content_processing(app, to.clone());
                }
            }
        }
        ClassifiedChange::Content(path) => {
            emit_watcher_change(app, path, "content");
            spawn_content_processing(app, path.clone());
        }
    }
    // Debug instrumentation: report the classified change so the frontend can
    // show what the watcher reacted to. The category field keeps the existing
    // "watcher" UI category.
    let message = match change {
        ClassifiedChange::Config => format!("config changed: {}", root.display()),
        ClassifiedChange::Create(path) => format!("structure change (create): {}", path.display()),
        ClassifiedChange::Remove(path) => format!("structure change (remove): {}", path.display()),
        ClassifiedChange::Structure(path) => format!("structure change (rename): {}", path.display()),
        ClassifiedChange::Rename { from, to } => {
            format!("structure change (rename): {} -> {}", from.display(), to.display())
        }
        ClassifiedChange::Content(path) => format!("content change: {}", path.display()),
    };
    tracing::info!(category = "watcher", "{message}");
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

// --- pipeline spawning -----------------------------------------------------

fn spawn_content_processing(app: &tauri::AppHandle, path: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        process_content_change(&app, &path);
    });
}

fn spawn_removal(app: &tauri::AppHandle, path: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<StoreState>();
        let lock = store.path_lock(&path);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        store.with_conn(|conn| remove_all_blocks_for_path(conn, &path));
    });
}

// A rename whose target is known: point all stored blocks at the new path
// without re-extraction. Concepts and occurrences stay attached.
fn spawn_relink(app: &tauri::AppHandle, from: PathBuf, to: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<StoreState>();
        // Lock the target path only. A cross-rename pair (a->b and b->a) must
        // never hold two path locks at once, or the tasks could deadlock.
        let lock = store.path_lock(&to);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        store.with_conn(|conn| match relink_blocks(conn, &from, &to) {
            Ok(updated) => tracing::info!(
                category = "pipeline",
                "relinked {} -> {} ({updated} blocks)",
                from.display(),
                to.display()
            ),
            Err(e) => tracing::error!(category = "store", "relink failed: {e}"),
        });
    });
}

// A single-path rename that notify-debouncer-full could not match to a side:
// the carried path may be either the old or the new name, so the safest
// fallback is to drop stale rows for it and then re-process the content.
// Diff-based processing makes the second step a no-op for unchanged blocks and
// idempotent across duplicate events.
fn spawn_rename_fallback(app: &tauri::AppHandle, path: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<StoreState>();
        let lock = store.path_lock(&path);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        store.with_conn(|conn| remove_all_blocks_for_path(conn, &path));
        process_path_locked(&store, &path);
    });
}

/// Thin AppHandle wrapper over the testable processing core. Serializes
/// same-file tasks through the per-path lock so concurrent tasks converge to
/// the latest content, reads the file under that lock, then runs
/// parse/diff/apply and logs a pipeline summary.
fn process_content_change(app: &tauri::AppHandle, path: &Path) {
    let store = app.state::<StoreState>();
    let lock = store.path_lock(path);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    process_path_locked(&store, path);
}

/// Read the file and process it. The caller holds the per-path lock. A missing
/// file means the path was removed (or renamed away), so all stored blocks for
/// it are dropped. Other read errors are logged and skipped.
fn process_path_locked(store: &StoreState, path: &Path) {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            store.with_conn(|conn| remove_all_blocks_for_path(conn, path));
            return;
        }
        Err(e) => {
            tracing::warn!(
                category = "pipeline",
                "cannot read {}: {e}",
                path.display()
            );
            return;
        }
    };
    let outcome = store.with_conn(|conn| process_file_content(conn, path, &content));
    let Some((diff, stats)) = outcome else {
        // Store unavailable; the file content is still readable but there is
        // nowhere to persist blocks.
        return;
    };
    tracing::info!(
        category = "pipeline",
        "processed {}: unchanged={} changed={} added={} moved={} removed={} stored={} enqueued={}",
        path.display(),
        diff.unchanged,
        diff.changed,
        diff.added,
        diff.moved,
        diff.removed,
        stats.stored,
        stats.enqueued
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
    Ok(())
}

#[cfg(test)]
mod tests;
