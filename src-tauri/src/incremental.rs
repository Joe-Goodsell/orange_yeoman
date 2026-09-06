// Incremental, diff-driven block processing (research doc section 2.2).
// A file's blocks are diffed against the stored block list by block hash;
// only added and changed blocks are enqueued, moved blocks keep their stored
// rows (concepts stay attached), and removed blocks are deleted. Pure module:
// no I/O, no database. The apply step lives in store.rs.

use crate::pipeline::Block;
use std::collections::VecDeque;

/// Above this many blocks on either side, the O(n*m) LCS table is replaced by
/// a cheap set-membership diff. 5000*5000 fits the DP table in u16 storage
/// (~50 MB worst case); bigger files are matched by hash membership only.
pub(crate) const MAX_LCS_BLOCKS: usize = 5000;

/// One classification from the diff, carrying the fields the apply step needs.
/// `Unchanged` and `Moved` keep the stored row id plus the new positions:
/// offsets shift after edits above the block, so apply refreshes
/// char_start/char_end/heading_path for both even when the hash did not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockChange {
    Unchanged { stored: Block, new: Block },
    Added { new: Block },
    Changed { old: Block, new: Block },
    Moved { stored: Block, new: Block },
    Removed { stored: Block },
}

/// The outcome of diffing one file: the ordered change list plus per-class
/// counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FileDiff {
    pub(crate) changes: Vec<BlockChange>,
    pub(crate) unchanged: usize,
    pub(crate) added: usize,
    pub(crate) changed: usize,
    pub(crate) moved: usize,
    pub(crate) removed: usize,
}

impl FileDiff {
    fn record(&mut self, change: BlockChange) {
        match &change {
            BlockChange::Unchanged { .. } => self.unchanged += 1,
            BlockChange::Added { .. } => self.added += 1,
            BlockChange::Changed { .. } => self.changed += 1,
            BlockChange::Moved { .. } => self.moved += 1,
            BlockChange::Removed { .. } => self.removed += 1,
        }
        self.changes.push(change);
    }
}

/// Diff the stored block list of a file against the freshly parsed block list.
///
/// Algorithm:
/// 1. LCS on the hash sequences (DP, O(n*m)).
/// 2. Matched pairs: same relative index -> `Unchanged`; shifted index ->
///    `Moved`.
/// 3. Unmatched stored hashes that also appear in the unmatched new list (same
///    hash on both sides) pair up as `Moved` (block relocated; pairs matched
///    in order by first occurrence).
/// 4. Remaining unmatched: the stored run and the new run pair in order into
///    `Changed` (min of the two run lengths); leftovers become `Removed` /
///    `Added`.
///
/// Above `MAX_LCS_BLOCKS` on either side the fallback `diff_by_set` runs.
pub(crate) fn diff_block_lists(stored: &[Block], new: &[Block]) -> FileDiff {
    if stored.len() > MAX_LCS_BLOCKS || new.len() > MAX_LCS_BLOCKS {
        return diff_by_set(stored, new);
    }

    let n = stored.len();
    let m = new.len();

    // DP table, row-major. Lengths are at most min(n, m) <= MAX_LCS_BLOCKS,
    // which fits u16 (the 5000 threshold keeps the table under ~50 MB).
    let width = m + 1;
    let mut table = vec![0u16; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * width + j] = if stored[i].block_hash == new[j].block_hash {
                table[(i + 1) * width + (j + 1)] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + (j + 1)])
            };
        }
    }

    // Backtrack the LCS. Matched pairs keep their relative-position flag;
    // everything else is collected into the two global unmatched runs so the
    // relocation pairing in step 3 can see both sides of a move even when the
    // LCS split them across different gaps.
    let mut matched: Vec<(bool, Block, Block)> = Vec::new();
    let mut unmatched_stored: Vec<Block> = Vec::new();
    let mut unmatched_new: Vec<Block> = Vec::new();
    let mut i = 0usize;
    let mut j = 0usize;
    while i < n && j < m {
        if stored[i].block_hash == new[j].block_hash {
            matched.push((i == j, stored[i].clone(), new[j].clone()));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + (j + 1)] {
            unmatched_stored.push(stored[i].clone());
            i += 1;
        } else {
            unmatched_new.push(new[j].clone());
            j += 1;
        }
    }
    while i < n {
        unmatched_stored.push(stored[i].clone());
        i += 1;
    }
    while j < m {
        unmatched_new.push(new[j].clone());
        j += 1;
    }

    let mut diff = FileDiff::default();
    for (same_position, stored_block, new_block) in matched {
        if same_position {
            diff.record(BlockChange::Unchanged {
                stored: stored_block,
                new: new_block,
            });
        } else {
            diff.record(BlockChange::Moved {
                stored: stored_block,
                new: new_block,
            });
        }
    }

    // Step 3: pair unmatched stored hashes with the first remaining unmatched
    // new block of the same hash, in order.
    let mut remaining_new: VecDeque<Block> = unmatched_new.into_iter().collect();
    let mut moved_pairs: Vec<(Block, Block)> = Vec::new();
    let mut leftover_stored: Vec<Block> = Vec::new();
    for stored_block in unmatched_stored {
        let found = remaining_new
            .iter()
            .position(|n| n.block_hash == stored_block.block_hash);
        match found {
            Some(pos) => {
                let new_block = remaining_new.remove(pos).expect("position is in bounds");
                moved_pairs.push((stored_block, new_block));
            }
            None => leftover_stored.push(stored_block),
        }
    }
    let leftover_new: Vec<Block> = remaining_new.into_iter().collect();

    // Step 4: position-adjacent pairing into Changed; leftovers to Removed /
    // Added.
    let pair_count = leftover_stored.len().min(leftover_new.len());
    for (old, new_block) in leftover_stored
        .iter()
        .take(pair_count)
        .zip(leftover_new.iter().take(pair_count))
    {
        diff.record(BlockChange::Changed {
            old: old.clone(),
            new: new_block.clone(),
        });
    }
    for stored_block in &leftover_stored[pair_count..] {
        diff.record(BlockChange::Removed {
            stored: stored_block.clone(),
        });
    }
    for new_block in &leftover_new[pair_count..] {
        diff.record(BlockChange::Added { new: new_block.clone() });
    }
    for (stored_block, new_block) in moved_pairs {
        diff.record(BlockChange::Moved {
            stored: stored_block,
            new: new_block,
        });
    }
    diff
}

/// Set-membership fallback for very large files (no O(n*m) table, no Changed
/// pairing): in both + same index -> Unchanged; in both + different index ->
/// Moved; only stored -> Removed; only new -> Added.
fn diff_by_set(stored: &[Block], new: &[Block]) -> FileDiff {
    let mut diff = FileDiff::default();
    let mut new_remaining: VecDeque<(usize, &Block)> = new.iter().enumerate().collect();

    for (i, stored_block) in stored.iter().enumerate() {
        let found = new_remaining
            .iter()
            .position(|(_, n)| n.block_hash == stored_block.block_hash);
        match found {
            Some(pos) => {
                let (j, new_block) = new_remaining
                    .remove(pos)
                    .expect("position is in bounds");
                if i == j {
                    diff.record(BlockChange::Unchanged {
                        stored: stored_block.clone(),
                        new: new_block.clone(),
                    });
                } else {
                    diff.record(BlockChange::Moved {
                        stored: stored_block.clone(),
                        new: new_block.clone(),
                    });
                }
            }
            None => diff.record(BlockChange::Removed {
                stored: stored_block.clone(),
            }),
        }
    }
    for (_, new_block) in new_remaining {
        diff.record(BlockChange::Added {
            new: new_block.clone(),
        });
    }
    diff
}

#[cfg(test)]
mod tests;