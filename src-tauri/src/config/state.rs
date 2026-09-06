//! The shared Tauri-managed config state.

use std::sync::Mutex;

use super::model::MergedConfig;

pub(crate) struct ConfigState {
    pub(crate) inner: Mutex<MergedConfig>,
}

impl ConfigState {
    /// Current small model id from the merged config. The resolved id is
    /// returned for both the structured and the legacy form.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn small_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| {
                guard
                    .small
                    .as_ref()
                    .map(|spec| spec.id().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| String::new())
    }

    /// Current large model id from the merged config. The resolved id is
    /// returned for both the structured and the legacy form.
    /// Falls back to an empty string when the lock is poisoned.
    pub(crate) fn large_model(&self) -> String {
        self.inner
            .lock()
            .map(|guard| {
                guard
                    .large
                    .as_ref()
                    .map(|spec| spec.id().to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|_| String::new())
    }

    /// Provider named by the current small model, when the structured form is
    /// used. None for the legacy plain-string form and for an unset slot.
    /// Falls back to None when the lock is poisoned.
    pub(crate) fn small_provider(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.small.as_ref().and_then(|s| s.provider().map(str::to_string)))
    }

    /// Provider named by the current large model, when the structured form is
    /// used. None for the legacy plain-string form and for an unset slot.
    /// Falls back to None when the lock is poisoned.
    pub(crate) fn large_provider(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.large.as_ref().and_then(|s| s.provider().map(str::to_string)))
    }

    /// Current debug flag from the merged config. Debug events are emitted
    /// only when this is true. Recovers the stored value when the lock is
    /// poisoned instead of failing open.
    pub(crate) fn debug(&self) -> bool {
        self.inner
            .lock()
            .map(|m| m.debug)
            .unwrap_or_else(|e| e.into_inner().debug)
    }

    /// Current mock_llm flag from the merged config. Recovers the stored
    /// value when the lock is poisoned instead of failing open.
    pub(crate) fn mock_llm(&self) -> bool {
        self.inner
            .lock()
            .map(|m| m.mock_llm)
            .unwrap_or_else(|e| e.into_inner().mock_llm)
    }
}

impl Default for ConfigState {
    fn default() -> Self {
        ConfigState {
            inner: Mutex::new(MergedConfig::default()),
        }
    }
}
