use super::*;
use crate::incremental::diff_block_lists;
use crate::pipeline::{stable_hash, Block, BlockKind};
use std::fs;
use std::path::Path;

// --- helpers ---------------------------------------------------------------

fn conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    migrate(&conn).unwrap();
    conn
}

fn stored(id: i64, hash: &str, start: usize) -> Block {
    Block {
        id: Some(id),
        block_hash: hash.to_string(),
        text: String::new(),
        kind: BlockKind::Paragraph,
        heading_path: String::new(),
        char_start: start,
        char_end: start + 10,
        excluded: false,
    }
}

fn nblock(hash: &str, start: usize) -> Block {
    Block {
        id: None,
        block_hash: hash.to_string(),
        text: hash.to_string(),
        kind: BlockKind::Paragraph,
        heading_path: String::new(),
        char_start: start,
        char_end: start + 10,
        excluded: false,
    }
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn count_where(conn: &Connection, sql: &str, param: &str) -> i64 {
    conn.query_row(sql, params![param], |row| row.get(0)).unwrap()
}

/// Insert a concept with one occurrence on the given block. Returns the
/// concept id.
fn insert_concept_with_occurrence(conn: &Connection, block_id: i64, name: &str) -> i64 {
    conn.execute(
        "INSERT INTO concepts (type, normalized_name, first_seen) VALUES ('entity', ?1, 1)",
        params![name],
    )
    .unwrap();
    let concept_id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO occurrences (concept_id, block_id, start, end, surface) \
         VALUES (?1, ?2, 0, 5, ?3)",
        params![concept_id, block_id, name],
    )
    .unwrap();
    concept_id
}

// --- migration and row operations ------------------------------------------

#[test]
fn migration_is_idempotent() {
    let mut conn = conn();
    migrate(&conn).unwrap();
    migrate(&conn).unwrap();
    // The schema is usable after the double migration.
    let id = insert_block(&conn, Path::new("notes/a.md"), &nblock("h1", 0)).unwrap();
    assert_eq!(id, 1);
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 1);
}

#[test]
fn insert_and_load_block_roundtrip() {
    let mut conn = conn();
    let mut new = nblock("h1", 0);
    new.heading_path = "H1 > H2".to_string();
    new.excluded = true;
    new.kind = BlockKind::CodeFence;
    let id = insert_block(&conn, Path::new("notes/a.md"), &new).unwrap();
    let rows = load_stored_blocks(&conn, Path::new("notes/a.md"));
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, Some(id));
    assert_eq!(row.block_hash, "h1");
    assert_eq!(row.heading_path, "H1 > H2");
    assert_eq!(row.char_start, 0);
    assert_eq!(row.char_end, 10);
    assert_eq!(row.kind, BlockKind::CodeFence);
    assert_eq!(row.text, "");
    assert!(row.excluded);
}

#[test]
fn delete_block_cascades_occurrences_and_orphans_shared_survives() {
    let conn = conn();
    let b1 = insert_block(&conn, Path::new("notes/a.md"), &nblock("a", 0)).unwrap();
    let b2 = insert_block(&conn, Path::new("notes/a.md"), &nblock("b", 10)).unwrap();
    let shared = insert_concept_with_occurrence(&conn, b1, "shared");
    // A second occurrence of the same concept on the other block.
    conn.execute(
        "INSERT INTO occurrences (concept_id, block_id, start, end, surface) VALUES (?1, ?2, 0, 5, 'shared')",
        params![shared, b2],
    )
    .unwrap();

    // Deleting one block cascades its occurrence but the shared concept stays.
    delete_block(&conn, b1).unwrap();
    delete_orphan_concepts(&conn).unwrap();
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM concepts WHERE id = ?1", &shared.to_string()), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM occurrences"), 1);

    // Deleting the last block orphans the concept.
    delete_block(&conn, b2).unwrap();
    delete_orphan_concepts(&conn).unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM concepts"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM occurrences"), 0);
}

#[test]
fn remove_all_blocks_for_path_clears_only_that_path() {
    let conn = conn();
    insert_block(&conn, Path::new("notes/a.md"), &nblock("a", 0)).unwrap();
    insert_block(&conn, Path::new("notes/b.md"), &nblock("b", 0)).unwrap();
    let mut conn = conn;
    remove_all_blocks_for_path(&mut conn, Path::new("notes/a.md"));
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 0);
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/b.md")).len(), 1);
}

// Re-linking a renamed file points every block row at the new path. Row ids,
// hashes, and occurrences stay attached; nothing is enqueued.
#[test]
fn relink_blocks_repoints_rows_and_keeps_occurrences() {
    let conn = conn();
    let b1 = insert_block(&conn, Path::new("notes/a.md"), &nblock("a", 0)).unwrap();
    let b2 = insert_block(&conn, Path::new("notes/a.md"), &nblock("b", 10)).unwrap();
    insert_concept_with_occurrence(&conn, b1, "kept");

    let updated = relink_blocks(&conn, Path::new("notes/a.md"), Path::new("notes/b.md")).unwrap();
    assert_eq!(updated, 2);

    // Both rows now live under the new path, ids and hashes preserved.
    let rows = load_stored_blocks(&conn, Path::new("notes/b.md"));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, Some(b1));
    assert_eq!(rows[0].block_hash, "a");
    assert_eq!(rows[1].id, Some(b2));
    assert_eq!(rows[1].block_hash, "b");
    // The occurrence stayed attached to its row; relink never enqueues.
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM occurrences"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 0);
    // The old path no longer has any rows.
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 0);
}

// --- apply_diff ------------------------------------------------------------

// A moved block keeps its row id and its occurrences, and is not enqueued.
#[test]
fn apply_moved_updates_position_without_enqueue() {
    let conn = conn();
    let id = insert_block(&conn, Path::new("notes/a.md"), &nblock("h", 0)).unwrap();
    insert_concept_with_occurrence(&conn, id, "kept");

    let old = Block {
        id: Some(id),
        char_start: 0,
        ..stored(id, "h", 0)
    };
    let mut new = nblock("h", 20);
    new.heading_path = "New > Heading".to_string();
    let diff = FileDiff {
        changes: vec![BlockChange::Moved {
            stored: old,
            new: new.clone(),
        }],
        ..FileDiff::default()
    };

    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.enqueued, 0);

    let rows = load_stored_blocks(&conn, Path::new("notes/a.md"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, Some(id));
    assert_eq!(rows[0].char_start, 20);
    assert_eq!(rows[0].char_end, 30);
    assert_eq!(rows[0].heading_path, "New > Heading");
    // Occurrences stay attached to the same row.
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM occurrences"), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 0);
}

// A removed block cascades its occurrences; only orphaned concepts are
// deleted, shared ones survive.
#[test]
fn apply_removed_cascades_and_cleans_orphans() {
    let conn = conn();
    let b1 = insert_block(&conn, Path::new("notes/a.md"), &nblock("a", 0)).unwrap();
    let b2 = insert_block(&conn, Path::new("notes/a.md"), &nblock("b", 10)).unwrap();
    let orphan = insert_concept_with_occurrence(&conn, b1, "only_in_a");
    let shared = insert_concept_with_occurrence(&conn, b1, "shared");
    conn.execute(
        "INSERT INTO occurrences (concept_id, block_id, start, end, surface) VALUES (?1, ?2, 0, 5, 'shared')",
        params![shared, b2],
    )
    .unwrap();

    let diff = FileDiff {
        changes: vec![BlockChange::Removed {
            stored: stored(b1, "a", 0),
        }],
        ..FileDiff::default()
    };

    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.deleted, 1);
    assert_eq!(stats.enqueued, 0);

    // b1's row is gone; b2 remains.
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 1);
    // The orphaned concept is gone, the shared one survives with its other
    // occurrence.
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM concepts WHERE id = ?1", &orphan.to_string()),
        0
    );
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM concepts WHERE id = ?1", &shared.to_string()),
        1
    );
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM occurrences WHERE concept_id = ?1", &shared.to_string()), 1);
}

// A changed block replaces the old row (occurrences cascade) and enqueues with
// reason 'changed'.
#[test]
fn apply_changed_replaces_and_enqueues() {
    let conn = conn();
    let old_id = insert_block(&conn, Path::new("notes/a.md"), &nblock("old", 0)).unwrap();
    insert_concept_with_occurrence(&conn, old_id, "doomed");

    let diff = FileDiff {
        changes: vec![BlockChange::Changed {
            old: stored(old_id, "old", 0),
            new: nblock("new", 0),
        }],
        ..FileDiff::default()
    };

    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.stored, 1);
    assert_eq!(stats.deleted, 1);
    assert_eq!(stats.enqueued, 1);

    let rows = load_stored_blocks(&conn, Path::new("notes/a.md"));
    assert_eq!(rows.len(), 1);
    // The old row is gone and the new row carries the new hash. SQLite may
    // reuse the old rowid when no other rows exist, so the id is not part of
    // the identity here.
    assert_eq!(rows[0].block_hash, "new");
    // Old row deleted -> its occurrence cascaded and its concept orphaned.
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM occurrences"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM concepts"), 0);
    // Queue row carries the new hash with reason 'changed'.
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM pending_work WHERE reason = 'changed' AND block_hash = ?1", "new"),
        1
    );
}

#[test]
fn apply_added_inserts_and_enqueues() {
    let conn = conn();
    let diff = FileDiff {
        changes: vec![BlockChange::Added { new: nblock("fresh", 0) }],
        ..FileDiff::default()
    };
    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.stored, 1);
    assert_eq!(stats.enqueued, 1);
    let rows = load_stored_blocks(&conn, Path::new("notes/a.md"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].block_hash, "fresh");
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM pending_work WHERE reason = 'added' AND block_hash = ?1", "fresh"),
        1
    );
}

// Excluded blocks (front matter, code fences, HTML) are stored but never
// enqueued.
#[test]
fn apply_stores_excluded_without_enqueue() {
    let conn = conn();
    let mut block = nblock("fence", 0);
    block.excluded = true;
    block.kind = BlockKind::CodeFence;
    let diff = FileDiff {
        changes: vec![BlockChange::Added { new: block }],
        ..FileDiff::default()
    };
    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.stored, 1);
    assert_eq!(stats.enqueued, 0);
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 0);
}

// Blocks carrying an /ignore slash command are stored but never enqueued.
#[test]
fn apply_stores_ignore_block_without_enqueue() {
    let conn = conn();
    let text = "/ignore this paragraph entirely";
    let mut block = nblock("ignored", 0);
    block.text = text.to_string();
    block.block_hash = stable_hash(text);
    let diff = FileDiff {
        changes: vec![BlockChange::Added { new: block }],
        ..FileDiff::default()
    };
    let mut conn = conn;
    let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
    assert_eq!(stats.stored, 1);
    assert_eq!(stats.enqueued, 0);
    assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 1);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 0);
}

// Blocks carrying /fact-check or /research still enqueue; only /ignore and
// excluded blocks are filtered out.
#[test]
fn apply_enqueues_fact_check_and_research_blocks() {
    for command in ["/fact-check", "/research"] {
        let mut conn = conn();
        let text = format!("{command} this paragraph");
        let mut block = nblock(command, 0);
        block.text = text.clone();
        block.block_hash = stable_hash(&text);
        let diff = FileDiff {
            changes: vec![BlockChange::Added { new: block }],
            ..FileDiff::default()
        };
        let stats = apply_diff(&mut conn, Path::new("notes/a.md"), &diff);
        assert_eq!(stats.stored, 1, "command {command}");
        assert_eq!(stats.enqueued, 1, "command {command}");
        assert_eq!(load_stored_blocks(&conn, Path::new("notes/a.md")).len(), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 1);
    }
}

// Re-enqueueing the same block hash resets an existing queue row to pending
// and refreshes the reason.
#[test]
fn pending_work_conflict_resets_status() {
    let conn = conn();
    let path = Path::new("notes/a.md");
    enqueue_pending(&conn, path, "h1", "added").unwrap();
    conn.execute("UPDATE pending_work SET status = 'done'", []).unwrap();
    enqueue_pending(&conn, path, "h1", "changed").unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 1);
    let (status, reason): (String, String) = conn
        .query_row(
            "SELECT status, reason FROM pending_work WHERE block_hash = 'h1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "pending");
    assert_eq!(reason, "changed");
}

// --- process_file_content (end-to-end-ish) ---------------------------------

#[test]
fn process_file_content_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.md");
    let mut conn = conn();

    // 1. Write a file with three blocks and process it: three rows, three
    //    pending entries.
    let v1 = "# H1\n\nPara A.\n\nPara B.\n";
    fs::write(&path, v1).unwrap();
    let (diff1, stats1) = process_file_content(&mut conn, &path, v1);
    assert_eq!(diff1.added, 3);
    assert_eq!(diff1.changed, 0);
    assert_eq!(stats1.stored, 3);
    assert_eq!(stats1.enqueued, 3);
    assert_eq!(load_stored_blocks(&conn, &path).len(), 3);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 3);

    // 2. Edit one block and insert one: one Changed, one Added; positions of
    //    the survivors refresh; pending_work gains changed + added entries.
    let v2 = "# H1\n\nPara A EDITED.\n\nPara B.\n\nPara C.\n";
    fs::write(&path, v2).unwrap();
    let (diff2, stats2) = process_file_content(&mut conn, &path, v2);
    assert_eq!(diff2.unchanged, 2);
    assert_eq!(diff2.changed, 1);
    assert_eq!(diff2.added, 1);
    assert_eq!(diff2.moved, 0);
    assert_eq!(stats2.enqueued, 2);

    let rows = load_stored_blocks(&conn, &path);
    assert_eq!(rows.len(), 4);
    // Para B survived unchanged at the same index but its offset moved down.
    let para_b = rows.iter().find(|r| r.block_hash == stable_hash("Para B.")).unwrap();
    assert_eq!(para_b.char_start, 22);
    assert_eq!(para_b.char_end, 29);
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM pending_work WHERE reason = 'changed' AND block_hash = ?1", &stable_hash("Para A EDITED.")), 1);
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM pending_work WHERE reason = 'added' AND block_hash = ?1", &stable_hash("Para C.")), 1);

    // 3. Delete the edited block: its row and occurrence cascade, the orphaned
    //    concept is deleted, and the shared concept survives.
    let pa_id = rows
        .iter()
        .find(|r| r.block_hash == stable_hash("Para A EDITED."))
        .unwrap()
        .id
        .expect("stored row id");
    let h1_id = rows
        .iter()
        .find(|r| r.block_hash == stable_hash("# H1"))
        .unwrap()
        .id
        .expect("stored row id");
    let pc_id = rows
        .iter()
        .find(|r| r.block_hash == stable_hash("Para C."))
        .unwrap()
        .id
        .expect("stored row id");
    let orphan = insert_concept_with_occurrence(&conn, pa_id, "only_in_para_a");
    let shared = insert_concept_with_occurrence(&conn, h1_id, "across_blocks");
    conn.execute(
        "INSERT INTO occurrences (concept_id, block_id, start, end, surface) VALUES (?1, ?2, 0, 5, 'across_blocks')",
        params![shared, pc_id],
    )
    .unwrap();

    let v3 = "# H1\n\nPara B.\n\nPara C.\n";
    fs::write(&path, v3).unwrap();
    let (diff3, stats3) = process_file_content(&mut conn, &path, v3);
    assert_eq!(diff3.removed, 1);
    assert_eq!(diff3.moved, 2);
    assert_eq!(stats3.enqueued, 0);

    assert_eq!(load_stored_blocks(&conn, &path).len(), 3);
    // Para A EDITED is gone; its concept is orphaned and deleted.
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM concepts WHERE id = ?1", &orphan.to_string()),
        0
    );
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM occurrences WHERE concept_id = ?1", &orphan.to_string()), 0);
    // The shared concept keeps its two occurrences on the surviving blocks.
    assert_eq!(
        count_where(&conn, "SELECT COUNT(*) FROM concepts WHERE id = ?1", &shared.to_string()),
        1
    );
    assert_eq!(count_where(&conn, "SELECT COUNT(*) FROM occurrences WHERE concept_id = ?1", &shared.to_string()), 2);
}

// Diff-then-apply round trip: applying a diff produced by diff_block_lists and
// re-processing the same content yields an all-unchanged diff (idempotent).
#[test]
fn apply_then_reprocess_is_unchanged() {
    let mut conn = conn();
    let path = Path::new("notes/a.md");
    let content = "# T\n\nOne.\n\nTwo.\n";
    let (_, _) = process_file_content(&mut conn, path, content);
    let (second, stats) = process_file_content(&mut conn, path, content);
    assert_eq!(second.unchanged, 3);
    assert_eq!(second.added, 0);
    assert_eq!(stats.enqueued, 0);
    // Nothing duplicated on reprocess.
    assert_eq!(load_stored_blocks(&conn, path).len(), 3);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM pending_work"), 3);

    // diff_block_lists itself is pure: the same inputs always classify the
    // same way.
    let stored_blocks = load_stored_blocks(&conn, path);
    let parsed = crate::pipeline::parse_markdown_blocks(content);
    let third = diff_block_lists(&stored_blocks, &parsed);
    assert_eq!(third.unchanged, 3);
}