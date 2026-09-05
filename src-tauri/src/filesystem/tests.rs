use super::*;

// write_text_file round-trips content to a temp file and returns the stable
// hash of the written bytes, matching what the watcher hashes when the write
// event arrives.
#[test]
fn write_text_file_writes_and_returns_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.md").to_string_lossy().to_string();
    let contents = "# Title\n\nBody text.\n";
    let hash = write_text_file(path.clone(), contents.to_string()).unwrap();
    assert_eq!(hash, crate::pipeline::stable_hash(contents));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);
}

// An unwritable path (missing parent directory) returns Err with the io
// message; nothing is written.
#[test]
fn write_text_file_unwritable_path_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join("no_such_dir")
        .join("note.md")
        .to_string_lossy()
        .to_string();
    assert!(write_text_file(path, "contents".to_string()).is_err());
}