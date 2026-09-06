//! Task domain types: lifecycle status, metadata, events, and results.
//! All are frontend-facing payloads that never carry API key material.

use serde::{Deserialize, Serialize};

/// Lifecycle status of a task. Serialized as lowercase strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Stale,
}

/// Metadata attached to a task. Serialized as camelCase for Tauri IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskMetadata {
    pub(crate) task_id: String,
    pub(crate) file_path: Option<String>,
    pub(crate) block_hash: String,
    pub(crate) source_hash: String,
    /// File-relative byte span of the inline command line. Only set for
    /// Inline-trigger tasks; absent from JSON when None so existing frontend
    /// consumers stay compatible. These are file-relative BYTE offsets, not
    /// character offsets; the frontend SourceRange uses character offsets, so
    /// conversion is needed when wiring the editor. The block_hash is the
    /// staleness key for inline tasks; this span records where the command
    /// line lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focus_start: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) focus_end: Option<usize>,
    /// Stable hash of the raw inline command line text. Only set for
    /// Inline-trigger tasks; absent from JSON when None so existing frontend
    /// consumers stay compatible. Staleness for inline tasks is line-granular:
    /// an edit to the command line itself invalidates the result, while edits
    /// to sibling lines in the same block do not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) line_text_hash: Option<String>,
    pub(crate) trigger: crate::pipeline::Trigger,
    pub(crate) status: TaskStatus,
    pub(crate) stale: bool,
    pub(crate) error: Option<String>,
}

/// A task event emitted to the frontend. Never contains API keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskEvent {
    pub(crate) task_id: String,
    pub(crate) status: TaskStatus,
    pub(crate) kind: crate::llm::LlmRequestKind,
    pub(crate) stale: bool,
    pub(crate) error: Option<String>,
}

/// A completed result payload. Never contains API keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskResult {
    pub(crate) task_id: String,
    pub(crate) status: TaskStatus,
    pub(crate) kind: crate::llm::LlmRequestKind,
    pub(crate) stale: bool,
    pub(crate) result: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
}