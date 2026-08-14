// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
// Orange Yeoman — Rust core. File watcher, folder-tree IPC, and slash-command
// surfaces are follow-up features; this skeleton wires the app shell only.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}