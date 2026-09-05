// File watcher built on notify + notify-debouncer-full. Events are classified
// by a pure function and emitted on the "watcher://change" channel with a
// { path, kind } payload where kind is "structure" (create/remove/rename) or
// "content" (.md data modify). Changes to the active root's .orange-yeoman.json
// reload the merged config and emit "config://changed". When the debug config
// flag is on, content changes also emit per-block add/remove/change events so
// the debug console can show exactly what the watcher saw on disk.

use crate::config::{reload_config_state, ConfigState, CONFIG_FILE_NAME};
use notify::event::{EventKind, ModifyKind};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager};

#[derive(Serialize, Clone)]
struct WatcherEvent {
    path: String,
    kind: String,
}

// The debounced watcher type. RecommendedCache is the platform default file ID
// cache (FileIdMap on macOS). The debouncer is Send + Sync, so it can live in
// Tauri state.
type Deb = notify_debouncer_full::Debouncer<
    notify::RecommendedWatcher,
    notify_debouncer_full::RecommendedCache,
>;

pub(crate) struct WatcherState {
    debouncer: Mutex<Option<Deb>>,
    // Block snapshots per watched file, used to emit block-level diff events
    // for content changes when the debug config flag is on.
    block_cache: Mutex<HashMap<PathBuf, Vec<BlockSnapshot>>>,
}

impl Default for WatcherState {
    fn default() -> Self {
        WatcherState {
            debouncer: Mutex::new(None),
            block_cache: Mutex::new(HashMap::new()),
        }
    }
}

// Classified outcome of a notify event relative to the watched root.
#[derive(Debug, PartialEq, Eq)]
enum ClassifiedChange {
    Structure,
    Content,
    Config,
}

// Classify a notify event against the watched root. Returns the first event
// path together with its classification, or None for irrelevant events.
// structure: create/remove/rename. content: .md data modify. config: the active
// root's own .orange-yeoman.json. Everything else (non-.md data modify,
// metadata, access, dotfile paths BELOW the root) is skipped. The watched
// root's own basename is exempt from the dotfile filter so dotfile-named roots
// like ~/.notes still work.
fn classify_event(root: &Path, event: &notify::Event) -> Option<(PathBuf, ClassifiedChange)> {
    let path = event.paths.first()?;

    // Repository config file: a dedicated classification. This check runs
    // BEFORE the dotfile filter because the file is hidden by design. Only the
    // active watched root's own config file is considered.
    if path == &root.join(CONFIG_FILE_NAME) {
        return Some((path.clone(), ClassifiedChange::Config));
    }

    // Dotfile filter: check only the path components BELOW the watched root.
    // This blocks .git/.obsidian noise while a root such as /Users/joe/.notes
    // (whose own basename starts with ".") still gets its events through.
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative
        .components()
        .any(|c| matches!(c, Component::Normal(seg) if seg.to_string_lossy().starts_with('.')))
    {
        return None;
    }

    let change = match &event.kind {
        EventKind::Create(_) | EventKind::Remove(_) => Some(ClassifiedChange::Structure),
        EventKind::Modify(ModifyKind::Name(_)) => Some(ClassifiedChange::Structure),
        EventKind::Modify(ModifyKind::Data(_)) => {
            if path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                Some(ClassifiedChange::Content)
            } else {
                None
            }
        }
        _ => None,
    };

    change.map(|change| (path.clone(), change))
}

// Debug-only block snapshot. The hash identifies a block across edits (see
// pipeline::stable_hash); the line range locates it in the file; the excerpt
// summarizes its content on one line for console messages.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BlockSnapshot {
    hash: String,
    start_line: usize,
    end_line: usize,
    excerpt: String,
}

// How one block changed between two snapshots of the same file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffKind {
    Added,
    Changed,
    Removed,
}

// One per-block change between two snapshots. For Added/Removed only the
// matching excerpt is set; for Changed both are set and the line range is the
// new block's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotDiff {
    kind: DiffKind,
    start_line: usize,
    end_line: usize,
    old_excerpt: Option<String>,
    new_excerpt: Option<String>,
}

// Collapse whitespace to single spaces, trim, and cap at `cap` chars. Keeps
// log messages single-line even when the source block is multi-line.
fn excerpt_of(text: &str, cap: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(cap)
        .collect()
}

// 1-based line number of a byte offset: count line breaks before it, plus one.
fn line_of(content: &str, offset: usize) -> usize {
    content[..offset.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

// Turn file content into block snapshots. Includes ALL blocks, excluded ones
// too: this is a debug view of disk state, not a fact-check filter.
fn snapshots_of(content: &str) -> Vec<BlockSnapshot> {
    crate::pipeline::parse_markdown_blocks(content)
        .into_iter()
        .map(|block| {
            // mdast quirk: the end offset of the LAST list item in a list
            // includes the trailing newline (paragraphs, headings, fences, and
            // quotes do not), so the byte before it is a newline and the naive
            // line count runs one line past the item. Back off one byte when
            // that happens; clamp so the range never collapses.
            let end_offset = if block.end > block.start
                && content.as_bytes().get(block.end - 1) == Some(&b'\n')
            {
                block.end - 1
            } else {
                block.end
            };
            let start_line = line_of(content, block.start);
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
fn diff_snapshots(old: &[BlockSnapshot], new: &[BlockSnapshot]) -> Vec<SnapshotDiff> {
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

// Classify a notify event and emit it if it is relevant. The config payload is
// ConfigStatus, which never contains API key values.
fn classify_and_emit(app: &tauri::AppHandle, root: &Path, event: &notify::Event) {
    let Some((path, change)) = classify_event(root, event) else {
        return;
    };
    match &change {
        ClassifiedChange::Config => {
            let config_state = app.state::<ConfigState>();
            let status = reload_config_state(&config_state, Some(root));
            let llm_state = app.state::<crate::llm::LlmState>();
            llm_state.set_provider(crate::llm::select_provider(config_state.mock_llm()));
            let _ = app.emit("config://changed", status);
            // Debug instrumentation: report the reload and the merged debug
            // flag, read AFTER the reload so the event reflects new state.
            tracing::info!(
                category = "config",
                "config reloaded: {} (debug = {})",
                root.display(),
                config_state.debug()
            );
        }
        ClassifiedChange::Structure => {
            emit_watcher_change(app, &path, "structure");
            // Any structure event (create/remove/rename) invalidates the block
            // cache entry: the cached snapshot no longer matches the file.
            app.state::<WatcherState>()
                .block_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&path);
            tracing::info!(category = "watcher", "structure change: {}", path.display());
        }
        ClassifiedChange::Content => {
            emit_watcher_change(app, &path, "content");
            tracing::info!(category = "watcher", "content change: {}", path.display());
            // Block-level diffs follow the file-level event so the console
            // shows the summary line before the per-block detail.
            emit_block_diffs(app, &path);
        }
    }
}

// Emit block-level diff events for a changed file. This is a debug-only path:
// with the debug flag off the cache entry is dropped (so a later re-enable
// starts from a fresh baseline) and nothing else happens.
fn emit_block_diffs(app: &tauri::AppHandle, path: &Path) {
    let config_state = app.state::<crate::config::ConfigState>();
    if !config_state.debug() {
        app.state::<WatcherState>()
            .block_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(path);
        return;
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            // Keep the old cache: the file may be transiently unreadable
            // (e.g. mid-save), and the next change event will retry.
            tracing::debug!(
                category = "watcher",
                "block diff unavailable: could not read {}: {}",
                path.display(),
                err
            );
            return;
        }
    };

    let new_snapshots = snapshots_of(&content);
    // Lock only to swap the cached snapshot. A poisoned mutex is skipped
    // silently: this is a debug-only path, not worth surfacing to the user.
    let old = match app.state::<WatcherState>().block_cache.lock() {
        Ok(mut guard) => guard.insert(path.to_path_buf(), new_snapshots.clone()),
        Err(poisoned) => poisoned
            .into_inner()
            .insert(path.to_path_buf(), new_snapshots.clone()),
    };

    let Some(old_snapshots) = old else {
        // First sight of this file: store the baseline and report it. The
        // snapshots are already cached above, so later edits diff against it.
        tracing::debug!(
            category = "watcher",
            "block baseline: {} blocks in {}",
            new_snapshots.len(),
            path.display()
        );
        return;
    };

    for diff in diff_snapshots(&old_snapshots, &new_snapshots) {
        let range = format!("{}:{}-{}", path.display(), diff.start_line, diff.end_line);
        match diff.kind {
            DiffKind::Added => {
                let excerpt = diff.new_excerpt.unwrap_or_default();
                tracing::debug!(category = "watcher", "block added in {range}: \"{excerpt}\"");
            }
            DiffKind::Removed => {
                let excerpt = diff.old_excerpt.unwrap_or_default();
                tracing::debug!(category = "watcher", "block removed from {range}: \"{excerpt}\"");
            }
            DiffKind::Changed => {
                let old_excerpt = diff.old_excerpt.unwrap_or_default();
                let new_excerpt = diff.new_excerpt.unwrap_or_default();
                tracing::debug!(
                    category = "watcher",
                    "block changed in {range}: \"{old_excerpt}\" -> \"{new_excerpt}\""
                );
            }
        }
    }
}

fn emit_watcher_change(app: &tauri::AppHandle, path: &Path, kind: &str) {
    let _ = app.emit(
        "watcher://change",
        WatcherEvent {
            path: path.to_string_lossy().to_string(),
            kind: kind.to_string(),
        },
    );
}

#[tauri::command]
pub(crate) fn start_watcher(
    root: String,
    app: tauri::AppHandle,
    state: tauri::State<WatcherState>,
) -> Result<(), String> {
    // Stop any existing watcher first. Dropping the old debouncer stops its fs watch.
    {
        *state.debouncer.lock().map_err(|e| e.to_string())? = None;
    }

    // A new watch session starts from a fresh baseline: drop block snapshots
    // cached by the previous session so the first content change does not diff
    // against a pre-stop snapshot.
    state
        .block_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();

    let root_path = PathBuf::from(&root);
    let meta = fs::metadata(&root_path).map_err(|e| format!("invalid root {root}: {e}"))?;
    if !meta.is_dir() {
        return Err(format!("{root} is not a directory"));
    }

    let app_for_callback = app.clone();
    let watch_root = root_path.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                for debounced in events {
                    classify_and_emit(&app_for_callback, &watch_root, &debounced.event);
                }
            }
            Err(errors) => {
                tracing::error!(category = "watcher", "debounce errors: {:?}", errors);
            }
        },
    )
    .map_err(|e| format!("failed to create debouncer: {e}"))?;

    debouncer
        .watch(&root_path, RecursiveMode::Recursive)
        .map_err(|e| format!("failed to watch {root}: {e}"))?;

    *state.debouncer.lock().map_err(|e| e.to_string())? = Some(debouncer);

    Ok(())
}

#[tauri::command]
pub(crate) fn stop_watcher(state: tauri::State<WatcherState>) -> Result<(), String> {
    *state.debouncer.lock().map_err(|e| e.to_string())? = None;
    // A new watch session starts from a fresh baseline: drop block snapshots
    // cached by the stopped session.
    state
        .block_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    Ok(())
}

#[cfg(test)]
mod tests;
