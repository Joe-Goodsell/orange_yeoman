//! Event classification: turn a notify event into a typed change.

use crate::config::CONFIG_FILE_NAME;
use notify::event::{EventKind, ModifyKind, RenameMode};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// A classified outcome of a notify event relative to the watched root. Each
/// variant carries the path(s) the follow-up work needs. Rename carries both
/// ends when notify-debouncer-full matched the pair (paths[0] = from,
/// paths[1] = to); a rename whose sides it could not match arrives as a
/// single-path Structure.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClassifiedChange {
    Create(PathBuf),
    Remove(PathBuf),
    Structure(PathBuf),
    Content(PathBuf),
    Config,
    Rename { from: PathBuf, to: PathBuf },
}

// Classify a notify event against the watched root. Returns the classification
// for relevant events, or None for irrelevant ones.
// create/remove/rename: structure. .md data modify: content. the active root's
// own .orange-yeoman.json: config. Everything else (non-.md data modify,
// metadata, access, dotfile paths BELOW the root) is skipped. The watched
// root's own basename is exempt from the dotfile filter so dotfile-named roots
// like ~/.notes still work.
//
// Rename shapes (verified against notify-debouncer-full 0.7.0): it normalizes
// raw RenameMode::Any events into From/To/Both before the callback by an
// exists() check plus file-id/inode matching, so a both-sides rename on any
// platform reaches the Both branch with paths [from, to]. The single-path
// fallback below handles the sides it could not match.
pub(crate) fn classify_event(root: &Path, event: &notify::Event) -> Option<ClassifiedChange> {
    let path = event.paths.first()?;

    // Repository config file: a dedicated classification. This check runs
    // BEFORE the dotfile filter because the file is hidden by design. Only the
    // active watched root's own config file is considered.
    if path == &root.join(CONFIG_FILE_NAME) {
        return Some(ClassifiedChange::Config);
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
        EventKind::Create(_) => Some(ClassifiedChange::Create(path.clone())),
        EventKind::Remove(_) => Some(ClassifiedChange::Remove(path.clone())),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            let from = event.paths.first()?;
            let to = event.paths.get(1)?;
            Some(ClassifiedChange::Rename {
                from: from.clone(),
                to: to.clone(),
            })
        }
        // The from side of a rename with no known target: treat as removal.
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            Some(ClassifiedChange::Remove(path.clone()))
        }
        // The to side of a rename with no known source: treat as creation.
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            Some(ClassifiedChange::Create(path.clone()))
        }
        // Single-path rename with no matched side: direction unknown, so the
        // fallback (drop stale rows, re-process) handles it.
        EventKind::Modify(ModifyKind::Name(RenameMode::Any)) => {
            Some(ClassifiedChange::Structure(path.clone()))
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Other)) => None,
        EventKind::Modify(ModifyKind::Data(_)) => {
            if is_markdown(path) {
                Some(ClassifiedChange::Content(path.clone()))
            } else {
                None
            }
        }
        _ => None,
    };

    change
}

pub(crate) fn is_markdown(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("md"))
        .unwrap_or(false)
}

// Stable content hash of a file for the watcher event payload, or None when
// the file cannot be read (e.g. transiently mid-save). The frontend compares
// this against the hash of its own last save to recognize self-writes.
pub(crate) fn content_hash(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|content| crate::pipeline::stable_hash(&content))
}