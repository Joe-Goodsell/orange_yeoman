// Provider-neutral LLM boundary. This phase defines the frontend-facing
// request/response domain types, an async provider trait, and a deterministic
// MockProvider that performs no network I/O and makes no real LLM calls. Real
// providers (e.g. Anthropic Message Batches) implement the same trait in a
// later phase.
//
// There is no streaming or message-list boundary yet: the boundary is a single
// request in, one structured response out. API key values never appear in this
// module or in any payload it produces.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// Kind of LLM work requested by the frontend. Serialized as snake_case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LlmRequestKind {
    Extraction,
    FactCheck,
    Research,
}

// Frontend-facing request. Field names serialize as camelCase. The user prompt
// is the only required field; model, system prompt, and max tokens are
// provider hints the mock may ignore.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LlmRequest {
    pub kind: LlmRequestKind,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

// Token usage reported for a completed request. Values come from the provider;
// the mock supplies fixed estimates.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

// Provider response. `result` is a structured JSON value whose shape depends
// on the request kind; every result carries a schema_version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LlmResponse {
    pub response_id: String,
    pub kind: LlmRequestKind,
    pub model: String,
    pub result: serde_json::Value,
    pub usage: LlmUsage,
}

// Internal LLM boundary error. Display output is sanitized: it never embeds
// user prompt content, raw provider payloads, or API key material. Providers
// must pass already-safe detail text to the Provider variant; the Display impl
// additionally scrubs API-key-like tokens and caps the length as a backstop.
#[derive(Debug)]
pub(crate) enum LlmError {
    EmptyUserPrompt,
    // Not yet constructed by any real provider in this phase; real providers
    // build it in a later phase.
    #[allow(dead_code)]
    Provider(String),
}

// Remove API-key-like material (whitespace-delimited tokens starting with
// "sk-", e.g. OpenAI/Anthropic-style keys) from provider detail text. Also
// normalizes whitespace so multi-line payloads collapse into one line.
fn scrub_secrets(text: &str) -> String {
    text.split_whitespace()
        .map(|token| {
            if token.starts_with("sk-") {
                "sk-[REDACTED]".to_string()
            } else {
                token.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::EmptyUserPrompt => write!(f, "user prompt must not be empty"),
            LlmError::Provider(detail) => {
                // Scrub key-like tokens, strip control characters, and cap the
                // length so no raw payload or embedded secret material reaches
                // the frontend intact.
                let safe: String = scrub_secrets(detail)
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(200)
                    .collect();
                write!(f, "LLM provider error: {safe}")
            }
        }
    }
}

impl std::error::Error for LlmError {}

// Async provider boundary. Implementations must be Send + Sync so an
// Arc<dyn LlmProvider> can live in Tauri state and be awaited from Tauri
// commands. A boxed future keeps the trait object-safe without needing
// async-trait.
pub(crate) trait LlmProvider: Send + Sync {
    fn complete(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + Send + '_>>;
}

// Deterministic mock provider. Returns hardcoded structured JSON for every
// request kind, never touches the network, and never copies the user prompt
// into its output. Results are marked mock: true so callers do not mistake
// them for authoritative or sourced research.
pub(crate) struct MockProvider;

impl MockProvider {
    fn new() -> Self {
        MockProvider
    }
}

const MOCK_MODEL: &str = "mock-provider-v1";
const MOCK_SCHEMA_VERSION: u64 = 1;
const MOCK_INPUT_TOKENS: u64 = 10;
const MOCK_OUTPUT_TOKENS: u64 = 20;

// Stable response ID for a request kind. Equivalent request kinds map to the
// same ID; different kinds map to different IDs.
fn mock_response_id(kind: LlmRequestKind) -> String {
    match kind {
        LlmRequestKind::Extraction => "mock-extraction",
        LlmRequestKind::FactCheck => "mock-factcheck",
        LlmRequestKind::Research => "mock-research",
    }
    .to_string()
}

// Hardcoded structured result per request kind. No prompt content, API key
// material, or model output ever appears here.
fn mock_result(kind: LlmRequestKind) -> serde_json::Value {
    match kind {
        LlmRequestKind::Extraction => serde_json::json!({
            "schema_version": MOCK_SCHEMA_VERSION,
            "mock": true,
            "claims": [
                {
                    "id": "mock-claim-1",
                    "text": "Placeholder claim text. Mock provider output.",
                    "confidence": "low",
                    "sources": []
                }
            ]
        }),
        LlmRequestKind::FactCheck => serde_json::json!({
            "schema_version": MOCK_SCHEMA_VERSION,
            "mock": true,
            "checks": [
                {
                    "id": "mock-check-1",
                    "claim": "Placeholder claim text. Mock provider output.",
                    "verdict": "unverified",
                    "sources": []
                }
            ]
        }),
        LlmRequestKind::Research => serde_json::json!({
            "schema_version": MOCK_SCHEMA_VERSION,
            "mock": true,
            "topics": [
                {
                    "id": "mock-topic-1",
                    "query": "Placeholder research topic. Mock provider output.",
                    "status": "queued",
                    "sources": []
                }
            ]
        }),
    }
}

fn mock_complete(request: LlmRequest) -> Result<LlmResponse, LlmError> {
    if request.user_prompt.trim().is_empty() {
        return Err(LlmError::EmptyUserPrompt);
    }
    let response_id = mock_response_id(request.kind);
    let result = mock_result(request.kind);
    Ok(LlmResponse {
        response_id,
        kind: request.kind,
        model: MOCK_MODEL.to_string(),
        result,
        usage: LlmUsage {
            input_tokens: MOCK_INPUT_TOKENS,
            output_tokens: MOCK_OUTPUT_TOKENS,
        },
    })
}

impl LlmProvider for MockProvider {
    fn complete(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Future<Output = Result<LlmResponse, LlmError>> + Send + '_>> {
        Box::pin(async move { mock_complete(request) })
    }
}

// Shared provider state held by Tauri. Defaults to the mock provider.
pub(crate) struct LlmState {
    provider: Arc<dyn LlmProvider>,
}

impl LlmState {
    /// Clone of the current provider Arc, for dispatch sites outside this module.
    pub(crate) fn provider(&self) -> Arc<dyn LlmProvider> {
        self.provider.clone()
    }
}

impl Default for LlmState {
    fn default() -> Self {
        LlmState {
            provider: Arc::new(MockProvider::new()),
        }
    }
}

// Run a completion against the current provider. Errors are converted to safe
// strings via LlmError's sanitized Display; internal provider details and any
// secret material never reach the frontend.
#[tauri::command]
pub(crate) async fn complete_llm(
    state: tauri::State<'_, LlmState>,
    request: LlmRequest,
) -> Result<LlmResponse, String> {
    match state.provider.complete(request).await {
        Ok(response) => Ok(response),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests;
