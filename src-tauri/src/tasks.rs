// Orange Yeoman - Task domain types and pure helpers for the AI pipeline.
// This module defines task metadata, events, and results (all frontend-facing
// payloads that never carry API key material), plus a deterministic task id
// derived from a dedup identity and a pure stale-check helper. The in-memory
// TaskStore tracks active and completed tasks, and dispatch_task runs a
// provider call on the async runtime while emitting Tauri events as the task
// progresses. The Tauri command surface (submit_block, submit_fact_check,
// submit_research, get_task_status) lives in the `commands` submodule; the
// module root re-exports it.

mod commands;
mod dispatch;
mod ids;
mod staleness;
mod store;
mod types;

pub(crate) use commands::{
    submit_block, submit_fact_check, submit_research, get_task_status,
    __cmd__submit_block, __cmd__submit_fact_check, __cmd__submit_research, __cmd__get_task_status,
    __tauri_command_name_submit_block, __tauri_command_name_submit_fact_check,
    __tauri_command_name_submit_research, __tauri_command_name_get_task_status,
};
pub(crate) use store::TaskStore;
// The task metadata is part of the backend event schema (BlockRef references
// it), so it is a permanent root re-export rather than a test-only one.
pub(crate) use types::TaskMetadata;

// Test-only re-exports. The sibling submodules import their items directly;
// the root re-exports these solely so tasks/tests.rs can keep `use super::*;`.
#[cfg(test)]
pub(crate) use dispatch::{cost_for_model_label, llm_call_model_label, llm_event, scrub_error};
#[cfg(test)]
pub(crate) use ids::{
    dedup_identity, fact_check_task_id, research_task_id, task_id_from_identity, DedupIdentity,
};
#[cfg(test)]
pub(crate) use staleness::{is_inline_result_stale, is_result_stale};
#[cfg(test)]
pub(crate) use types::{TaskEvent, TaskResult, TaskStatus};

#[cfg(test)]
mod tests;