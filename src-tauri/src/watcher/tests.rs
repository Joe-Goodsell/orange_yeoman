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
