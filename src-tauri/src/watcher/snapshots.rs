//! Block snapshots and longest-common-subsequence diffing for debug events.

// Debug-only block snapshot. The hash identifies a block across edits (see
// pipeline::stable_hash); the line range locates it in the file; the excerpt
// summarizes its content on one line for console messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BlockSnapshot {
    pub(crate) hash: String,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) excerpt: String,
}

// How one block changed between two snapshots of the same file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffKind {
    Added,
    Changed,
    Removed,
}

// One per-block change between two snapshots. For Added/Removed only the
// matching excerpt is set; for Changed both are set and the line range is the
// new block's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotDiff {
    pub(crate) kind: DiffKind,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) old_excerpt: Option<String>,
    pub(crate) new_excerpt: Option<String>,
}

// Collapse whitespace to single spaces, trim, and cap at `cap` chars. Keeps
// log messages single-line even when the source block is multi-line.
pub(crate) fn excerpt_of(text: &str, cap: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(cap)
        .collect()
}

// 1-based line number of a byte offset: count line breaks before it, plus one.
pub(crate) fn line_of(content: &str, offset: usize) -> usize {
    content[..offset.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

// Turn file content into block snapshots. Includes ALL blocks, excluded ones
// too: this is a debug view of disk state, not a fact-check filter.
pub(crate) fn snapshots_of(content: &str) -> Vec<BlockSnapshot> {
    crate::pipeline::parse_markdown_blocks(content)
        .into_iter()
        .map(|block| {
            // mdast quirk: the end offset of the LAST list item in a list
            // includes the trailing newline (paragraphs, headings, fences, and
            // quotes do not), so the byte before it is a newline and the naive
            // line count runs one line past the item. Back off one byte when
            // that happens; clamp so the range never collapses.
            let end_offset = if block.char_end > block.char_start
                && content.as_bytes().get(block.char_end - 1) == Some(&b'\n')
            {
                block.char_end - 1
            } else {
                block.char_end
            };
            let start_line = line_of(content, block.char_start);
            BlockSnapshot {
                hash: block.block_hash,
                start_line,
                end_line: line_of(content, end_offset).max(start_line),
                excerpt: excerpt_of(&block.text, 80),
            }
        })
        .collect()
}

// Diff two snapshot lists by hash using a longest-common-subsequence pass, so
// unchanged blocks stay anchored and only genuinely changed regions produce
// diffs. Block lists are small, so the O(n*m) DP is fine.
pub(crate) fn diff_snapshots(old: &[BlockSnapshot], new: &[BlockSnapshot]) -> Vec<SnapshotDiff> {
    let (m, n) = (old.len(), new.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if old[i].hash == new[j].hash {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }

    // Backtrack the DP table to collect matched (old, new) index pairs in
    // document order. These anchors delimit the changed regions below.
    let mut anchors = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < m && j < n {
        if old[i].hash == new[j].hash {
            anchors.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }

    let mut diffs = Vec::new();
    let (mut prev_old, mut prev_new) = (0, 0);
    for (oi, ni) in anchors {
        push_region_diffs(&mut diffs, &old[prev_old..oi], &new[prev_new..ni]);
        prev_old = oi + 1;
        prev_new = ni + 1;
    }
    push_region_diffs(&mut diffs, &old[prev_old..], &new[prev_new..]);
    diffs
}

// Emit diffs for a region between two matched anchors (or before the first or
// after the last). One old and one new block pair into a single Changed diff;
// anything else becomes one Removed per old block and one Added per new block,
// keeping the console output honest about every block.
fn push_region_diffs(
    diffs: &mut Vec<SnapshotDiff>,
    old_region: &[BlockSnapshot],
    new_region: &[BlockSnapshot],
) {
    if old_region.len() == 1 && new_region.len() == 1 {
        let (old_block, new_block) = (&old_region[0], &new_region[0]);
        diffs.push(SnapshotDiff {
            kind: DiffKind::Changed,
            start_line: new_block.start_line,
            end_line: new_block.end_line,
            old_excerpt: Some(old_block.excerpt.clone()),
            new_excerpt: Some(new_block.excerpt.clone()),
        });
        return;
    }
    for block in old_region {
        diffs.push(SnapshotDiff {
            kind: DiffKind::Removed,
            start_line: block.start_line,
            end_line: block.end_line,
            old_excerpt: Some(block.excerpt.clone()),
            new_excerpt: None,
        });
    }
    for block in new_region {
        diffs.push(SnapshotDiff {
            kind: DiffKind::Added,
            start_line: block.start_line,
            end_line: block.end_line,
            old_excerpt: None,
            new_excerpt: Some(block.excerpt.clone()),
        });
    }
}