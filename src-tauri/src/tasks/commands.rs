//! Tauri command surface: wires the pipeline to the task store.

use std::sync::Arc;
use tauri::{AppHandle, State};

use super::dispatch::{dispatch_task, source_hash_of};
use super::ids::{dedup_identity, fact_check_task_id, research_task_id, task_id_from_identity};
use super::store::TaskStore;
use super::types::{TaskMetadata, TaskStatus};

// --- Tauri command surface ---
//
// The commands below wire the pipeline to the task store. They take the store
// as `State<'_, Arc<TaskStore>>` because dispatch_task owns an Arc<TaskStore>;
// the Arc is cloned from the managed state and handed to the spawned task.

/// Process a block of markdown: parse, classify, route, and dispatch. The
/// routing decision determines the action: an inline /fact-check command
/// dispatches a fact-check task with the Inline trigger, an inline /research
/// command dispatches a research task with the Inline trigger, and a
/// needs-extraction block dispatches an extraction task. Inline /ignore is
/// recognized but not dispatched yet (roadmap), and Skip decisions do nothing.
/// Callers include both the slash-command path and future automatic paths.
/// Returns the new task id, or None when no task was dispatched.
#[tauri::command]
pub(crate) async fn submit_block(
    app: AppHandle,
    store: State<'_, Arc<TaskStore>>,
    llm_state: State<'_, crate::llm::LlmState>,
    config_state: State<'_, crate::config::ConfigState>,
    file_path: Option<String>,
    block_text: String,
) -> Result<Option<String>, String> {
    let blocks = crate::pipeline::parse_markdown_blocks(&block_text);
    // Prefer the first non-excluded block that contains a slash command so an
    // inline command after a heading/list/etc. is not silently dropped; fall
    // back to the first non-excluded block when no block has a command (the
    // automatic path).
    let block = blocks
        .iter()
        .filter(|b| !b.excluded)
        .find(|b| crate::pipeline::parse_slash_command_in_block(b).is_some())
        .or_else(|| blocks.iter().find(|b| !b.excluded));
    let Some(block) = block else {
        return Ok(None);
    };

    // Parse a slash command from the block and let route_block honor it. An
    // inline /fact-check command dispatches a fact-check task with the Inline
    // trigger, and an inline /research command dispatches a research task with
    // the Inline trigger. Inline /ignore is recognized but not dispatched yet
    // (roadmap). Explicit commands go through the dedicated
    // submit_fact_check / submit_research commands.
    let command = crate::pipeline::parse_slash_command_in_block(&block);
    // Debug instrumentation: report a recognized slash command so the frontend
    // can show what the auto path saw. The argument is truncated to keep the
    // event payload small.
    if let Some(parsed) = &command {
        let name = match &parsed.command {
            crate::pipeline::SlashCommand::FactCheck => "/fact-check",
            crate::pipeline::SlashCommand::Research => "/research",
            crate::pipeline::SlashCommand::Ignore => "/ignore",
        };
        let mut message = name.to_string();
        if !parsed.focus_text.is_empty() {
            let truncated: String = parsed.focus_text.chars().take(80).collect();
            message.push(' ');
            message.push_str(&truncated);
        }
        tracing::info!(category = "slash_command", "{}", message);
    }
    let decision = crate::pipeline::route_block(&block, command.as_ref());
    match decision {
        crate::pipeline::RoutingDecision::FactCheck => {
            // Inline /fact-check: dispatch a fact-check task. route_block
            // returns FactCheck only when a parsed command is present, so the
            // command is guaranteed to exist here. The claim is the parsed
            // focus text; the block text and heading chain provide context.
            let parsed = command
                .as_ref()
                .expect("FactCheck decision implies a parsed command");
            let envelope =
                crate::pipeline::build_inline_envelope(&block, parsed, file_path.as_deref());
            let source_hash = source_hash_of(&file_path, &block_text)?;
            let small_model = config_state.small_model();
            let request = crate::pipeline::build_fact_check_request(
                &parsed.focus_text,
                &block.text,
                &block.heading_path,
                &source_hash,
                &small_model,
            );
            let task_id = crate::pipeline::inline_task_id(&envelope);
            let metadata = TaskMetadata {
                task_id,
                file_path,
                block_hash: block.block_hash.clone(),
                source_hash,
                focus_start: Some(envelope.focus_ref.start),
                focus_end: Some(envelope.focus_ref.end),
                line_text_hash: Some(crate::pipeline::stable_hash(
                    &block.text[parsed.line_start..parsed.line_end],
                )),
                trigger: crate::pipeline::Trigger::Inline,
                status: TaskStatus::Queued,
                stale: false,
                error: None,
            };
            let identity = dedup_identity(
                &metadata.block_hash,
                crate::pipeline::Trigger::Inline,
                crate::llm::LlmRequestKind::FactCheck,
            );

            let store_arc = store.inner().clone();
            let provider_arc = llm_state.provider();
            return match dispatch_task(app, store_arc, provider_arc, request, metadata, identity)
            {
                Ok(task_id) => Ok(Some(task_id)),
                Err(e) if e.starts_with("duplicate") => {
                    // Defensive: the Inline path cannot currently produce a
                    // duplicate error (dedup applies to Automatic triggers
                    // only), so the original task's own result is all the
                    // frontend needs.
                    Ok(None)
                }
                Err(e) => Err(e),
            };
        }
        crate::pipeline::RoutingDecision::Research => {
            // Inline /research: dispatch a research task. route_block returns
            // Research only when a parsed command is present, so the command
            // is guaranteed to exist here. The goal is the parsed focus text;
            // the selection is empty (no editor selection on the inline path);
            // the block text is the document.
            let parsed = command
                .as_ref()
                .expect("Research decision implies a parsed command");
            let envelope =
                crate::pipeline::build_inline_envelope(&block, parsed, file_path.as_deref());
            let source_hash = source_hash_of(&file_path, &block_text)?;
            let large_model = config_state.large_model();
            let request = crate::pipeline::build_research_request(
                &parsed.focus_text,
                "",
                &block.text,
                &large_model,
            );
            let task_id = crate::pipeline::inline_task_id(&envelope);
            let metadata = TaskMetadata {
                task_id,
                file_path,
                block_hash: block.block_hash.clone(),
                source_hash,
                focus_start: Some(envelope.focus_ref.start),
                focus_end: Some(envelope.focus_ref.end),
                line_text_hash: Some(crate::pipeline::stable_hash(
                    &block.text[parsed.line_start..parsed.line_end],
                )),
                trigger: crate::pipeline::Trigger::Inline,
                status: TaskStatus::Queued,
                stale: false,
                error: None,
            };
            let identity = dedup_identity(
                &metadata.block_hash,
                crate::pipeline::Trigger::Inline,
                crate::llm::LlmRequestKind::Research,
            );

            let store_arc = store.inner().clone();
            let provider_arc = llm_state.provider();
            return match dispatch_task(app, store_arc, provider_arc, request, metadata, identity)
            {
                Ok(task_id) => Ok(Some(task_id)),
                Err(e) if e.starts_with("duplicate") => {
                    // Duplicate tasks are silently dropped because the
                    // original task is still in progress and will emit its own
                    // result.
                    Ok(None)
                }
                Err(e) => Err(e),
            };
        }
        crate::pipeline::RoutingDecision::Ignore => {
            // Roadmap: ignore-rule persistence is not implemented yet. The
            // command is recognized so the routing decision is authoritative;
            // persisting ignore rules arrives in a later phase.
            return Ok(None);
        }
        crate::pipeline::RoutingDecision::Skip => return Ok(None),
        crate::pipeline::RoutingDecision::NeedsExtraction
        | crate::pipeline::RoutingDecision::HighConfidenceLocal => {}
    }

    let small_model = config_state.small_model();
    let request = crate::pipeline::build_extraction_request(&block, &small_model);
    let block_hash = block.block_hash.clone();
    let source_hash = source_hash_of(&file_path, &block_text)?;
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
        focus_start: None,
        focus_end: None,
        line_text_hash: None,
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
            tracing::debug!(category = "task", "duplicate task suppressed");
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
    tracing::info!(category = "slash_command", "/fact-check");
    let small_model = config_state.small_model();
    let request = crate::pipeline::build_fact_check_request(
        &claim_text,
        &block_text,
        &heading_chain.join(" > "),
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
        focus_start: None,
        focus_end: None,
        line_text_hash: None,
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
    tracing::info!(category = "slash_command", "/research");
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
        focus_start: None,
        focus_end: None,
        line_text_hash: None,
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