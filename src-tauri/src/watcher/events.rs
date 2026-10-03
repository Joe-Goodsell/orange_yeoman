//! Watcher event payloads and emission on the "watcher://change" channel.

use super::commands::WatcherState;
use super::snapshots::{diff_snapshots, snapshots_of, DiffKind};
use serde::Serialize;
use std::fs;
use std::path::Path;
use tauri::{Emitter, Manager};

#[derive(Serialize, Clone)]
struct WatcherEvent {
    path: String,
    kind: String,
    // Stable content hash of the new on-disk bytes. Present only on content
    // events; omitted from JSON otherwise so existing frontend consumers stay
    // compatible.
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
}

// Emit block-level diff events for a changed file. This is a debug-only path:
// with the debug flag off the cache entry is dropped (so a later re-enable
// starts from a fresh baseline) and nothing else happens.
pub(crate) fn emit_block_diffs(app: &tauri::AppHandle, path: &Path) {
    let config_state = app.state::<crate::config::ConfigState>();
    if !config_state.debug() {
        app.state::<WatcherState>()
            .block_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path);
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            // Keep the old cache: the file may be transiently unreadable
            // (e.g. mid-save), and the next change event will retry.
            tracing::debug!(
                category = "watcher",
                "block diff unavailable: could not read {}: {}",
                path.display(),
                err
            );
            return;
        }
    };

    let new_snapshots = snapshots_of(&content);
    // Lock only to swap the cached snapshot. A poisoned mutex is skipped
    // silently: this is a debug-only path, not worth surfacing to the user.
    let old = match app.state::<WatcherState>().block_cache.lock() {
        Ok(mut guard) => guard.insert(path.to_path_buf(), new_snapshots.clone()),
        Err(poisoned) => poisoned
            .into_inner()
            .insert(path.to_path_buf(), new_snapshots.clone()),
    };

    let Some(old_snapshots) = old else {
        // First sight of this file: store the baseline and report it. The
        // snapshots are already cached above, so later edits diff against it.
        tracing::debug!(
            category = "watcher",
            "block baseline: {} blocks in {}",
            new_snapshots.len(),
            path.display()
        );
        return;
    };

    for diff in diff_snapshots(&old_snapshots, &new_snapshots) {
        let range = format!("{}:{}-{}", path.display(), diff.start_line, diff.end_line);
        match diff.kind {
            DiffKind::Added => {
                let excerpt = diff.new_excerpt.unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block added in {range}: \"{excerpt}\""
                );
            }
            DiffKind::Removed => {
                let excerpt = diff.old_excerpt.unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block removed from {range}: \"{excerpt}\""
                );
            }
            DiffKind::Changed => {
                let old_excerpt = diff.old_excerpt.unwrap_or_default();
                let new_excerpt = diff.new_excerpt.unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block changed in {range}: \"{old_excerpt}\" -> \"{new_excerpt}\""
                );
            }
        }
    }
}

pub(crate) fn emit_watcher_change(app: &tauri::AppHandle, path: &Path, kind: &str, hash: Option<String>) {
    let _ = app.emit(
        "watcher://change",
        WatcherEvent {
            path: path.to_string_lossy().to_string(),
            kind: kind.to_string(),
            hash,
        },
    );
}