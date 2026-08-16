// Filesystem commands: directory listing and plain text file reads. The folder
// tree mirrors on-disk structure exactly; there is no app-owned note database.

use serde::Serialize;
use std::fs;

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[tauri::command]
pub(crate) fn list_dir(dir: String) -> Result<Vec<FileEntry>, String> {
    let mut entries: Vec<FileEntry> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path().to_string_lossy().to_string();
            // file_type does not follow symlinks, so symlinked directories are
            // not reported as directories.
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some(FileEntry { name, path, is_dir })
        })
        .collect();

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

#[tauri::command]
pub(crate) fn read_text_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| e.to_string())
}
