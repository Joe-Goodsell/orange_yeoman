use super::*;

#[test]
fn task_id_is_deterministic() {
    let a = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::Extraction,
    );
    let b = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::Extraction,
    );
    assert_eq!(a, b);
}

#[test]
fn task_id_differs_for_different_kinds() {
    let extraction = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::Extraction,
    );
    let fact_check = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::FactCheck,
    );
    assert_ne!(extraction, fact_check);
}

#[test]
fn task_id_differs_for_different_triggers() {
    let automatic = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::Automatic,
        &crate::llm::LlmRequestKind::Extraction,
    );
    let fact_check = task_id_from_identity(
        "abc123",
        &crate::pipeline::Trigger::FactCheck,
        &crate::llm::LlmRequestKind::Extraction,
    );
    assert_ne!(automatic, fact_check);
}

// Two fact-checks of different claims in the same block must not share a task
// id: the claim hash disambiguator keeps them from overwriting each other in
// the store.
#[test]
fn fact_check_task_id_includes_claim_hash() {
    let a = fact_check_task_id("abc123", "claim one");
    let b = fact_check_task_id("abc123", "claim two");
    assert_ne!(a, b);
}

#[test]
fn research_task_id_includes_goal_hash() {
    let a = research_task_id("abc123", "goal one");
    let b = research_task_id("abc123", "goal two");
    assert_ne!(a, b);
}

#[test]
fn dedup_identity_equality() {
    let a = dedup_identity(
        "abc123",
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    );
    let b = dedup_identity(
        "abc123",
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    );
    assert_eq!(a, b);
}

#[test]
fn dedup_identity_inequality() {
    let a = dedup_identity(
        "abc123",
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    );
    let b = dedup_identity(
        "def456",
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    );
    assert_ne!(a, b);
}

#[test]
fn is_result_stale_no_file_path_returns_false() {
    assert!(!is_result_stale(
        None,
        "hash",
        |_| Ok("content".to_string())
    ));
}

#[test]
fn is_result_stale_file_missing_returns_true() {
    assert!(is_result_stale(Some("path"), "hash", |_| Err(())));
}

#[test]
fn is_result_stale_hash_matches_returns_false() {
    let hash = crate::pipeline::stable_hash("hello");
    assert!(!is_result_stale(Some("path"), &hash, |_| Ok(
        "hello".to_string()
    )));
}

#[test]
fn is_result_stale_hash_differs_returns_true() {
    let hash = crate::pipeline::stable_hash("hello");
    assert!(is_result_stale(Some("path"), &hash, |_| Ok(
        "world".to_string()
    )));
}

// Documents why submit_block hashes the whole file, not the block text:
// the stale check re-reads the file and hashes its content, so only a
// whole-file hash keeps the result fresh.
#[test]
fn auto_task_source_hash_uses_file_content() {
    let file_hash = crate::pipeline::stable_hash("whole file content");
    assert!(!is_result_stale(Some("path"), &file_hash, |_| Ok(
        "whole file content".to_string()
    )));
    let block_hash = crate::pipeline::stable_hash("block text");
    assert!(is_result_stale(Some("path"), &block_hash, |_| Ok(
        "whole file content".to_string()
    )));
}

// Non-inline tasks keep whole-file staleness: a whole-file hash mismatch is
// stale even when the target block is unchanged. This is the behavior
// is_inline_result_stale refines for inline tasks, whose staleness keys on the
// raw command line's hash instead.
#[test]
fn is_result_stale_whole_file_hash_difference_returns_true() {
    let file_hash = crate::pipeline::stable_hash(
        "The value is 450 celsius according to Smith.\n\nThe value is 5 kg.",
    );
    assert!(is_result_stale(Some("path"), &file_hash, |_| Ok(
        "The value is 450 celsius according to Smith.\n\nThe value is 6 kg."
            .to_string()
    )));
}

// --- inline line-granular staleness tests ---
//
// is_inline_result_stale re-parses the file and keys staleness on the hash of
// the raw command line text, so edits to sibling lines in the same block or to
// other blocks never invalidate an inline command result. Only an edit to the
// command line itself makes it stale.

// The stable hash of the raw command line text of the first slash command
// found in the content, mirroring how submit_block stamps line_text_hash.
fn inline_line_hash(content: &str) -> String {
    let blocks = crate::pipeline::parse_markdown_blocks(content);
    for block in blocks.iter().filter(|b| !b.excluded) {
        if let Some(parsed) = crate::pipeline::parse_slash_command_in_block(block) {
            return crate::pipeline::stable_hash(&block.text[parsed.line_start..parsed.line_end]);
        }
    }
    panic!("content should contain a slash command");
}

#[test]
fn is_inline_result_stale_no_file_path_returns_false() {
    assert!(!is_inline_result_stale(None, "hash", |_| Ok(
        "content".to_string()
    )));
}

#[test]
fn is_inline_result_stale_file_missing_returns_true() {
    assert!(is_inline_result_stale(Some("path"), "hash", |_| Err(())));
}

// A command line with the same hash still present in the file keeps the result
// fresh.
#[test]
fn is_inline_result_stale_command_line_present_returns_false() {
    let content = "The value is 450 celsius according to Smith.\n/fact-check amperes are tricky."
        .to_string();
    let blocks = crate::pipeline::parse_markdown_blocks(&content);
    assert_eq!(blocks.len(), 1);
    let line_hash = inline_line_hash(&content);
    assert!(!is_inline_result_stale(Some("path"), &line_hash, |_| Ok(
        content.clone()
    )));
}

// When the command line itself is edited, its raw text hash no longer matches
// any line in the file and the result is stale.
#[test]
fn is_inline_result_stale_command_line_edited_returns_true() {
    let original = "/fact-check amperes are tricky.".to_string();
    let line_hash = inline_line_hash(&original);
    let edited = "/fact-check amperes are harder.".to_string();
    assert!(is_inline_result_stale(Some("path"), &line_hash, |_| Ok(
        edited.clone()
    )));
}

// The key granularity case: an edit to a DIFFERENT block in the same file must
// not invalidate the target command line's result. The command line's hash
// still matches, so the result is fresh.
#[test]
fn is_inline_result_stale_other_block_edited_returns_false() {
    let original =
        "The value is 450 celsius according to Smith.\n/fact-check amperes are tricky.\n\nThe value is 5 kg."
            .to_string();
    let blocks = crate::pipeline::parse_markdown_blocks(&original);
    assert_eq!(blocks.len(), 2);
    let line_hash = inline_line_hash(&original);

    let edited = "The value is 450 celsius according to Smith.\n/fact-check amperes are tricky.\n\nThe value is 6 kg."
        .to_string();
    assert!(!is_inline_result_stale(Some("path"), &line_hash, |_| Ok(
        edited.clone()
    )));
}

// The line-granular case inside a single block: the command line shares its
// paragraph block with sibling lines. Editing a sibling line leaves the
// command line untouched, so the result is NOT stale; editing the command line
// itself makes it stale.
#[test]
fn is_inline_result_stale_same_block_sibling_line_edited_returns_false() {
    let original = "Sibling line one.\n/fact-check amperes are tricky.\nSibling line three."
        .to_string();
    let blocks = crate::pipeline::parse_markdown_blocks(&original);
    assert_eq!(blocks.len(), 1);
    let line_hash = inline_line_hash(&original);

    let sibling_edited =
        "Sibling line ONE changed.\n/fact-check amperes are tricky.\nSibling line three."
            .to_string();
    assert!(!is_inline_result_stale(Some("path"), &line_hash, |_| Ok(
        sibling_edited.clone()
    )));

    let command_edited = "Sibling line one.\n/fact-check amperes are harder.\nSibling line three."
        .to_string();
    assert!(is_inline_result_stale(Some("path"), &line_hash, |_| Ok(
        command_edited.clone()
    )));
}

#[test]
fn task_metadata_serializes_to_camel_case() {
    let metadata = TaskMetadata {
        task_id: "t1".to_string(),
        file_path: Some("p".to_string()),
        block_hash: "b".to_string(),
        source_hash: "s".to_string(),
        focus_start: None,
        focus_end: None,
        line_text_hash: None,
        trigger: crate::pipeline::Trigger::Automatic,
        status: TaskStatus::Completed,
        stale: false,
        error: None,
    };
    let json = serde_json::to_string(&metadata).unwrap();
    assert!(json.contains("\"filePath\""));
    assert!(json.contains("\"blockHash\""));
    assert!(json.contains("\"sourceHash\""));
    assert!(json.contains("\"completed\""));
    assert!(!json.contains("file_path"));
    assert!(!json.contains("block_hash"));
    // The optional focus span and line hash are absent from JSON when None, so
    // existing frontend consumers that do not know about them stay compatible.
    assert!(!json.contains("focusStart"));
    assert!(!json.contains("focusEnd"));
    assert!(!json.contains("lineTextHash"));
}

// The inline focus span serializes as camelCase only when present.
#[test]
fn task_metadata_focus_span_serializes_camel_case_when_present() {
    let metadata = TaskMetadata {
        task_id: "t1".to_string(),
        file_path: Some("p".to_string()),
        block_hash: "b".to_string(),
        source_hash: "s".to_string(),
        focus_start: Some(10),
        focus_end: Some(42),
        line_text_hash: Some("abc123".to_string()),
        trigger: crate::pipeline::Trigger::Inline,
        status: TaskStatus::Completed,
        stale: false,
        error: None,
    };
    let json = serde_json::to_string(&metadata).unwrap();
    assert!(json.contains("\"focusStart\":10"));
    assert!(json.contains("\"focusEnd\":42"));
    assert!(json.contains("\"lineTextHash\":\"abc123\""));
    assert!(!json.contains("focus_start"));
    assert!(!json.contains("focus_end"));
    assert!(!json.contains("line_text_hash"));
}

#[test]
fn task_event_serializes_camel_case() {
    let event = TaskEvent {
        task_id: "t1".to_string(),
        status: TaskStatus::Queued,
        kind: crate::llm::LlmRequestKind::Research,
        stale: false,
        error: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"taskId\""));
    assert!(json.contains("\"status\""));
}

#[test]
fn task_result_serializes_camel_case() {
    let result = TaskResult {
        task_id: "t1".to_string(),
        status: TaskStatus::Completed,
        kind: crate::llm::LlmRequestKind::Research,
        stale: false,
        result: Some(serde_json::json!({"schema_version": 1})),
        error: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"taskId\""));
    assert!(json.contains("\"status\":\"completed\""));
    assert!(json.contains("\"kind\":\"research\""));
    assert!(json.contains("\"schema_version\""));
}

#[test]
fn task_status_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&TaskStatus::Queued).unwrap(),
        "\"queued\""
    );
    assert_eq!(
        serde_json::to_string(&TaskStatus::Running).unwrap(),
        "\"running\""
    );
    assert_eq!(
        serde_json::to_string(&TaskStatus::Completed).unwrap(),
        "\"completed\""
    );
    assert_eq!(
        serde_json::to_string(&TaskStatus::Failed).unwrap(),
        "\"failed\""
    );
    assert_eq!(
        serde_json::to_string(&TaskStatus::Stale).unwrap(),
        "\"stale\""
    );
}

// --- TaskStore and scrub_error tests ---

// dispatch_task is intentionally not unit-tested here. It requires a real
// tauri::AppHandle, which cannot be constructed in a unit test without the
// tauri "test" feature enabled in Cargo.toml. Enabling that feature is out of
// scope for this slice, so dispatch_task is exercised later through a Tauri
// command integration test once the command surface exists.

fn task_metadata(task_id: &str, trigger: crate::pipeline::Trigger) -> TaskMetadata {
    TaskMetadata {
        task_id: task_id.to_string(),
        file_path: None,
        block_hash: "b".to_string(),
        source_hash: "s".to_string(),
        focus_start: None,
        focus_end: None,
        line_text_hash: None,
        trigger,
        status: TaskStatus::Queued,
        stale: false,
        error: None,
    }
}

fn extraction_identity() -> DedupIdentity {
    dedup_identity(
        "b",
        crate::pipeline::Trigger::Automatic,
        crate::llm::LlmRequestKind::Extraction,
    )
}

#[test]
fn task_store_insert_and_get() {
    let store = TaskStore::new();
    let metadata = task_metadata("t1", crate::pipeline::Trigger::Automatic);
    let identity = extraction_identity();
    assert_eq!(store.insert(metadata, &identity), Some("t1".to_string()));
    let got = store.get("t1").expect("task should exist");
    assert_eq!(got.task_id, "t1");
    assert_eq!(got.status, TaskStatus::Queued);
}

#[test]
fn task_store_dedup_blocks_duplicate_automatic() {
    let store = TaskStore::new();
    let identity = extraction_identity();
    let first = task_metadata("t1", crate::pipeline::Trigger::Automatic);
    let second = TaskMetadata {
        task_id: "t2".to_string(),
        ..first.clone()
    };
    assert_eq!(store.insert(first, &identity), Some("t1".to_string()));
    assert_eq!(store.insert(second, &identity), None);
}

#[test]
fn task_store_explicit_commands_not_deduped() {
    let store = TaskStore::new();
    let identity = dedup_identity(
        "b",
        crate::pipeline::Trigger::FactCheck,
        crate::llm::LlmRequestKind::FactCheck,
    );
    let first = task_metadata("t1", crate::pipeline::Trigger::FactCheck);
    let second = TaskMetadata {
        task_id: "t2".to_string(),
        ..first.clone()
    };
    assert_eq!(store.insert(first, &identity), Some("t1".to_string()));
    assert_eq!(store.insert(second, &identity), Some("t2".to_string()));
}

#[test]
fn task_store_update_changes_status() {
    let store = TaskStore::new();
    let metadata = task_metadata("t1", crate::pipeline::Trigger::Automatic);
    let identity = extraction_identity();
    store.insert(metadata, &identity);
    store.update("t1", TaskStatus::Running, false, None);
    let got = store.get("t1").expect("task should exist");
    assert_eq!(got.status, TaskStatus::Running);
    assert!(!got.stale);
}

#[test]
fn task_store_update_sets_error() {
    let store = TaskStore::new();
    let metadata = task_metadata("t1", crate::pipeline::Trigger::Automatic);
    let identity = extraction_identity();
    store.insert(metadata, &identity);
    store.update("t1", TaskStatus::Failed, false, Some("err".to_string()));
    let got = store.get("t1").expect("task should exist");
    assert_eq!(got.status, TaskStatus::Failed);
    assert_eq!(got.error, Some("err".to_string()));
}

#[test]
fn task_store_clear_dedup_allows_reinsert() {
    let store = TaskStore::new();
    let identity = extraction_identity();
    let first = task_metadata("t1", crate::pipeline::Trigger::Automatic);
    let second = TaskMetadata {
        task_id: "t2".to_string(),
        ..first.clone()
    };
    assert_eq!(store.insert(first, &identity), Some("t1".to_string()));
    store.clear_dedup(&identity);
    assert_eq!(store.insert(second, &identity), Some("t2".to_string()));
}

#[test]
fn task_store_get_missing_returns_none() {
    let store = TaskStore::new();
    assert!(store.get("nonexistent").is_none());
}

#[test]
fn scrub_error_redacts_sk_tokens() {
    let cleaned = scrub_error("error with sk-abc123 token");
    assert!(!cleaned.contains("sk-abc123"));
    assert!(cleaned.contains("[REDACTED]"));
}

#[test]
fn scrub_error_caps_length() {
    let cleaned = scrub_error(&"x".repeat(300));
    assert!(cleaned.len() <= 200);
}

#[test]
fn scrub_error_strips_control_chars() {
    let cleaned = scrub_error("error\x00\x01text");
    assert!(!cleaned.contains('\x00'));
    assert!(!cleaned.contains('\x01'));
}

// --- inline slash-command dispatch tests ---
//
// submit_block is a tauri::command and needs a real AppHandle, so the
// inline dispatch is verified through its pure pieces in the exact order the
// command wires them: parse the block, build the envelope, derive the task id.
// The routing contract is covered by the route_block tests below: an inline
// /ignore command makes submit_block return Ok(None), and inline /fact-check
// and /research commands are the dispatch-producing paths.

// A paragraph block with a stable hash, matching the pipeline test helper.
fn command_block(text: &str) -> crate::pipeline::MarkdownBlock {
    crate::pipeline::MarkdownBlock {
        kind: crate::pipeline::BlockKind::Paragraph,
        text: text.to_string(),
        start: 0,
        end: text.len(),
        heading_chain: vec![],
        excluded: false,
        block_hash: crate::pipeline::stable_hash(text),
    }
}

// An inline /fact-check block parses to a FactCheck command whose focus text
// is the claim; the envelope carries Trigger::Inline, which is what
// submit_block stamps on the TaskMetadata.
#[test]
fn inline_fact_check_parses_claim_and_inline_trigger() {
    let block = command_block("/fact-check some claim");
    let parsed = crate::pipeline::parse_slash_command_in_block(&block)
        .expect("command should parse");
    assert_eq!(parsed.command, crate::pipeline::SlashCommand::FactCheck);
    assert_eq!(parsed.focus_text, "some claim");
    let envelope =
        crate::pipeline::build_inline_envelope(&block, &parsed, Some("notes/a.md"));
    assert_eq!(envelope.trigger, crate::pipeline::Trigger::Inline);
    assert_eq!(envelope.focus_text, "some claim");
    assert_eq!(envelope.focus_ref.file.as_deref(), Some("notes/a.md"));
}

// The inline task id tracks the claim text: two inline /fact-check commands
// with different claims get distinct ids, so the task store keeps them apart.
#[test]
fn inline_fact_check_task_id_distinct_for_different_claims() {
    let b1 = command_block("/fact-check some claim");
    let b2 = command_block("/fact-check a different claim");
    let e1 = crate::pipeline::build_inline_envelope(
        &b1,
        &crate::pipeline::parse_slash_command_in_block(&b1).unwrap(),
        None,
    );
    let e2 = crate::pipeline::build_inline_envelope(
        &b2,
        &crate::pipeline::parse_slash_command_in_block(&b2).unwrap(),
        None,
    );
    assert_ne!(
        crate::pipeline::inline_task_id(&e1),
        crate::pipeline::inline_task_id(&e2)
    );
}

// The inline path must not collide with the explicit path: for the same block
// and claim, inline_task_id and fact_check_task_id / research_task_id produce
// different ids, so the two dispatch paths never overwrite each other in the
// task store. The explicit paths themselves are covered by
// fact_check_task_id_includes_claim_hash and research_task_id_includes_goal_hash.
#[test]
fn inline_fact_check_task_id_differs_from_explicit() {
    let block = command_block("/fact-check some claim");
    let parsed = crate::pipeline::parse_slash_command_in_block(&block).unwrap();
    let envelope = crate::pipeline::build_inline_envelope(&block, &parsed, None);
    let inline_id = crate::pipeline::inline_task_id(&envelope);
    assert_ne!(
        inline_id,
        fact_check_task_id(&block.block_hash, &parsed.focus_text)
    );
    assert_ne!(
        inline_id,
        research_task_id(&block.block_hash, &parsed.focus_text)
    );
}

// An inline /ignore command overrides strong factual signals and routes to
// Ignore; submit_block returns Ok(None) for that decision (roadmap).
#[test]
fn inline_ignore_routes_to_ignore_no_dispatch() {
    let block = command_block("/ignore\nThe value is 450 celsius according to Smith.");
    let parsed = crate::pipeline::parse_slash_command_in_block(&block).unwrap();
    assert_eq!(parsed.command, crate::pipeline::SlashCommand::Ignore);
    assert_eq!(
        crate::pipeline::route_block(&block, Some(&parsed)),
        crate::pipeline::RoutingDecision::Ignore
    );
}

// An inline /research command routes to Research, the dispatch-producing
// path for submit_block.
#[test]
fn inline_research_routes_to_research_dispatches() {
    let block = command_block("/research what happened at the battle of Hastings");
    let parsed = crate::pipeline::parse_slash_command_in_block(&block).unwrap();
    assert_eq!(parsed.command, crate::pipeline::SlashCommand::Research);
    assert_eq!(
        crate::pipeline::route_block(&block, Some(&parsed)),
        crate::pipeline::RoutingDecision::Research
    );
}

// --- llm_call_model_label tests ---
//
// The llm_call debug event shows the provider's fixed model (the mock)
// overriding the request's model hint; the real provider serves the request's
// configured model; an unset model shows as "<unset>".

fn research_request_with_model(model: Option<&str>) -> crate::llm::LlmRequest {
    crate::llm::LlmRequest {
        kind: crate::llm::LlmRequestKind::Research,
        model: model.map(|m| m.to_string()),
        system_prompt: None,
        user_prompt: "prompt".to_string(),
        max_tokens: None,
    }
}

// The mock provider's fixed identity overrides even a configured request model.
#[test]
fn llm_call_model_label_mock_overrides_request_model() {
    let provider = crate::llm::select_provider(true);
    let request = research_request_with_model(Some("configured-model"));
    assert_eq!(
        llm_call_model_label(provider.as_ref(), &request),
        "mock-provider-v1"
    );
}

// The mock provider's fixed identity also covers an unset request model.
#[test]
fn llm_call_model_label_mock_with_unset_model() {
    let provider = crate::llm::select_provider(true);
    let request = research_request_with_model(None);
    assert_eq!(
        llm_call_model_label(provider.as_ref(), &request),
        "mock-provider-v1"
    );
}

// The real provider serves the request's configured model.
#[test]
fn llm_call_model_label_real_uses_request_model() {
    let provider = crate::llm::select_provider(false);
    let request = research_request_with_model(Some("deepseek-chat"));
    assert_eq!(
        llm_call_model_label(provider.as_ref(), &request),
        "deepseek-chat"
    );
}

// An empty request model on the real provider shows as "<unset>".
#[test]
fn llm_call_model_label_real_empty_model_is_unset() {
    let provider = crate::llm::select_provider(false);
    let request = research_request_with_model(Some(""));
    assert_eq!(
        llm_call_model_label(provider.as_ref(), &request),
        "<unset>"
    );
}

// An absent request model on the real provider shows as "<unset>".
#[test]
fn llm_call_model_label_real_unset_model_is_unset() {
    let provider = crate::llm::select_provider(false);
    let request = research_request_with_model(None);
    assert_eq!(
        llm_call_model_label(provider.as_ref(), &request),
        "<unset>"
    );
}
