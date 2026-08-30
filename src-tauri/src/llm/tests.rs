use super::*;

// Poll a future to completion with a noop waker. The mock future is always
// immediately ready, so this never spins.
fn block_on<F: Future<Output = T>, T>(future: F) -> T {
    let waker = noop_waker();
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn noop_waker() -> std::task::Waker {
    use std::task::{RawWaker, RawWakerVTable};
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    fn wake(_: *const ()) {}
    fn wake_by_ref(_: *const ()) {}
    fn drop(_: *const ()) {}
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { std::task::Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

fn request(kind: LlmRequestKind, user_prompt: &str) -> LlmRequest {
    LlmRequest {
        kind,
        model: None,
        system_prompt: None,
        user_prompt: user_prompt.to_string(),
        max_tokens: None,
    }
}

fn complete(kind: LlmRequestKind, user_prompt: &str) -> Result<LlmResponse, LlmError> {
    let provider = MockProvider::new();
    block_on(provider.complete(request(kind, user_prompt)))
}

// Each request kind returns the same flat schema shape: a top-level
// schema_version, the mock marker, and readable placeholder text.
#[test]
fn each_request_kind_returns_correct_schema_shape() {
    for kind in [
        LlmRequestKind::Extraction,
        LlmRequestKind::FactCheck,
        LlmRequestKind::Research,
    ] {
        let response =
            complete(kind, "example prompt").expect("mock must succeed");
        assert_eq!(response.result["schema_version"], 1);
        assert_eq!(response.result["mock"], true);
        let text = response.result["text"]
            .as_str()
            .expect("text string");
        assert!(!text.is_empty(), "text must be non-empty");
        assert!(
            text.contains("Lorem ipsum"),
            "text must contain readable placeholder: {text}"
        );
    }
}

// Equivalent request kinds (same kind, same prompt) return the same stable
// response ID.
#[test]
fn equivalent_request_kinds_return_stable_response_ids() {
    let first = complete(LlmRequestKind::Research, "prompt one").expect("mock must succeed");
    let second = complete(LlmRequestKind::Research, "prompt one").expect("mock must succeed");
    assert_eq!(first.response_id, second.response_id);
    assert!(
        !first.response_id.is_empty(),
        "response ID must not be empty"
    );
}

// Different request kinds return different response IDs.
#[test]
fn different_request_kinds_return_different_response_ids() {
    let extraction = complete(LlmRequestKind::Extraction, "example").expect("mock must succeed");
    let fact_check = complete(LlmRequestKind::FactCheck, "example").expect("mock must succeed");
    let research = complete(LlmRequestKind::Research, "example").expect("mock must succeed");

    assert_ne!(extraction.response_id, fact_check.response_id);
    assert_ne!(fact_check.response_id, research.response_id);
    assert_ne!(extraction.response_id, research.response_id);
}

// An empty or whitespace-only user prompt is rejected with a safe error, and
// the error text never echoes the prompt.
#[test]
fn empty_user_prompt_is_rejected() {
    for prompt in ["", "   ", "\n\t "] {
        let err =
            complete(LlmRequestKind::Research, prompt).expect_err("empty prompt must be rejected");
        assert!(matches!(err, LlmError::EmptyUserPrompt));
        assert!(
            err.to_string().contains("user prompt must not be empty"),
            "unexpected error text: {err}"
        );
    }
}

// Arbitrary prompt content is never copied into the mock result.
#[test]
fn arbitrary_prompt_content_is_not_copied_into_result() {
    let marker = "ZYXW-PROMPT-MARKER-9876";
    let response = complete(LlmRequestKind::Research, marker).expect("mock must succeed");
    let serialized = serde_json::to_string(&response.result).expect("serialize result");
    assert!(
        !serialized.contains(marker),
        "prompt content leaked into result: {serialized}"
    );
}

// Mock usage is present and non-negative.
#[test]
fn mock_usage_is_present_and_non_negative() {
    let response = complete(LlmRequestKind::Extraction, "example").expect("mock must succeed");
    assert!(
        response.usage.input_tokens > 0,
        "input tokens must be present"
    );
    assert!(
        response.usage.output_tokens > 0,
        "output tokens must be present"
    );
}

// An API-key-like secret supplied in the prompt never reaches the result or
// any error string produced by the mock.
#[test]
fn secret_in_prompt_never_reaches_result_or_error() {
    let secret = "sk-live-secret-1234";
    let prompt = format!("check this claim {secret}");
    let response = complete(LlmRequestKind::Research, &prompt).expect("mock must succeed");
    let serialized = serde_json::to_string(&response).expect("serialize response");
    assert!(
        !serialized.contains(secret),
        "secret leaked into response: {serialized}"
    );

    // The sanitized error path scrubs key-like tokens even if a provider
    // passes a raw secret through.
    let err = LlmError::Provider(format!("request failed with {secret}"));
    assert!(
        !err.to_string().contains(secret),
        "secret leaked into error: {err}"
    );
    assert!(
        err.to_string().contains("sk-[REDACTED]"),
        "redaction marker missing: {err}"
    );
}

// The frontend-facing response serializes with camelCase field names.
#[test]
fn response_json_uses_camel_case_field_names() {
    let response = complete(LlmRequestKind::FactCheck, "example").expect("mock must succeed");
    let value = serde_json::to_value(&response).expect("serialize response");
    let object = value.as_object().expect("response must be an object");
    assert!(object.contains_key("responseId"), "missing responseId");
    assert!(object.contains_key("kind"), "missing kind");
    assert!(object.contains_key("model"), "missing model");
    assert!(object.contains_key("result"), "missing result");
    let usage = object["usage"]
        .as_object()
        .expect("usage must be an object");
    assert!(usage.contains_key("inputTokens"), "missing inputTokens");
    assert!(usage.contains_key("outputTokens"), "missing outputTokens");
}

// select_provider(true) installs the mock, which completes successfully.
#[test]
fn select_provider_true_uses_mock() {
    let provider = select_provider(true);
    let response = block_on(provider.complete(request(LlmRequestKind::Research, "prompt")))
        .expect("mock provider must succeed");
    assert_eq!(response.result["mock"], true);
}

// select_provider(false) installs the real provider stub. A non-empty
// prompt returns the not-yet-available error; an empty prompt returns
// EmptyUserPrompt. The error text never echoes the prompt.
#[test]
fn select_provider_false_real_provider_errors() {
    let provider = select_provider(false);
    let err = block_on(provider.complete(request(LlmRequestKind::Research, "prompt")))
        .expect_err("real provider stub must error");
    assert!(matches!(err, LlmError::Provider(_)));
    let msg = err.to_string();
    assert!(msg.contains("not yet available"), "unexpected error: {msg}");
    assert!(!msg.contains("prompt"), "error echoes prompt: {msg}");

    let empty_err = block_on(provider.complete(request(LlmRequestKind::Research, "   ")))
        .expect_err("empty prompt must be rejected");
    assert!(matches!(empty_err, LlmError::EmptyUserPrompt));
}
