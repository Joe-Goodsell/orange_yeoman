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
    assert_eq!(classify_event(root, &event), Some(ClassifiedChange::Create(path)));
}

#[test]
fn remove_event_is_structure() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Remove(RemoveKind::File), &path);
    assert_eq!(classify_event(root, &event), Some(ClassifiedChange::Remove(path)));
}

// A single-path rename (macOS FSEvents shape) classifies as Structure with the
// carried path.
#[test]
fn single_path_rename_event_is_structure() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Name(RenameMode::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some(ClassifiedChange::Structure(path))
    );
}

// debouncer-full 0.7 emits RenameMode::Both with paths [from, to] when it
// matched the pair; the classification carries both ends.
#[test]
fn both_paths_rename_event_carries_from_to() {
    let root = Path::new("/repo");
    let from = root.join("old.md");
    let to = root.join("new.md");
    let event = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path(from.clone())
        .add_path(to.clone());
    assert_eq!(
        classify_event(root, &event),
        Some(ClassifiedChange::Rename { from, to })
    );
}

// The unpaired from side of a rename classifies as removal (old path gone).
#[test]
fn rename_from_event_is_remove() {
    let root = Path::new("/repo");
    let path = root.join("old.md");
    let event = event_at(EventKind::Modify(ModifyKind::Name(RenameMode::From)), &path);
    assert_eq!(classify_event(root, &event), Some(ClassifiedChange::Remove(path)));
}

// The unpaired to side of a rename classifies as creation (new path present).
#[test]
fn rename_to_event_is_create() {
    let root = Path::new("/repo");
    let path = root.join("new.md");
    let event = event_at(EventKind::Modify(ModifyKind::Name(RenameMode::To)), &path);
    assert_eq!(classify_event(root, &event), Some(ClassifiedChange::Create(path)));
}

// A rename that notify-debouncer-full could not interpret (RenameMode::Other)
// is skipped entirely.
#[test]
fn rename_other_event_is_none() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Name(RenameMode::Other)), &path);
    assert_eq!(classify_event(root, &event), None);
}

#[test]
fn markdown_data_modify_is_content() {
    let root = Path::new("/repo");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some(ClassifiedChange::Content(path))
    );
}

#[test]
fn uppercase_markdown_extension_is_content() {
    let root = Path::new("/repo");
    let path = root.join("NOTE.MD");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some(ClassifiedChange::Content(path))
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
    assert_eq!(classify_event(root, &event), Some(ClassifiedChange::Config));
}

#[test]
fn dotfile_named_root_still_reports_children() {
    let root = Path::new("/Users/joe/.notes");
    let path = root.join("note.md");
    let event = event_at(EventKind::Modify(ModifyKind::Data(DataChange::Any)), &path);
    assert_eq!(
        classify_event(root, &event),
        Some(ClassifiedChange::Content(path))
    );
}
