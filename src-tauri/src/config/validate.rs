//! Pure model-validation helpers. The message format is testable without
//! network I/O.

// Validate two configured model strings against the available-models union
// fetched from a provider. Returns one message per problem; an empty result
// means both models are valid. The available list is sorted and comma-joined
// in each message. This function is pure so the message format is testable
// without network I/O.
pub(crate) fn validate_model_strings(small: &str, large: &str, available: &[String]) -> Vec<String> {
    let mut sorted: Vec<&str> = available.iter().map(String::as_str).collect();
    sorted.sort();
    sorted.dedup();
    let list = if sorted.is_empty() {
        "(none)".to_string()
    } else {
        sorted.join(", ")
    };

    let mut messages = Vec::new();
    if small.is_empty() {
        messages.push("No small model configured".to_string());
    } else if !sorted.contains(&small) {
        messages.push(format!("Invalid model {small}. Available models: {list}"));
    }
    if large.is_empty() {
        messages.push("No large model configured".to_string());
    } else if !sorted.contains(&large) {
        messages.push(format!("Invalid model {large}. Available models: {list}"));
    }
    messages
}

// Build the user-facing messages for one structured { provider, id } entry.
// `available` is the provider's fetched model list: None when the list is
// unavailable (fetch failed), Some(list) when the fetch succeeded. This
// function is pure so the message format is testable without network I/O.
pub(crate) fn validate_structured_model(
    _slot: &str,
    provider: &str,
    id: &str,
    available: Option<&[String]>,
) -> Vec<String> {
    match available {
        None => vec![format!(
            "could not check model {id}: provider {provider} did not respond"
        )],
        Some(list) => {
            let mut sorted: Vec<&str> = list.iter().map(String::as_str).collect();
            sorted.sort();
            sorted.dedup();
            let joined = if sorted.is_empty() {
                "(none)".to_string()
            } else {
                sorted.join(", ")
            };
            if sorted.contains(&id) {
                Vec::new()
            } else {
                vec![format!(
                    "Invalid model {id} for provider {provider}. Available models: {joined}"
                )]
            }
        }
    }
}

// Message for a structured entry whose named provider has no API key
// configured. The slot (small/large) is labeled so the message identifies
// which model slot the problem belongs to.
pub(crate) fn unconfigured_provider_message(slot: &str, provider: &str) -> String {
    format!("provider {provider} has no API key configured for the {slot} model")
}