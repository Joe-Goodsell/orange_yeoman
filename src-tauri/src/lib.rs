// Orange Yeoman - Rust core. Filesystem access (list_dir, read_text_file), the
// dialog plugin (native folder picker), and the file watcher (notify +
// notify-debouncer-full) are wired here. The watcher emits Tauri events on the
// "watcher://change" channel with a { path, kind } payload where kind is
// "structure" (create/remove/rename) or "content" (.md data modify).

mod config;
mod filesystem;
mod llm;
mod watcher;

use tauri::Manager;

pub use filesystem::FileEntry;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(watcher::WatcherState::default())
        .manage(config::ConfigState::default())
        .manage(llm::LlmState::default())
        .setup(|app| {
            // Initialize the merged config with built-in defaults plus the
            // global config. No config file is created; missing files are valid.
            // Any error is stored in status and the app continues.
            config::reload_config_state(&app.state::<config::ConfigState>(), None);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config_status,
            config::load_project_config,
            filesystem::list_dir,
            filesystem::read_text_file,
            llm::complete_llm,
            watcher::start_watcher,
            watcher::stop_watcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
