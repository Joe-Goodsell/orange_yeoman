use super::*;

// --- helpers ---------------------------------------------------------------

fn stored(id: i64, hash: &str, start: usize) -> StoredBlock {
    StoredBlock {
        id,
        file_path: "notes/a.md".to_string(),
        block_hash: hash.to_string(),
        heading_path: String::new(),
        char_start: start,
        char_end: start + 10,
        kind: "paragraph".to_string(),
        excluded: false,
    }
}

fn new_block(hash: &str, start: usize) -> NewBlock {
    NewBlock {
        block_hash: hash.to_string(),
        text: hash.to_string(),
        kind: BlockKind::Paragraph,
        heading_path: String::new(),
        char_start: start,
        char_end: start + 10,
        excluded: false,
    }
}

/// Compress a diff into its classification sequence for readable assertions.
fn classes(diff: &FileDiff) -> Vec<&'static str> {
    diff.changes
        .iter()
        .map(|c| match c {
            BlockChange::Unchanged { .. } => "unchanged",
            BlockChange::Added { .. } => "added",
            BlockChange::Changed { .. } => "changed",
            BlockChange::Moved { .. } => "moved",
            BlockChange::Removed { .. } => "removed",
        })
        .collect()
}

// --- tests -----------------------------------------------------------------

// Pure unchanged file: every block matches at the same index.
#[test]
fn identical_lists_are_all_unchanged() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![new_block("a", 0), new_block("b", 10), new_block("c", 20)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["unchanged", "unchanged", "unchanged"]);
    assert_eq!(diff.unchanged, 3);
    assert_eq!(diff.added, 0);
    assert_eq!(diff.changed, 0);
    assert_eq!(diff.moved, 0);
    assert_eq!(diff.removed, 0);
}

// One block edited in place: the edited block is Changed; the rest stay
// Unchanged at the same relative position.
#[test]
fn edit_one_block_is_changed() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![new_block("a", 0), new_block("b2", 10), new_block("c", 20)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(
        classes(&diff),
        vec!["unchanged", "unchanged", "changed"]
    );
    assert_eq!(diff.changed, 1);
    assert_eq!(diff.unchanged, 2);
}

// Insert a block: the inserted block is Added; the blocks after it shift
// position and classify as Moved, carrying the refreshed positions.
#[test]
fn insert_block_adds_and_refreshes_positions() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![
        new_block("a", 0),
        new_block("x", 10),
        new_block("b", 24),
        new_block("c", 34),
    ];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(
        classes(&diff),
        vec!["unchanged", "moved", "moved", "added"]
    );
    assert_eq!(diff.added, 1);
    assert_eq!(diff.moved, 2);
    // The moved blocks carry the new positions for apply to write back.
    for change in &diff.changes {
        if let BlockChange::Moved { new, .. } = change {
            assert_eq!(new.char_start, if new.block_hash == "b" { 24 } else { 34 });
        }
    }
}

// Delete a block: the removed block classifies as Removed.
#[test]
fn delete_block_is_removed() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![new_block("a", 0), new_block("c", 10)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["unchanged", "moved", "removed"]);
    assert_eq!(diff.removed, 1);
    assert_eq!(diff.moved, 1);
}

// Move a block within the file: relocation by hash produces Moved for both the
// shifted survivors and the relocated block; nothing is enqueued later.
#[test]
fn move_block_within_file_is_moved() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![new_block("a", 0), new_block("c", 10), new_block("b", 20)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["unchanged", "moved", "moved"]);
    assert_eq!(diff.moved, 2);
    assert_eq!(diff.added, 0);
    assert_eq!(diff.changed, 0);
    assert_eq!(diff.removed, 0);
}

// Replace one block with two: the first new block pairs with the old one as
// Changed, the second is Added.
#[test]
fn replace_one_block_with_two() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![
        new_block("a", 0),
        new_block("x", 10),
        new_block("y", 18),
        new_block("c", 26),
    ];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(
        classes(&diff),
        vec!["unchanged", "moved", "changed", "added"]
    );
    assert_eq!(diff.changed, 1);
    assert_eq!(diff.added, 1);
}

// Duplicate identical hashes in one file stay matched to their own positions.
#[test]
fn duplicate_identical_blocks_stay_unchanged() {
    let stored = vec![stored(1, "a", 0), stored(2, "a", 10), stored(3, "b", 20)];
    let new = vec![new_block("a", 0), new_block("a", 10), new_block("b", 20)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(
        classes(&diff),
        vec!["unchanged", "unchanged", "unchanged"]
    );
    assert_eq!(diff.unchanged, 3);
}

// A duplicated block that also relocates: LCS picks one match; the other is
// recovered by hash pairing (Moved), never Added/Removed.
#[test]
fn duplicate_block_relocation_is_moved() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10)];
    let new = vec![new_block("b", 0), new_block("a", 10)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["moved", "moved"]);
    assert_eq!(diff.moved, 2);
    assert_eq!(diff.added, 0);
    assert_eq!(diff.removed, 0);
}

// Empty new list: everything is Removed.
#[test]
fn empty_new_list_is_all_removed() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10)];
    let new: Vec<NewBlock> = Vec::new();
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["removed", "removed"]);
    assert_eq!(diff.removed, 2);
}

// Empty stored list: everything is Added.
#[test]
fn empty_stored_list_is_all_added() {
    let stored: Vec<StoredBlock> = Vec::new();
    let new = vec![new_block("a", 0), new_block("b", 10)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["added", "added"]);
    assert_eq!(diff.added, 2);
}

// Full reorder: every block keeps its hash but shifts index, so all Moved.
#[test]
fn full_reorder_is_all_moved() {
    let stored = vec![stored(1, "a", 0), stored(2, "b", 10), stored(3, "c", 20)];
    let new = vec![new_block("c", 0), new_block("b", 10), new_block("a", 20)];
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(classes(&diff), vec!["moved", "moved", "moved"]);
    assert_eq!(diff.moved, 3);
}

// The set-membership fallback classifies by hash membership and index when the
// input is large. Build a synthetic large input without O(n^2) table blowup.
#[test]
fn set_fallback_classifies_large_lists() {
    let n = MAX_LCS_BLOCKS + 50;
    let stored: Vec<StoredBlock> = (0..n).map(|i| stored(i as i64, &format!("s{i}"), i * 10)).collect();
    let mut new: Vec<NewBlock> = (0..n).map(|i| new_block(&format!("s{i}"), i * 10)).collect();
    // Replace one block in place and append a fresh one. The set-membership
    // fallback has no Changed pairing, so the replaced block surfaces as
    // Removed (its old hash is gone) plus Added (the new content), and the
    // appended block is Added too; every other block keeps its index and stays
    // Unchanged.
    new[7] = new_block("edited", 7 * 10);
    new.push(new_block("fresh", n * 10));
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(diff.unchanged, n - 1);
    assert_eq!(diff.removed, 1);
    assert_eq!(diff.added, 2);
    assert_eq!(diff.changed, 0);
    assert_eq!(diff.moved, 0);
}

// At exactly MAX_LCS_BLOCKS the O(n*m) LCS path still runs: an edited block
// surfaces as Changed. The set fallback has no Changed pairing, so that single
// classification proves which algorithm ran at the boundary.
#[test]
fn lcs_path_runs_at_max_boundary() {
    let n = MAX_LCS_BLOCKS;
    let stored: Vec<StoredBlock> = (0..n).map(|i| stored(i as i64, &format!("s{i}"), i * 10)).collect();
    let mut new: Vec<NewBlock> = (0..n).map(|i| new_block(&format!("s{i}"), i * 10)).collect();
    new[7] = new_block("edited", 7 * 10);
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(diff.changed, 1);
    assert_eq!(diff.removed, 0);
    assert_eq!(diff.added, 0);
}

// One block past the boundary the cheap set-membership fallback runs instead:
// the edited block becomes Removed + Added, never Changed.
#[test]
fn set_fallback_runs_past_max_boundary() {
    let n = MAX_LCS_BLOCKS + 1;
    let stored: Vec<StoredBlock> = (0..n).map(|i| stored(i as i64, &format!("s{i}"), i * 10)).collect();
    let mut new: Vec<NewBlock> = (0..n).map(|i| new_block(&format!("s{i}"), i * 10)).collect();
    new[7] = new_block("edited", 7 * 10);
    let diff = diff_block_lists(&stored, &new);
    assert_eq!(diff.changed, 0);
    assert_eq!(diff.removed, 1);
    assert_eq!(diff.added, 1);
}