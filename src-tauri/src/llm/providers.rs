// Provider catalog for model discovery. This module knows the HTTP details of
// each supported provider's /models endpoint: URL, auth style, and how to
// extract model ids from the JSON response. The validate_models command uses
// this catalog to check configured model strings against a provider's real
// available-models list.
//
// No API key material ever appears in a returned error string: request URLs
// and raw response bodies are never echoed into errors or logs. Query-key
// providers (Gemini) put the key in the URL, so transport errors must not
// carry the raw reqwest error text.

use serde_json::Value;
use std::time::Duration;

// How a provider expects its API key on the request.
enum AuthStyle {
    // Authorization: Bearer <key> (OpenAI, OpenRouter).
    BearerHeader,
    // x-api-key: <key> plus a fixed anthropic-version header (Anthropic).
    ApiKeyHeader { version_header: &'static str },
    // <url>?<param>=<key> (Gemini).
    QueryKey { param: &'static str },
}

// Where model ids live in the provider's JSON response.
enum IdPath {
    // data[].id (OpenAI, Anthropic, OpenRouter).
    OpenAi,
    // models[].name with the leading "models/" prefix stripped (Gemini).
    Gemini,
}

// Static description of one provider's /models endpoint.
struct ProviderSpec {
    models_url: &'static str,
    auth: AuthStyle,
    id_path: IdPath,
}

// Look up the catalog entry for a provider key. Returns None for unknown
// providers.
fn spec_for(provider: &str) -> Option<ProviderSpec> {
    match provider {
        "openai" => Some(ProviderSpec {
            models_url: "https://api.openai.com/v1/models",
            auth: AuthStyle::BearerHeader,
            id_path: IdPath::OpenAi,
        }),
        "anthropic" => Some(ProviderSpec {
            models_url: "https://api.anthropic.com/v1/models",
            auth: AuthStyle::ApiKeyHeader {
                version_header: "2023-06-01",
            },
            id_path: IdPath::OpenAi,
        }),
        "openrouter" => Some(ProviderSpec {
            models_url: "https://openrouter.ai/api/v1/models",
            auth: AuthStyle::BearerHeader,
            id_path: IdPath::OpenAi,
        }),
        "gemini" => Some(ProviderSpec {
            models_url: "https://generativelanguage.googleapis.com/v1beta/models",
            auth: AuthStyle::QueryKey { param: "key" },
            id_path: IdPath::Gemini,
        }),
        // DeepSeek's API is OpenAI-compatible: Bearer auth and a data[].id
        // response shape on GET https://api.deepseek.com/models.
        "deepseek" => Some(ProviderSpec {
            models_url: "https://api.deepseek.com/models",
            auth: AuthStyle::BearerHeader,
            id_path: IdPath::OpenAi,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The catalog covers every provider the example config and the UI can
    // name. Unknown provider keys resolve to None so validation treats them
    // as unresolvable instead of guessing an endpoint.
    #[test]
    fn spec_for_covers_known_providers() {
        for provider in ["openai", "anthropic", "openrouter", "gemini", "deepseek"] {
            assert!(spec_for(provider).is_some(), "missing provider: {provider}");
        }
        assert!(spec_for("unknown-provider").is_none());
    }
}

// Fetch the sorted, de-duplicated list of model ids a provider currently
// offers. The request uses the provider's auth style and a 30-second timeout.
// Error strings carry only the provider name and a safe reason: never the api
// key, the request URL, or the raw response body.
pub(crate) async fn fetch_available_models(
    provider: &str,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let spec = spec_for(provider).ok_or_else(|| format!("unknown provider: {provider}"))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| format!("provider {provider}: cannot build http client"))?;

    let mut request = client.get(spec.models_url);
    match spec.auth {
        AuthStyle::BearerHeader => {
            request = request.bearer_auth(api_key);
        }
        AuthStyle::ApiKeyHeader { version_header } => {
            request = request.header("x-api-key", api_key);
            request = request.header("anthropic-version", version_header);
        }
        AuthStyle::QueryKey { param } => {
            request = request.query(&[(param, api_key)]);
        }
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => {
            // The raw transport error may embed the request URL, which for
            // query-key providers contains the api key. Report only a safe
            // reason.
            let reason = if e.is_timeout() {
                "request timed out"
            } else if e.is_connect() {
                "connection failed"
            } else {
                "request failed"
            };
            return Err(format!("provider {provider}: {reason}"));
        }
    };

    let status = response.status();
    if !status.is_success() {
        return Err(format!("provider {provider}: HTTP {status}"));
    }

    let body = response
        .text()
        .await
        .map_err(|_| format!("provider {provider}: cannot read response body"))?;
    let json: Value = serde_json::from_str(&body)
        .map_err(|_| format!("provider {provider}: invalid JSON response"))?;

    let mut ids: Vec<String> = Vec::new();
    match spec.id_path {
        IdPath::OpenAi => {
            if let Some(items) = json.get("data").and_then(Value::as_array) {
                for item in items {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        ids.push(id.to_string());
                    }
                }
            }
        }
        IdPath::Gemini => {
            if let Some(items) = json.get("models").and_then(Value::as_array) {
                for item in items {
                    if let Some(name) = item.get("name").and_then(Value::as_str) {
                        ids.push(name.strip_prefix("models/").unwrap_or(name).to_string());
                    }
                }
            }
        }
    }

    ids.sort();
    ids.dedup();
    Ok(ids)
}