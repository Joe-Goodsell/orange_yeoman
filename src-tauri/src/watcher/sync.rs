//! Async pipeline spawning: debouncer callback orchestration and the spawn_*
//! helpers that hand processing to the blocking pipeline pool.

use crate::config::{reload_config_state, ConfigState};
use crate::store::{process_file_content, relink_blocks, remove_all_blocks_for_path, StoreState};
use super::classify::{classify_event, content_hash, is_markdown, ClassifiedChange};
use super::commands::WatcherState;
use super::events::{emit_block_diffs, emit_watcher_change};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

// Classify a notify event and emit it if it is relevant. The config payload is
// ConfigStatus, which never contains API key values.
pub(crate) fn classify_and_emit(app: &tauri::AppHandle, root: &Path, event: &notify::Event) {
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
            // Debug instrumentation: report the reload and the merged debug
            // flag, read AFTER the reload so the event reflects new state.
            tracing::info!(
                category = "config",
                "config reloaded: {} (debug = {})",
                root.display(),
                config_state.debug()
            );
            let event = crate::events::BackendEvent {
                id: crate::events::next_event_id("config_reload"),
                event_type: crate::events::EventType::ConfigReload,
                status: crate::events::EventStatus::Done,
                created_at: crate::events::now_ms(),
                ts: crate::events::now_ms(),
                duration_ms: None,
                model: None,
                estimated_cost_usd: None,
                final_cost_usd: None,
                block: None,
                summary: "project config reloaded".to_string(),
                detail: Some(serde_json::json!({
                    "path": root.join(crate::config::CONFIG_FILE_NAME).to_string_lossy().to_string()
                })),
            };
            crate::events::emit_backend_event(app, &event);
        }
        ClassifiedChange::Structure(path) => {
            emit_watcher_change(app, &path, "structure", None);
            // Any structure event (create/remove/rename) invalidates the block
            // cache entry: the cached snapshot no longer matches the file.
            app.state::<WatcherState>()
                .block_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(path);
            tracing::info!(category = "watcher", "structure change: {}", path.display());
            emit_watcher_backend_event(app, &path, "structure", None, None);
            // A rename whose sides notify-debouncer-full could not match: the
            // carried path may be the old or the new name, so drop stale rows
            // for it and then re-process the content.
            spawn_rename_fallback(app, path.clone());
        }
        ClassifiedChange::Content(path) => {
            // The hash stamps the new on-disk content so the frontend can
            // recognize this editor's own writes. emit_block_diffs reads the
            // file again and already handles transient failures; a failed hash
            // read just omits the hash.
            let hash = content_hash(path);
            emit_watcher_change(app, &path, "content", hash.clone());
            tracing::info!(category = "watcher", "content change: {}", path.display());
            // The file-level summary event is emitted last, so the newest-first
            // feed renders it first (on top); the block-level diffs emitted just
            // before it fall below the summary card.
            emit_block_diffs(app, &path);
            emit_watcher_backend_event(app, &path, "content", hash, None);
            // Ingest the disk change into the concept store; the pipeline
            // reads disk state, so the store must track it.
            spawn_content_processing(app, path.clone());
        }
        ClassifiedChange::Create(path) => {
            emit_watcher_change(app, &path, "structure", None);
            // The cached snapshot no longer matches the file: invalidate it.
            app.state::<WatcherState>()
                .block_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(path);
            tracing::info!(category = "watcher", "created: {}", path.display());
            emit_watcher_backend_event(app, &path, "create", None, None);
            if is_markdown(path) {
                spawn_content_processing(app, path.clone());
            }
        }
        ClassifiedChange::Remove(path) => {
            emit_watcher_change(app, &path, "structure", None);
            // The cached snapshot no longer matches the file: invalidate it.
            app.state::<WatcherState>()
                .block_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(path);
            tracing::info!(category = "watcher", "removed: {}", path.display());
            emit_watcher_backend_event(app, &path, "remove", None, None);
            spawn_removal(app, path.clone());
        }
        ClassifiedChange::Rename { from, to } => {
            // The `to` path is the live one, so the structure event names it;
            // the tree mirrors on-disk structure. Both cache entries are
            // invalidated: the old path's snapshot is gone and the new path
            // starts from a fresh baseline.
            emit_watcher_change(app, &to, "structure", None);
            let state = app.state::<WatcherState>();
            let mut cache = state.block_cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.remove(from);
            cache.remove(to);
            drop(cache);
            tracing::info!(
                category = "watcher",
                "renamed: {} -> {}",
                from.display(),
                to.display()
            );
            emit_watcher_backend_event(app, &to, "rename", None, Some(from));
            spawn_relink(app, from.clone(), to.clone());
        }
    }
}

// --- pipeline spawning -----------------------------------------------------

// Emit one "backend://event" card for a file-level watcher change. The path
// and kind go in detail as strings; a rename additionally carries the old
// path. The hash is present only when the caller read the new content.
fn emit_watcher_backend_event(
    app: &tauri::AppHandle,
    path: &Path,
    kind: &str,
    hash: Option<String>,
    renamed_from: Option<&Path>,
) {
    let mut detail = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "kind": kind,
        "hash": hash,
    });
    if let Some(from) = renamed_from {
        detail["renamedFrom"] = serde_json::json!(from.to_string_lossy().to_string());
    }
    let event = crate::events::BackendEvent {
        id: crate::events::next_event_id("watcher_file"),
        event_type: crate::events::EventType::WatcherFile,
        status: crate::events::EventStatus::Done,
        created_at: crate::events::now_ms(),
        ts: crate::events::now_ms(),
        duration_ms: None,
        model: None,
        estimated_cost_usd: None,
        final_cost_usd: None,
        block: None,
        summary: format!("file change: {kind} {}", path.display()),
        detail: Some(detail),
    };
    crate::events::emit_backend_event(app, &event);
}

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
        process_path_locked(&app, &store, &path);
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
    process_path_locked(app, &store, path);
}

/// Read the file and process it. The caller holds the per-path lock. A missing
/// file means the path was removed (or renamed away), so all stored blocks for
/// it are dropped. Other read errors are logged and skipped. On success a
/// "backend://event" store_update card reports the apply stats.
fn process_path_locked(app: &tauri::AppHandle, store: &StoreState, path: &Path) {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            store.with_conn(|conn| remove_all_blocks_for_path(conn, path));
            return;
        }
        Err(e) => {
            tracing::warn!(category = "pipeline", "cannot read {}: {e}", path.display());
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
    let basename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    let event = crate::events::BackendEvent {
        id: crate::events::next_event_id("store_update"),
        event_type: crate::events::EventType::StoreUpdate,
        status: crate::events::EventStatus::Done,
        created_at: crate::events::now_ms(),
        ts: crate::events::now_ms(),
        duration_ms: None,
        model: None,
        estimated_cost_usd: None,
        final_cost_usd: None,
        block: None,
        summary: format!(
            "store: {basename} processed ({} added, {} changed, {} removed, {} enqueued)",
            diff.added, diff.changed, diff.removed, stats.enqueued
        ),
        detail: Some(serde_json::json!({
            "path": path.to_string_lossy().to_string(),
            "stored": stats.stored,
            "updated": stats.updated,
            "deleted": stats.deleted,
            "enqueued": stats.enqueued,
            "unchanged": diff.unchanged,
            "changed": diff.changed,
            "added": diff.added,
            "moved": diff.moved,
            "removed": diff.removed,
        })),
    };
    crate::events::emit_backend_event(app, &event);
}