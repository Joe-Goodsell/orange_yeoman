use super::*;
use notify::event::{CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode};
use notify::EventKind;
use std::path::Path;

fn event_at(kind: EventKind, path: &Path) -> notify::Event {
    notify::Event::new(kind).add_path(path.to_path_buf())
}

#[test]
fn create_event_is_structure() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Create(CreateKind::File), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Structure))
    );
}

#[test]
fn remove_event_is_structure() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Remove(RemoveKind::File), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Structure))
    );
}

#[test]
fn rename_event_is_structure() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Structure))
    );
}

#[test]
fn markdown_data_modify_is_content() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Content))
    );
}

#[test]
fn uppercase_markdown_extension_is_content() {
    let root = Path::new("/repo");
    let path = root.join("NOTE.MD");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Content))
    );
}

#[test]
fn non_markdown_data_modify_is_none() {
    let root = Path::new("/repo");
    let path = root.join("notes.txt");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(classify_event(root, &event), None);
}

#[test]
fn nested_dotfile_path_is_none() {
    let root = Path::new("/repo");
    let path = root.join(".obsidian").join("workspace.json");
    let event = event_at(EventKind::Create(CreateKind::File), &path);
    assert_eq!(classify_event(root, &event), None);
}

#[test]
fn repository_config_path_is_config() {
    let root = Path::new("/repo");
    let path = root.join(CONFIG_FILE_NAME);
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Config))
    );
}

#[test]
fn dotfile_named_root_still_reports_children() {
    let root = Path::new("/Users/joe/.notes");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some((path, ClassifiedChange::Content))
    );
}

// --- Block snapshot diffing ---

fn snap(hash: &str, start_line: usize, end_line: usize, excerpt: &str) -> BlockSnapshot {
    BlockSnapshot {
        hash: hash.to_string(),
        start_line,
        end_line,
        excerpt: excerpt.to_string(),
    }
}

#[test]
fn identical_snapshots_diff_to_empty() {
    let blocks = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    assert_eq!(diff_snapshots(&blocks, &blocks), Vec::new());
}

#[test]
fn appended_block_is_one_added_diff() {
    let old = vec![snap("a", 1, 1, "alpha")];
    let new = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    let diffs = diff_snapshots(&old, &new);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, DiffKind::Added);
    assert_eq!(diffs[0].start_line, 3);
    assert_eq!(diffs[0].end_line, 3);
    assert_eq!(diffs[0].old_excerpt, None);
    assert_eq!(diffs[0].new_excerpt.as_deref(), Some("beta"));
}

#[test]
fn removed_block_is_one_removed_diff() {
    let old = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    let new = vec![snap("a", 1, 1, "alpha")];
    let diffs = diff_snapshots(&old, &new);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, DiffKind::Removed);
    assert_eq!(diffs[0].start_line, 3);
    assert_eq!(diffs[0].end_line, 3);
    assert_eq!(diffs[0].old_excerpt.as_deref(), Some("beta"));
    assert_eq!(diffs[0].new_excerpt, None);
}

#[test]
fn replaced_block_pairs_into_one_changed_diff() {
    let old = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    let new = vec![snap("a", 1, 1, "alpha"), snap("b2", 3, 3, "BETA!")];
    let diffs = diff_snapshots(&old, &new);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, DiffKind::Changed);
    // The Changed diff carries the NEW block's line range.
    assert_eq!(diffs[0].start_line, 3);
    assert_eq!(diffs[0].end_line, 3);
    assert_eq!(diffs[0].old_excerpt.as_deref(), Some("beta"));
    assert_eq!(diffs[0].new_excerpt.as_deref(), Some("BETA!"));
}

#[test]
fn insert_at_top_keeps_later_blocks_matched() {
    let old = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    let new = vec![
        snap("x", 1, 1, "xray"),
        snap("a", 3, 3, "alpha"),
        snap("b", 5, 5, "beta"),
    ];
    let diffs = diff_snapshots(&old, &new);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, DiffKind::Added);
    assert_eq!(diffs[0].new_excerpt.as_deref(), Some("xray"));
}

#[test]
fn multiple_divergent_blocks_emit_separate_diffs() {
    let old = vec![
        snap("a", 1, 1, "alpha"),
        snap("b", 3, 3, "beta"),
        snap("c", 5, 5, "gamma"),
        snap("d", 7, 7, "delta"),
    ];
    let new = vec![
        snap("a", 1, 1, "alpha"),
        snap("x", 3, 3, "xray"),
        snap("y", 5, 5, "yankee"),
        snap("d", 7, 7, "delta"),
    ];
    let diffs = diff_snapshots(&old, &new);
    assert_eq!(diffs.len(), 4);
    assert_eq!(diffs[0].kind, DiffKind::Removed);
    assert_eq!(diffs[0].old_excerpt.as_deref(), Some("beta"));
    assert_eq!(diffs[1].kind, DiffKind::Removed);
    assert_eq!(diffs[1].old_excerpt.as_deref(), Some("gamma"));
    assert_eq!(diffs[2].kind, DiffKind::Added);
    assert_eq!(diffs[2].new_excerpt.as_deref(), Some("xray"));
    assert_eq!(diffs[3].kind, DiffKind::Added);
    assert_eq!(diffs[3].new_excerpt.as_deref(), Some("yankee"));
}

#[test]
fn excerpt_is_collapsed_trimmed_and_capped() {
    assert_eq!(excerpt_of("  alpha\n  beta\t  gamma  ", 80), "alpha beta gamma");
    let long = "word ".repeat(40); // 200 chars, far over the 80-char cap
    assert_eq!(excerpt_of(&long, 80).chars().count(), 80);
}

#[test]
fn line_numbers_are_one_based_from_byte_offsets() {
    let content = "alpha\nbeta\ngamma";
    assert_eq!(line_of(content, 0), 1);
    assert_eq!(line_of(content, 6), 2);
    assert_eq!(line_of(content, 11), 3);
    assert_eq!(line_of(content, 15), 3);
    assert_eq!(line_of(content, 99), 3); // clamped beyond end of file
}

#[test]
fn snapshots_from_content_include_all_blocks() {
    let content = "# Title\n\nFirst paragraph.\n\nSecond paragraph.\n";
    let snapshots = snapshots_of(content);
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].start_line, 1);
    assert_eq!(snapshots[0].excerpt, "# Title");
    assert_eq!(snapshots[1].start_line, 3);
    assert_eq!(snapshots[1].excerpt, "First paragraph.");
    assert_eq!(snapshots[2].start_line, 5);
    assert_eq!(snapshots[2].excerpt, "Second paragraph.");
}

#[test]
fn empty_old_yields_all_added() {
    let new = vec![snap("a", 1, 1, "alpha"), snap("b", 3, 3, "beta")];
    let diffs = diff_snapshots(&[], &new);
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0].kind, DiffKind::Added);
    assert_eq!(diffs[0].start_line, 1);
    assert_eq!(diffs[0].end_line, 1);
    assert_eq!(diffs[0].old_excerpt, None);
    assert_eq!(diffs[0].new_excerpt.as_deref(), Some("alpha"));
    assert_eq!(diffs[1].kind, DiffKind::Added);
    assert_eq!(diffs[1].start_line, 3);
    assert_eq!(diffs[1].end_line, 3);
    assert_eq!(diffs[1].old_excerpt, None);
    assert_eq!(diffs[1].new_excerpt.as_deref(), Some("beta"));
}

#[test]
fn empty_new_yields_all_removed() {
    let old = vec![snap("a", 1, 1, "alpha")];
    let diffs = diff_snapshots(&old, &[]);
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].kind, DiffKind::Removed);
    assert_eq!(diffs[0].start_line, 1);
    assert_eq!(diffs[0].end_line, 1);
    assert_eq!(diffs[0].old_excerpt.as_deref(), Some("alpha"));
    assert_eq!(diffs[0].new_excerpt, None);
}

#[test]
fn list_item_end_line_excludes_trailing_newline() {
    // The list is the last block and a blank line follows it, so the mdast
    // end offset of its last item includes the trailing newline; the item's
    // end_line must not run past its own line.
    let content = "- alpha\n- beta\n\n";
    let snapshots = snapshots_of(content);
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[1].start_line, 2);
    assert_eq!(snapshots[1].end_line, snapshots[1].start_line);
    assert_eq!(snapshots[1].excerpt, "- beta");
}
