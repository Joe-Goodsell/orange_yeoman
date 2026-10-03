//! Watcher event payloads and emission on the "watcher://change" channel.

use super::commands::WatcherState;
use super::snapshots::{diff_snapshots, snapshots_of, DiffKind, SnapshotDiff};
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

// Emit block-level diff events for a changed file. Each diff also becomes a
// "backend://event" card so the console shows per-block detail without the
// debug flag. The cache entry is always kept as the diff baseline.
pub(crate) fn emit_block_diffs(app: &tauri::AppHandle, path: &Path) {
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
    // silently: this is a best-effort view, not worth surfacing to the user.
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
        match &diff.kind {
            DiffKind::Added => {
                let excerpt = diff.new_excerpt.as_deref().unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block added in {range}: \"{excerpt}\""
                );
            }
            DiffKind::Removed => {
                let excerpt = diff.old_excerpt.as_deref().unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block removed from {range}: \"{excerpt}\""
                );
            }
            DiffKind::Changed => {
                let old_excerpt = diff.old_excerpt.as_deref().unwrap_or_default();
                let new_excerpt = diff.new_excerpt.as_deref().unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block changed in {range}: \"{old_excerpt}\" -> \"{new_excerpt}\""
                );
            }
        }
        emit_block_diff_event(app, path, &diff);
    }
}

// Emit one "backend://event" card per block diff. The block ref points at the
// hash/excerpt of the current side (new for Added/Changed, old for Removed);
// the line range goes into detail as numbers.
fn emit_block_diff_event(app: &tauri::AppHandle, path: &Path, diff: &SnapshotDiff) {
    let (hash, excerpt, diff_kind) = match &diff.kind {
        DiffKind::Added => (&diff.new_hash, diff.new_excerpt.as_deref(), "added"),
        DiffKind::Removed => (&diff.old_hash, diff.old_excerpt.as_deref(), "removed"),
        DiffKind::Changed => (&diff.new_hash, diff.new_excerpt.as_deref(), "changed"),
    };
    let hash = hash.clone().unwrap_or_default();
    let excerpt = excerpt.unwrap_or_default().to_string();
    let block = crate::events::BlockRef {
        file_path: Some(path.to_string_lossy().to_string()),
        block_hash: hash.clone(),
        block_kind: None,
        block_start: None,
        block_end: None,
        position_start: None,
        position_end: None,
        excerpt: if excerpt.is_empty() {
            None
        } else {
            Some(excerpt.clone())
        },
    };
    let label = if excerpt.is_empty() {
        hash.chars().take(8).collect::<String>()
    } else {
        excerpt
    };
    let event = crate::events::BackendEvent {
        id: crate::events::next_event_id("block_diff"),
        event_type: crate::events::EventType::BlockDiff,
        status: crate::events::EventStatus::Done,
        created_at: crate::events::now_ms(),
        ts: crate::events::now_ms(),
        duration_ms: None,
        model: None,
        estimated_cost_usd: None,
        final_cost_usd: None,
        block: Some(block),
        summary: format!("block {:?}: {label}", diff.kind),
        detail: Some(serde_json::json!({
            "diffKind": diff_kind,
            "startLine": diff.start_line,
            "endLine": diff.end_line,
        })),
    };
    crate::events::emit_backend_event(app, &event);
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