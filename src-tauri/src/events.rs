//! Structured backend event schema for the "backend://event" Tauri channel.
//! The single-pane backend debug console renders these events as cards: the
//! `id` identifies a card, `status` drives its lifecycle, and later emissions
//! with the same id update the card in place. Payloads never carry API key
//! material; free-form `detail` is JSON for the expanded view.

use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The closed, extensible set of backend event kinds. New variants are added
/// here as backend work grows (PER-31 will add concept-extraction types); the
/// frontend mirrors this enum as a TS union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    WatcherFile,  // serializes "watcher_file"
    BlockDiff,    // "block_diff"
    StoreUpdate,  // "store_update"
    LlmCall,      // "llm_call"
    ConfigReload, // "config_reload"
}

/// Lifecycle status of a backend event card. One id moves Queued -> InFlight
/// -> Done/Failed as the underlying work progresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventStatus {
    Queued,   // "queued"
    InFlight, // "in_flight"
    Done,     // "done"
    Failed,   // "failed"
}

/// Reference to the file/block a backend event is about. Optional fields are
/// null when unknown so the pane can distinguish "unknown" from "zero".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockRef {
    pub file_path: Option<String>,
    pub block_hash: String,
    pub block_kind: Option<String>,     // block_kind_as_str value, when known
    pub block_start: Option<usize>,     // byte offset of block start in the file, when known
    pub block_end: Option<usize>,       // byte offset of block end in the file, when known
    pub position_start: Option<usize>,  // range within the block this event is about
    pub position_end: Option<usize>,
    pub excerpt: Option<String>,        // single-line excerpt, ~80 chars max
}

impl BlockRef {
    /// Build a block reference from a parsed pipeline block. Byte offsets and
    /// the kind come from the block; the position range stays None (a block
    /// ref, not a range within it).
    ///
    /// Schema API used by the tests now; the emission sites that consume a
    /// full parsed block arrive with later backend work.
    #[allow(dead_code)]
    pub fn from_block(file_path: Option<&str>, block: &crate::pipeline::Block) -> Self {
        BlockRef {
            file_path: file_path.map(str::to_string),
            block_hash: block.block_hash.clone(),
            block_kind: Some(crate::pipeline::block_kind_as_str(&block.kind).to_string()),
            block_start: Some(block.char_start),
            block_end: Some(block.char_end),
            position_start: None,
            position_end: None,
            excerpt: excerpt_of(&block.text),
        }
    }

    /// Build a block reference from task metadata: the task's file, block
    /// hash, and inline focus span. Offsets and kind are not tracked there.
    pub fn from_task_metadata(meta: &crate::tasks::TaskMetadata) -> Self {
        BlockRef {
            file_path: meta.file_path.clone(),
            block_hash: meta.block_hash.clone(),
            block_kind: None,
            block_start: None,
            block_end: None,
            position_start: meta.focus_start,
            position_end: meta.focus_end,
            excerpt: None,
        }
    }
}

/// One backend event card payload. Same id = same card, updated in place by
/// the pane. `created_at` is set at first emission; `ts` refreshes on every
/// emission; `duration_ms` is set at the terminal status.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendEvent {
    pub id: String,
    pub event_type: EventType,
    pub status: EventStatus,
    pub created_at: u64, // ms since UNIX epoch, set at first emission
    pub ts: u64,         // ms since UNIX epoch, updated on every emission
    pub duration_ms: Option<u64>, // set at terminal status (done/failed)
    pub model: Option<String>,
    pub estimated_cost_usd: Option<f64>, // None = unknown; Some(0.0) for the mock provider
    pub final_cost_usd: Option<f64>,     // None = unknown; Some(0.0) for the mock provider
    pub block: Option<BlockRef>,
    pub summary: String, // one line, shown on the collapsed card row
    pub detail: Option<serde_json::Value>, // free-form structured detail for the expanded view
}

/// Milliseconds since the UNIX epoch, 0 on clock errors so emission never
/// panics.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Monotonic counter backing one-shot event ids.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique id for a one-shot event. Lifecycle events (llm_call) use the task
/// id instead so the pane updates one card across status changes.
pub fn next_event_id(prefix: &str) -> String {
    format!("{}-{}", prefix, COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Emit a backend event on the "backend://event" channel. Emission failure is
/// ignored: the console is a view, never a hard dependency of backend work.
pub fn emit_backend_event(app: &tauri::AppHandle, event: &BackendEvent) {
    use tauri::Emitter;
    let _ = app.emit("backend://event", event);
}

/// Collapse whitespace in the first line to single spaces, trim, and cap at
/// 80 chars. None when the first line is empty so the pane can fall back to a
/// hash prefix.
#[allow(dead_code)]
fn excerpt_of(text: &str) -> Option<String> {
    let line = text.lines().next()?;
    let collapsed: String = line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(80).collect())
}

#[cfg(test)]
mod tests;