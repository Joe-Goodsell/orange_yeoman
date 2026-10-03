use super::*;

// --- Enum serde ---

#[test]
fn event_type_serializes_to_snake_case() {
    assert_eq!(serde_json::to_value(EventType::WatcherFile).unwrap(), "watcher_file");
    assert_eq!(serde_json::to_value(EventType::BlockDiff).unwrap(), "block_diff");
    assert_eq!(serde_json::to_value(EventType::StoreUpdate).unwrap(), "store_update");
    assert_eq!(serde_json::to_value(EventType::LlmCall).unwrap(), "llm_call");
    assert_eq!(serde_json::to_value(EventType::ConfigReload).unwrap(), "config_reload");
}

#[test]
fn event_status_serializes_to_snake_case() {
    assert_eq!(serde_json::to_value(EventStatus::Queued).unwrap(), "queued");
    assert_eq!(serde_json::to_value(EventStatus::InFlight).unwrap(), "in_flight");
    assert_eq!(serde_json::to_value(EventStatus::Done).unwrap(), "done");
    assert_eq!(serde_json::to_value(EventStatus::Failed).unwrap(), "failed");
}

// --- BackendEvent camelCase shape ---

fn full_event() -> BackendEvent {
    BackendEvent {
        id: "task-1".to_string(),
        event_type: EventType::LlmCall,
        status: EventStatus::Done,
        created_at: 1000,
        ts: 2000,
        duration_ms: Some(500),
        model: Some("mock-provider-v1".to_string()),
        estimated_cost_usd: Some(0.0),
        final_cost_usd: Some(0.0),
        block: Some(BlockRef {
            file_path: Some("/repo/note.md".to_string()),
            block_hash: "abc123".to_string(),
            block_kind: Some("paragraph".to_string()),
            block_start: Some(10),
            block_end: Some(20),
            position_start: Some(12),
            position_end: Some(15),
            excerpt: Some("some text".to_string()),
        }),
        summary: "done".to_string(),
        detail: Some(serde_json::json!({"extra": 1})),
    }
}

#[test]
fn backend_event_field_names_are_camel_case() {
    let value = serde_json::to_value(full_event()).unwrap();
    let obj = value.as_object().expect("event serializes to an object");
    for key in [
        "id",
        "eventType",
        "status",
        "createdAt",
        "ts",
        "durationMs",
        "model",
        "estimatedCostUsd",
        "finalCostUsd",
        "block",
        "summary",
        "detail",
    ] {
        assert!(obj.contains_key(key), "missing key {key}");
    }
    let block = obj["block"].as_object().expect("block serializes to an object");
    for key in [
        "filePath",
        "blockHash",
        "blockKind",
        "blockStart",
        "blockEnd",
        "positionStart",
        "positionEnd",
        "excerpt",
    ] {
        assert!(block.contains_key(key), "missing block key {key}");
    }
    assert_eq!(obj["eventType"], "llm_call");
    assert_eq!(obj["status"], "done");
}

#[test]
fn option_fields_serialize_none_as_null_and_some_as_value() {
    let mut event = full_event();
    event.duration_ms = None;
    event.model = None;
    event.estimated_cost_usd = None;
    event.final_cost_usd = None;
    event.block = None;
    event.detail = None;
    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["durationMs"], serde_json::Value::Null);
    assert_eq!(value["model"], serde_json::Value::Null);
    assert_eq!(value["estimatedCostUsd"], serde_json::Value::Null);
    assert_eq!(value["finalCostUsd"], serde_json::Value::Null);
    assert_eq!(value["block"], serde_json::Value::Null);
    assert_eq!(value["detail"], serde_json::Value::Null);

    let event = full_event();
    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["durationMs"], 500);
    assert_eq!(value["estimatedCostUsd"], 0.0);
    assert_eq!(value["block"]["blockHash"], "abc123");
}

// --- BlockRef::from_block ---

#[test]
fn from_block_maps_hash_kind_offsets_and_excerpt() {
    let blocks = crate::pipeline::parse_markdown_blocks("# Title\n\nSome text here.");
    let block = &blocks[0];
    let reference = BlockRef::from_block(Some("/repo/note.md"), block);
    assert_eq!(reference.file_path.as_deref(), Some("/repo/note.md"));
    assert_eq!(reference.block_hash, crate::pipeline::stable_hash("# Title"));
    assert_eq!(reference.block_kind.as_deref(), Some("heading"));
    assert_eq!(reference.block_start, Some(0));
    assert_eq!(reference.block_end, Some(7));
    assert_eq!(reference.position_start, None);
    assert_eq!(reference.position_end, None);
    assert_eq!(reference.excerpt.as_deref(), Some("# Title"));
}

#[test]
fn from_block_none_path_leaves_file_path_null() {
    let blocks = crate::pipeline::parse_markdown_blocks("plain text");
    let reference = BlockRef::from_block(None, &blocks[0]);
    assert_eq!(reference.file_path, None);
}

#[test]
fn from_block_excerpt_collapses_whitespace_and_truncates_at_80() {
    let long = "lorem   ipsum  dolor ".repeat(20); // ~360 chars, well over 80
    let blocks = crate::pipeline::parse_markdown_blocks(&long);
    let reference = BlockRef::from_block(None, &blocks[0]);
    let excerpt = reference.excerpt.expect("excerpt present for non-empty text");
    assert_eq!(excerpt.chars().count(), 80);
    // Collapsed: no run of spaces survives.
    assert!(!excerpt.contains("  "));
    assert!(excerpt.starts_with("lorem ipsum dolor"));
}

#[test]
fn from_block_empty_text_has_no_excerpt() {
    // A block consisting only of whitespace yields an empty first line.
    let block = crate::pipeline::Block {
        id: None,
        kind: crate::pipeline::BlockKind::Paragraph,
        text: "   \n\n  ".to_string(),
        block_hash: "h".to_string(),
        heading_path: String::new(),
        char_start: 0,
        char_end: 7,
        excluded: false,
    };
    let reference = BlockRef::from_block(None, &block);
    assert_eq!(reference.excerpt, None);
}

// --- BlockRef::from_task_metadata ---

fn metadata() -> crate::tasks::TaskMetadata {
    crate::tasks::TaskMetadata {
        task_id: "t1".to_string(),
        file_path: Some("/repo/note.md".to_string()),
        block_hash: "deadbeef".to_string(),
        source_hash: "s".to_string(),
        focus_start: Some(12),
        focus_end: Some(18),
        line_text_hash: Some("line".to_string()),
        trigger: crate::pipeline::Trigger::Inline,
        status: crate::tasks::TaskStatus::Queued,
        stale: false,
        error: None,
    }
}

#[test]
fn from_task_metadata_maps_file_hash_and_focus() {
    let reference = BlockRef::from_task_metadata(&metadata());
    assert_eq!(reference.file_path.as_deref(), Some("/repo/note.md"));
    assert_eq!(reference.block_hash, "deadbeef");
    assert_eq!(reference.position_start, Some(12));
    assert_eq!(reference.position_end, Some(18));
}

#[test]
fn from_task_metadata_leaves_unknown_fields_null() {
    let reference = BlockRef::from_task_metadata(&metadata());
    assert_eq!(reference.block_kind, None);
    assert_eq!(reference.block_start, None);
    assert_eq!(reference.block_end, None);
    assert_eq!(reference.excerpt, None);
}

#[test]
fn from_task_metadata_none_focus_stays_null() {
    let mut meta = metadata();
    meta.focus_start = None;
    meta.focus_end = None;
    meta.file_path = None;
    let reference = BlockRef::from_task_metadata(&meta);
    assert_eq!(reference.file_path, None);
    assert_eq!(reference.position_start, None);
    assert_eq!(reference.position_end, None);
}

// --- next_event_id ---

#[test]
fn next_event_id_uses_prefix_and_increases() {
    let first = next_event_id("watcher_file");
    let second = next_event_id("watcher_file");
    assert!(first.starts_with("watcher_file-"));
    assert_ne!(first, second);
    let first_num: u64 = first.rsplit('-').next().unwrap().parse().unwrap();
    let second_num: u64 = second.rsplit('-').next().unwrap().parse().unwrap();
    assert_eq!(second_num, first_num + 1);
}

// --- now_ms ---

#[test]
fn now_ms_is_recent_epoch_time() {
    let now = now_ms();
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    // The call happens microseconds apart; allow a generous skew.
    assert!(now <= since_epoch + 5000);
    assert!(now > 1_700_000_000_000); // well past 2023
}