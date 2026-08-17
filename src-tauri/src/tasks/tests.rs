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

// Documents why submit_auto_task hashes the whole file, not the block text:
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

#[test]
fn task_metadata_serializes_to_camel_case() {
    let metadata = TaskMetadata {
        task_id: "t1".to_string(),
        file_path: Some("p".to_string()),
        block_hash: "b".to_string(),
        source_hash: "s".to_string(),
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
        stale: false,
        result: Some(serde_json::json!({"schema_version": 1})),
        error: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("\"taskId\""));
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
