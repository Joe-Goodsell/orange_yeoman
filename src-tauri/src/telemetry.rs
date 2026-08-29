// Telemetry: the `tracing` subscriber registry and the IPC debug bridge.
// Two layers share the registry: a fmt layer to stderr (filtered by the
// RUST_LOG env filter, defaulting to info) and a DebugEventLayer that forwards
// events to the frontend on the "debug://event" channel.
//
// Gating policy (the debug: false contract):
// - ERROR and WARN events always reach the IPC channel, so failures stay
//   visible even when the debug config flag is false.
// - INFO, DEBUG, and TRACE events reach the IPC channel only when the merged
//   config's debug flag is true.
// The stderr layer is never gated; it follows the EnvFilter only.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{Event, Level};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// The app handle for IPC emission. Empty until the Tauri setup callback runs,
/// so events logged before setup no-op for IPC while still reaching stderr.
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// Payload emitted on the "debug://event" channel. `ts` is stamped in Rust at
/// emit time (milliseconds since UNIX epoch) so the frontend never has to guess
/// the timestamp. Never contains API key material.
#[derive(Clone, serde::Serialize)]
pub(crate) struct DebugEvent {
    pub category: String,
    pub message: String,
    pub level: String,
    pub ts: u64,
}

/// Initialize the global subscriber registry. Call once at startup, before the
/// Tauri builder runs, so every subsequent tracing call is captured. Uses
/// try_init so a duplicate init (e.g. in tests) does not panic.
pub(crate) fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_filter(filter),
        )
        .with(DebugEventLayer)
        .try_init();
}

/// Populate the app handle so the DebugEventLayer can emit IPC. Call from the
/// Tauri setup callback. Idempotent: the OnceLock keeps the first handle.
pub(crate) fn set_app_handle(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
}

/// Layer that forwards tracing events to the "debug://event" IPC channel under
/// the gating policy documented at the top of this module.
#[derive(Clone, Copy, Default)]
pub(crate) struct DebugEventLayer;

impl<S> Layer<S> for DebugEventLayer
where
    S: tracing::Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        // ERROR and WARN always reach IPC. INFO and below reach IPC only when
        // the merged config's debug flag is true; without a handle yet there is
        // no config to read, so they stay silent on IPC.
        if level >= Level::INFO {
            let debug_on = APP_HANDLE
                .get()
                .map(|app| app.state::<crate::config::ConfigState>().debug())
                .unwrap_or(false);
            if !debug_on {
                return;
            }
        }
        let Some(app) = APP_HANDLE.get() else {
            return;
        };

        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);
        let category = visitor
            .category
            .unwrap_or_else(|| event.metadata().target().to_string());
        let level_str = match level {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            Level::DEBUG => "debug",
            Level::TRACE => "trace",
        };
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let _ = app.emit(
            "debug://event",
            DebugEvent {
                category,
                message: visitor.message,
                level: level_str.to_string(),
                ts,
            },
        );
    }
}

/// Field visitor that extracts the structured `category` field (when present)
/// and the formatted `message` field from an event.
#[derive(Default)]
struct EventFieldVisitor {
    category: Option<String>,
    message: String,
}

impl tracing::field::Visit for EventFieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "category" => self.category = Some(value.to_string()),
            "message" => self.message.push_str(value),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            use std::fmt::Write;
            // The message field carries a Formatted value whose Debug impl
            // forwards to Display, so this prints the message without quotes.
            let _ = write!(self.message, "{value:?}");
        }
    }
}