//! Async task dispatch: runs a provider call on the async runtime while
//! emitting Tauri events as the task progresses.

use tauri::async_runtime;
use tauri::{AppHandle, Emitter};

use super::ids::DedupIdentity;
use super::staleness::{is_inline_result_stale, is_result_stale};
use super::store::TaskStore;
use super::types::{TaskEvent, TaskMetadata, TaskResult, TaskStatus};

/// Sanitize an error string before it reaches the frontend: replace
/// API-key-like tokens (whitespace-delimited tokens starting with "sk-"),
/// strip control characters, and cap the length at 200 chars. This mirrors
/// the scrubbing done in llm.rs so no secret material leaks through task
/// payloads.
pub(crate) fn scrub_error(msg: &str) -> String {
    msg.split_whitespace()
        .map(|token| {
            if token.starts_with("sk-") {
                "sk-[REDACTED]".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect()
}

/// Collapse whitespace to single spaces and cap the length. Used for debug log
/// summaries so an event message always stays on one line.
fn collapse_and_cap(text: &str, cap: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(cap)
        .collect()
}

/// The snake_case name of a task status, matching its serde serialization.
/// Used in debug log messages so the console shows the same names as IPC.
fn status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::Completed => "completed",
        TaskStatus::Failed => "failed",
        TaskStatus::Stale => "stale",
    }
}

/// Compute the source hash for a task: the whole-file content hash when a
/// file path is available, otherwise the hash of the block text. The stale
/// check in dispatch_task re-reads the file and hashes its content, so a
/// block hash would never match and every task with a file_path would be
/// marked stale.
pub(crate) fn source_hash_of(file_path: &Option<String>, block_text: &str) -> Result<String, String> {
    match file_path {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| scrub_error(&format!("file not readable: {e}")))?;
            Ok(crate::pipeline::stable_hash(&content))
        }
        None => Ok(crate::pipeline::stable_hash(block_text)),
    }
}

/// Resolve the model id shown in the llm_call debug event. A provider that
/// serves a fixed model (the mock) overrides the request's model hint; the
/// real provider serves the configured model; an unset model shows as
/// "<unset>".
pub(crate) fn llm_call_model_label(
    provider: &dyn crate::llm::LlmProvider,
    request: &crate::llm::LlmRequest,
) -> String {
    provider
        .serving_model()
        .map(str::to_string)
        .or_else(|| request.model.clone().filter(|m| !m.is_empty()))
        .unwrap_or_else(|| "<unset>".to_string())
}

/// Dispatch an LLM task asynchronously. Returns the task_id immediately.
/// The provider call runs in a spawned async task. Events are emitted on completion.
///
/// Events:
/// - "agent://task-updated" with TaskEvent payload when status changes to Running.
/// - "agent://result-ready" with TaskResult payload when the task completes, goes stale, or fails.
pub(crate) fn dispatch_task(
    app: AppHandle,
    store: std::sync::Arc<TaskStore>,
    provider: std::sync::Arc<dyn crate::llm::LlmProvider>,
    request: crate::llm::LlmRequest,
    metadata: TaskMetadata,
    identity: DedupIdentity,
) -> Result<String, String> {
    let Some(task_id) = store.insert(metadata.clone(), &identity) else {
        return Err("duplicate automatic task already in progress".to_string());
    };

    let spawned_task_id = task_id.clone();
    async_runtime::spawn(async move {
        // Mark the task as running and notify the frontend.
        store.update(&spawned_task_id, TaskStatus::Running, false, None);
        tracing::debug!(category = "task", "task {} -> running", spawned_task_id);
        let event = TaskEvent {
            task_id: spawned_task_id.clone(),
            status: TaskStatus::Running,
            kind: request.kind,
            stale: false,
            error: None,
        };
        let _ = app.emit("agent://task-updated", event);

        // Debug instrumentation: report the outgoing LLM call (kind and
        // resolved model) right before it is dispatched to the provider.
        // The category field keeps the existing "llm_call" UI category. The
        // provider's serving_model (the mock identity) overrides the request's
        // model hint so mock calls are identifiable in the debug console.
        let kind_str = match request.kind {
            crate::llm::LlmRequestKind::Extraction => "extraction",
            crate::llm::LlmRequestKind::FactCheck => "fact_check",
            crate::llm::LlmRequestKind::Research => "research",
        };
        let request_kind = request.kind;
        let model = llm_call_model_label(provider.as_ref(), &request);
        tracing::info!(category = "llm_call", "{kind_str} -> {model}");

        // The provider call runs in an INNER task so that a panic inside it is
        // observed by the supervisor below instead of silently hanging the
        // outer task and leaving the frontend stuck on "running". The inner
        // task returns the outcome as a Result; the outer task maps it to
        // frontend events and always clears the dedup identity. Capture the
        // start instant so the return events can report the call duration.
        let started = std::time::Instant::now();
        let inner = async_runtime::spawn(async move {
            provider.complete(request.clone()).await.map(|response| {
                // Inline command tasks anchor to a single command line, so
                // their staleness is line-granular: re-parse the file and
                // check the raw command line's hash. A missing line_text_hash
                // cannot be verified and is treated as stale. Every other task
                // compares the whole-file content hash, so any edit anywhere
                // in the file marks it stale.
                let stale = if metadata.trigger == crate::pipeline::Trigger::Inline {
                    let line_text_hash = metadata.line_text_hash.as_deref().unwrap_or_default();
                    is_inline_result_stale(
                        metadata.file_path.as_deref(),
                        line_text_hash,
                        |path| std::fs::read_to_string(path).map_err(|_| ()),
                    )
                } else {
                    is_result_stale(
                        metadata.file_path.as_deref(),
                        &metadata.source_hash,
                        |path| std::fs::read_to_string(path).map_err(|_| ()),
                    )
                };
                let (status, stale_flag) = if stale {
                    (TaskStatus::Stale, true)
                } else {
                    (TaskStatus::Completed, false)
                };
                (status, stale_flag, response.result)
            })
        });

        match inner.await {
            Ok(Ok((status, stale_flag, result_value))) => {
                let dur = format!("{:.2}s", started.elapsed().as_secs_f64());
                let stale_note = if stale_flag { " (stale)" } else { "" };
                let summary = collapse_and_cap(&result_value.to_string(), 120);
                tracing::info!(
                    category = "llm_call",
                    "{kind_str} returned in {dur}{stale_note}: {summary}"
                );
                store.update(&spawned_task_id, status, stale_flag, None);
                tracing::debug!(
                    category = "task",
                    "task {} -> {}",
                    spawned_task_id,
                    status_name(status)
                );
                let result = TaskResult {
                    task_id: spawned_task_id.clone(),
                    status,
                    kind: request_kind,
                    stale: stale_flag,
                    result: Some(result_value),
                    error: None,
                };
                let _ = app.emit("agent://result-ready", result);
            }
            Ok(Err(e)) => {
                let dur = format!("{:.2}s", started.elapsed().as_secs_f64());
                // The LlmError Display impl already scrubs key material, so
                // the raw error is safe for the debug console at error level
                // (always visible even with the debug flag off).
                tracing::error!(category = "llm_call", "{kind_str} failed in {dur}: {e}");
                let sanitized = scrub_error(&e.to_string());
                store.update(
                    &spawned_task_id,
                    TaskStatus::Failed,
                    false,
                    Some(sanitized.clone()),
                );
                tracing::debug!(category = "task", "task {} -> failed", spawned_task_id);
                let result = TaskResult {
                    task_id: spawned_task_id.clone(),
                    status: TaskStatus::Failed,
                    kind: request_kind,
                    stale: false,
                    result: None,
                    error: Some(sanitized),
                };
                let _ = app.emit("agent://result-ready", result);
            }
            Err(join_error) => {
                let dur = format!("{:.2}s", started.elapsed().as_secs_f64());
                // The inner task panicked. Never scrub the panic payload: the
                // fixed safe message crosses to the frontend result payload;
                // the panic details go to the error-level debug event and stderr.
                let sanitized = scrub_error("internal task error");
                store.update(
                    &spawned_task_id,
                    TaskStatus::Failed,
                    false,
                    Some(sanitized.clone()),
                );
                tracing::debug!(category = "task", "task {} -> failed", spawned_task_id);
                let result = TaskResult {
                    task_id: spawned_task_id.clone(),
                    status: TaskStatus::Failed,
                    kind: request_kind,
                    stale: false,
                    result: None,
                    error: Some(sanitized),
                };
                let _ = app.emit("agent://result-ready", result);
                tracing::error!(
                    category = "task",
                    "task {} panicked after {dur}: {:?}",
                    spawned_task_id,
                    join_error
                );
            }
        }

        store.clear_dedup(&identity);
    });

    Ok(task_id)
}