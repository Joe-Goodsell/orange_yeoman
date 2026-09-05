// Orange Yeoman - Rust core. Filesystem access (list_dir, read_text_file), the
// dialog plugin (native folder picker), and the file watcher (notify +
// notify-debouncer-full) are wired here. The watcher emits Tauri events on the
// "watcher://change" channel with a { path, kind } payload where kind is
// "structure" (create/remove/rename) or "content" (.md data modify).

mod config;
mod filesystem;
mod incremental;
mod llm;
mod pipeline;
mod store;
mod tasks;
mod telemetry;
mod watcher;

use std::sync::Arc;
use tauri::Manager;

pub use filesystem::FileEntry;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize the tracing subscriber registry before any instrumentation
    // runs. The debug-event layer no-ops for IPC until set_app_handle fills
    // the handle in setup; stderr logging works immediately.
    telemetry::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(watcher::WatcherState::default())
        .manage(config::ConfigState::default())
        .manage(llm::LlmState::default())
        .manage(Arc::new(tasks::TaskStore::default()))
        .manage(store::StoreState::new())
        .setup(|app| {
            // Initialize the merged config with empty defaults plus the
            // global config. No config file is created; missing files are valid.
            // Any error is stored in status and the app continues.
            let config_state = app.state::<config::ConfigState>();
            let llm_state = app.state::<llm::LlmState>();
            config::reload_config_state(&config_state, None);
            llm_state.set_provider(llm::select_provider(config_state.mock_llm()));
            // Open the concept store DB next to the global config. On failure
            // the connection stays None inside StoreState and processing
            // no-ops; the app keeps running.
            let store_state = app.state::<store::StoreState>();
            store_state.open();
            // Populate the telemetry layer's app handle so debug://event IPC
            // emission works from here on.
            telemetry::set_app_handle(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            config::get_config_status,
            config::load_project_config,
            config::validate_models,
            filesystem::list_dir,
            filesystem::read_text_file,
            llm::complete_llm,
            tasks::submit_block,
            tasks::submit_fact_check,
            tasks::submit_research,
            tasks::get_task_status,
            watcher::start_watcher,
            watcher::stop_watcher
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
