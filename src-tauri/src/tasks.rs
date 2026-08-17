// Orange Yeoman - Task domain types and pure helpers for the AI pipeline.
// This module defines task metadata, events, and results (all frontend-facing
// payloads that never carry API key material), plus a deterministic task id
// derived from a dedup identity and a pure stale-check helper. The in-memory
// TaskStore tracks active and completed tasks, and dispatch_task runs a
// provider call on the async runtime while emitting Tauri events as the task
// progresses. There is no Tauri command surface here; commands live in later
// slices.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tauri::async_runtime;
use tauri::{AppHandle, Emitter, Manager, State};

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
    pub(crate) stale: bool,
    pub(crate) result: Option<serde_json::Value>,
    pub(crate) error: Option<String>,
}

/// Generate a deterministic task id from a dedup identity.
/// Format: "{block_hash}:{trigger}:{kind}" lowercased and with non-alphanumeric chars replaced by '_'.
pub(crate) fn task_id_from_identity(
    block_hash: &str,
    trigger: &crate::pipeline::Trigger,
    kind: &crate::llm::LlmRequestKind,
) -> String {
    let trigger_str = match trigger {
        crate::pipeline::Trigger::Automatic => "automatic",
        crate::pipeline::Trigger::FactCheck => "fact_check",
        crate::pipeline::Trigger::Research => "research",
    };
    let kind_str = match kind {
        crate::llm::LlmRequestKind::Extraction => "extraction",
        crate::llm::LlmRequestKind::FactCheck => "fact_check",
        crate::llm::LlmRequestKind::Research => "research",
    };
    format!("{block_hash}:{trigger_str}:{kind_str}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Build the task id for an explicit fact-check command. The base id from
/// task_id_from_identity is disambiguated with a hash of the claim text so
/// that two fact-checks of different claims in the same block get distinct
/// task ids instead of overwriting each other in the store.
pub(crate) fn fact_check_task_id(block_hash: &str, claim_text: &str) -> String {
    format!(
        "{}:{}",
        task_id_from_identity(
            block_hash,
            &crate::pipeline::Trigger::FactCheck,
            &crate::llm::LlmRequestKind::FactCheck,
        ),
        crate::pipeline::stable_hash(claim_text)
    )
}

/// Build the task id for an explicit research command. The base id from
/// task_id_from_identity is disambiguated with a hash of the goal so that two
/// research tasks with different goals in the same block get distinct task
/// ids instead of overwriting each other in the store.
pub(crate) fn research_task_id(block_hash: &str, goal: &str) -> String {
    format!(
        "{}:{}",
        task_id_from_identity(
            block_hash,
            &crate::pipeline::Trigger::Research,
            &crate::llm::LlmRequestKind::Research,
        ),
        crate::pipeline::stable_hash(goal)
    )
}

/// A dedup identity for automatic tasks. Two automatic tasks with the same identity
/// while queued or running should not both be dispatched.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DedupIdentity {
    pub(crate) block_hash: String,
    pub(crate) trigger: crate::pipeline::Trigger,
    pub(crate) kind: crate::llm::LlmRequestKind,
}

/// Build a dedup identity for an automatic task.
pub(crate) fn dedup_identity(
    block_hash: &str,
    trigger: crate::pipeline::Trigger,
    kind: crate::llm::LlmRequestKind,
) -> DedupIdentity {
    DedupIdentity {
        block_hash: block_hash.to_string(),
        trigger,
        kind,
    }
}

/// Determine whether a result is stale by comparing the submitted source hash
/// with the current source hash. Returns true when:
/// - the file path is None (cannot verify; treat as not stale by default), OR
/// - the file does not exist, OR
/// - the file exists but its current content hash differs from submitted_source_hash.
/// Otherwise return false.
///
/// `read_file` is a function that takes a path and returns Ok(String) with the file content,
/// or Err(()) if the file cannot be read. This keeps the function pure and testable.
pub(crate) fn is_result_stale(
    file_path: Option<&str>,
    submitted_source_hash: &str,
    read_file: impl Fn(&str) -> Result<String, ()>,
) -> bool {
    let Some(path) = file_path else {
        return false;
    };
    let content = match read_file(path) {
        Ok(content) => content,
        Err(()) => return true,
    };
    crate::pipeline::stable_hash(&content) != submitted_source_hash
}

/// In-memory store of active and completed tasks. Thread-safe via Mutex.
pub(crate) struct TaskStore {
    tasks: Mutex<HashMap<String, TaskMetadata>>,
    active_dedup: Mutex<HashSet<DedupIdentity>>,
}

impl TaskStore {
    pub(crate) fn new() -> Self {
        TaskStore {
            tasks: Mutex::new(HashMap::new()),
            active_dedup: Mutex::new(HashSet::new()),
        }
    }

    /// Insert a new task. Returns the task_id.
    /// If an automatic task with the same DedupIdentity is already active (Queued or Running),
    /// return None to signal a duplicate. Explicit commands (FactCheck or Research trigger)
    /// always insert and never dedup.
    pub(crate) fn insert(
        &self,
        metadata: TaskMetadata,
        identity: &DedupIdentity,
    ) -> Option<String> {
        {
            let mut active = self.active_dedup.lock().unwrap();
            if identity.trigger == crate::pipeline::Trigger::Automatic && active.contains(identity)
            {
                return None;
            }
            active.insert(identity.clone());
        }

        let mut tasks = self.tasks.lock().unwrap();
        let task_id = metadata.task_id.clone();
        tasks.insert(task_id.clone(), metadata);
        Some(task_id)
    }

    /// Get a copy of a task by id.
    pub(crate) fn get(&self, task_id: &str) -> Option<TaskMetadata> {
        self.tasks.lock().unwrap().get(task_id).cloned()
    }

    /// Update a task's status, stale flag, and error.
    pub(crate) fn update(
        &self,
        task_id: &str,
        status: TaskStatus,
        stale: bool,
        error: Option<String>,
    ) {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(task_id) {
            task.status = status;
            task.stale = stale;
            task.error = error;
        }
    }

    /// Remove a task's dedup identity from the active set (call when task completes or fails).
    pub(crate) fn clear_dedup(&self, identity: &DedupIdentity) {
        self.active_dedup.lock().unwrap().remove(identity);
    }
}

impl Default for TaskStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Sanitize an error string before it reaches the frontend: replace
/// API-key-like tokens (whitespace-delimited tokens starting with "sk-"),
/// strip control characters, and cap the length at 200 chars. This mirrors
/// the scrubbing done in llm.rs so no secret material leaks through task
/// payloads.
fn scrub_error(msg: &str) -> String {
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
        if app.state::<crate::config::ConfigState>().debug() {
            let kind_str = match request.kind {
                crate::llm::LlmRequestKind::Extraction => "extraction",
                crate::llm::LlmRequestKind::FactCheck => "fact_check",
                crate::llm::LlmRequestKind::Research => "research",
            };
            let model = request
                .model
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or("<unset>");
            crate::debug::emit_debug_event(
                &app,
                "llm_call",
                format!("{kind_str} -> {model}"),
            );
        }

        match provider.complete(request.clone()).await {
            Ok(response) => {
                let stale = is_result_stale(
                    metadata.file_path.as_deref(),
                    &metadata.source_hash,
                    |path| std::fs::read_to_string(path).map_err(|_| ()),
                );
                let (status, stale_flag) = if stale {
                    (TaskStatus::Stale, true)
                } else {
                    (TaskStatus::Completed, false)
                };
                store.update(&spawned_task_id, status, stale_flag, None);
                let result = TaskResult {
                    task_id: spawned_task_id.clone(),
                    status,
                    stale: stale_flag,
                    result: Some(response.result),
                    error: None,
                };
                let _ = app.emit("agent://result-ready", result);
            }
            Err(e) => {
                let sanitized = scrub_error(&e.to_string());
                store.update(
                    &spawned_task_id,
                    TaskStatus::Failed,
                    false,
                    Some(sanitized.clone()),
                );
                let result = TaskResult {
                    task_id: spawned_task_id.clone(),
                    status: TaskStatus::Failed,
                    stale: false,
                    result: None,
                    error: Some(sanitized),
                };
                let _ = app.emit("agent://result-ready", result);
            }
        }

        store.clear_dedup(&identity);
    });

    Ok(task_id)
}

// --- Tauri command surface ---
//
// The commands below wire the pipeline to the task store. They take the store
// as `State<'_, Arc<TaskStore>>` because dispatch_task owns an Arc<TaskStore>;
// the Arc is cloned from the managed state and handed to the spawned task.

/// Process a block of markdown automatically: parse, classify, route, and
/// dispatch an extraction when the routing decision is NeedsExtraction or
/// HighConfidenceLocal. Skip and Ignore decisions do nothing. Returns the new
/// task id, or None when no task was dispatched.
#[tauri::command]
pub(crate) async fn submit_auto_task(
    app: AppHandle,
    store: State<'_, Arc<TaskStore>>,
    llm_state: State<'_, crate::llm::LlmState>,
    config_state: State<'_, crate::config::ConfigState>,
    file_path: Option<String>,
    block_text: String,
) -> Result<Option<String>, String> {
    let blocks = crate::pipeline::parse_markdown_blocks(&block_text);
    let Some(block) = blocks.into_iter().find(|b| !b.excluded) else {
        return Ok(None);
    };

    // Parse a slash command from the block and let route_block honor it.
    // Explicit commands (/fact-check, /research) are dispatched via the
    // dedicated submit_fact_check / submit_research commands, not the auto
    // path, so the auto path only handles NeedsExtraction and
    // HighConfidenceLocal. /ignore silently drops the block.
    let command = crate::pipeline::parse_slash_command_in_block(&block);
    // Debug instrumentation: report a recognized slash command so the frontend
    // can show what the auto path saw. The argument is truncated to keep the
    // event payload small.
    if let Some(parsed) = &command {
        if config_state.debug() {
            let name = match &parsed.command {
                crate::pipeline::SlashCommand::FactCheck => "/fact-check",
                crate::pipeline::SlashCommand::Research => "/research",
                crate::pipeline::SlashCommand::Ignore => "/ignore",
            };
            let mut message = name.to_string();
            if let Some(argument) = &parsed.argument {
                let truncated: String = argument.chars().take(80).collect();
                message.push(' ');
                message.push_str(&truncated);
            }
            crate::debug::emit_debug_event(&app, "slash_command", message);
        }
    }
    let decision = crate::pipeline::route_block(&block, command.as_ref());
    match decision {
        crate::pipeline::RoutingDecision::NeedsExtraction
        | crate::pipeline::RoutingDecision::HighConfidenceLocal => {}
        crate::pipeline::RoutingDecision::Skip
        | crate::pipeline::RoutingDecision::Ignore
        | crate::pipeline::RoutingDecision::FactCheck
        | crate::pipeline::RoutingDecision::Research => return Ok(None),
    }

    let small_model = config_state.small_model();
    let request = crate::pipeline::build_extraction_request(&block, &small_model);
    let block_hash = block.block_hash.clone();
    // The source hash must cover the whole file, not the block text: the
    // stale check in dispatch_task re-reads the file and hashes its content,
    // so a block hash would never match and every automatic task with a
    // file_path would be marked stale.
    let source_hash = match &file_path {
        Some(path) => {
            let content = std::fs::read_to_string(path)
                .map_err(|e| scrub_error(&format!("file not readable: {e}")))?;
            crate::pipeline::stable_hash(&content)
        }
        None => crate::pipeline::stable_hash(&block_text),
    };
    let task_id = task_id_from_identity(
        &block_hash,
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::Extraction,
    );
    let metadata = TaskMetadata {
        task_id,
        file_path,
        block_hash,
        source_hash,
        trigger: crate::pipeline::Trigger::Automatic,
        status: TaskStatus::Queued,
        stale: false,
        error: None,
    };
    let identity = dedup_identity(
        &metadata.block_hash,
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    );

    let store_arc = store.inner().clone();
    let provider_arc = llm_state.provider();
    let task_id = match dispatch_task(app, store_arc, provider_arc, request, metadata, identity) {
        Ok(task_id) => task_id,
        Err(e) if e.starts_with("duplicate") => {
            // Duplicate automatic tasks are silently dropped because the
            // original task is still in progress and will emit its own result.
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    Ok(Some(task_id))
}

/// Submit an explicit fact-check task for a claim. Always inserts; explicit
/// commands are never deduped. Returns the new task id.
#[tauri::command]
pub(crate) async fn submit_fact_check(
    app: AppHandle,
    store: State<'_, Arc<TaskStore>>,
    llm_state: State<'_, crate::llm::LlmState>,
    config_state: State<'_, crate::config::ConfigState>,
    file_path: Option<String>,
    claim_text: String,
    block_text: String,
    heading_chain: Vec<String>,
    source_hash: String,
) -> Result<String, String> {
    if config_state.debug() {
        crate::debug::emit_debug_event(&app, "slash_command", "/fact-check".to_string());
    }
    let small_model = config_state.small_model();
    let request = crate::pipeline::build_fact_check_request(
        &claim_text,
        &block_text,
        &heading_chain,
        &source_hash,
        &small_model,
    );
    let block_hash = crate::pipeline::stable_hash(&block_text);
    // The task id is disambiguated with the claim hash so two fact-checks of
    // different claims in the same block do not collide. The DedupIdentity
    // keeps the plain block_hash; explicit commands bypass dedup anyway.
    let task_id = fact_check_task_id(&block_hash, &claim_text);
    let metadata = TaskMetadata {
        task_id,
        file_path,
        block_hash,
        source_hash,
        trigger: crate::pipeline::Trigger::FactCheck,
        status: TaskStatus::Queued,
        stale: false,
        error: None,
    };
    let identity = dedup_identity(
        &metadata.block_hash,
        crate::pipeline::Trigger::FactCheck,
        crate::llm::LlmRequestKind::FactCheck,
    );

    let store_arc = store.inner().clone();
    let provider_arc = llm_state.provider();
    dispatch_task(app, store_arc, provider_arc, request, metadata, identity)
}

/// Submit an explicit research task for a goal and selection. Always inserts;
/// explicit commands are never deduped. Returns the new task id.
#[tauri::command]
pub(crate) async fn submit_research(
    app: AppHandle,
    store: State<'_, Arc<TaskStore>>,
    llm_state: State<'_, crate::llm::LlmState>,
    config_state: State<'_, crate::config::ConfigState>,
    file_path: Option<String>,
    goal: String,
    selection: String,
    document: String,
    source_hash: String,
) -> Result<String, String> {
    if config_state.debug() {
        crate::debug::emit_debug_event(&app, "slash_command", "/research".to_string());
    }
    let large_model = config_state.large_model();
    let request =
        crate::pipeline::build_research_request(&goal, &selection, &document, &large_model);
    let block_hash = crate::pipeline::stable_hash(&document);
    // The task id is disambiguated with a hash of the goal so two research
    // tasks with different goals in the same block do not collide. The
    // DedupIdentity keeps the plain block_hash; explicit commands bypass
    // dedup anyway.
    let task_id = research_task_id(&block_hash, &goal);
    let metadata = TaskMetadata {
        task_id,
        file_path,
        block_hash,
        source_hash,
        trigger: crate::pipeline::Trigger::Research,
        status: TaskStatus::Queued,
        stale: false,
        error: None,
    };
    let identity = dedup_identity(
        &metadata.block_hash,
        crate::pipeline::Trigger::Research,
        crate::llm::LlmRequestKind::Research,
    );

    let store_arc = store.inner().clone();
    let provider_arc = llm_state.provider();
    dispatch_task(app, store_arc, provider_arc, request, metadata, identity)
}

/// Look up a task by id. Returns None when the task does not exist.
#[tauri::command]
pub(crate) fn get_task_status(
    store: State<'_, Arc<TaskStore>>,
    task_id: String,
) -> Result<Option<TaskMetadata>, String> {
    Ok(store.get(&task_id))
}

#[cfg(test)]
mod tests;
