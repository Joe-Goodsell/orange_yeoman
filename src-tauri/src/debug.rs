// Debug event channel. Emits lightweight { category, message } events to the
// frontend on "debug://event" so the UI can show what the Rust core is doing.
// Callers MUST gate every emit on the merged config's debug flag; when debug
// is false, no debug IPC traffic exists at all.

use tauri::{AppHandle, Emitter};

#[derive(Clone, serde::Serialize)]
pub(crate) struct DebugEvent {
    pub category: String,
    pub message: String,
}

/// Emit a debug event to the frontend on the `debug://event` channel.
/// Callers MUST gate this on `app.state::<crate::config::ConfigState>().debug()`
/// being true, so that `debug: false` runs carry zero debug IPC traffic.
pub(crate) fn emit_debug_event(app: &AppHandle, category: &str, message: String) {
    let _ = app.emit(
        "debug://event",
        DebugEvent {
            category: category.into(),
            message,
        },
    );
}