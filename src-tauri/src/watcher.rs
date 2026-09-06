// File watcher built on notify + notify-debouncer-full. Events are classified
// by a pure function and emitted on the "watcher://change" channel with a
// { path, kind, hash } payload where kind is "structure" (create/remove/rename)
// or "content" (.md data modify). The optional hash is the stable content hash
// of the new on-disk bytes; it is present only on content events so the
// frontend can recognize its own writes. Structure arms also keep the concept
// store in sync: creates ingest the new file, removes drop stored blocks,
// matched renames relink blocks to the new path, and unmatched single-path
// renames fall back to drop + reprocess. Changes to the active root's
// .orange-yeoman.json reload the merged config and emit "config://changed".
// When the debug config flag is on, content changes also emit per-block
// add/remove/change events so the debug console can show exactly what the
// watcher saw on disk.

mod classify;
mod commands;
mod events;
mod snapshots;
mod sync;

pub(crate) use commands::{
    start_watcher, stop_watcher,
    __cmd__start_watcher, __cmd__stop_watcher,
    __tauri_command_name_start_watcher, __tauri_command_name_stop_watcher,
};
pub(crate) use commands::WatcherState;

// Test-only re-exports. The sibling submodules import their items directly;
// the root re-exports these solely so watcher/tests.rs can keep `use super::*;`.
#[cfg(test)]
pub(crate) use crate::config::CONFIG_FILE_NAME;
#[cfg(test)]
pub(crate) use classify::{classify_event, ClassifiedChange, content_hash};
#[cfg(test)]
pub(crate) use snapshots::{
    diff_snapshots, BlockSnapshot, DiffKind, excerpt_of, line_of, snapshots_of,
};

#[cfg(test)]
mod tests;