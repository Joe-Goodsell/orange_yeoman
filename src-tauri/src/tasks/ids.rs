//! Deterministic task ids and the dedup identity for automatic tasks.

/// Generate a deterministic task id from a dedup identity.
/// Format: "{block_hash}:{trigger}:{kind}" lowercased and with non-alphanumeric chars replaced by '_'.
pub(crate) fn task_id_from_identity(
    block_hash: &str,
    trigger: &crate::pipeline::Trigger,
    kind: &crate::llm::LlmRequestKind,
) -> String {
    let trigger_str = match trigger {
        crate::pipeline::Trigger::Automatic => "automatic",
        crate::pipeline::Trigger::FactCheck => "fact_check",
        crate::pipeline::Trigger::Research => "research",
        // The inline-trigger variants never produce ids through this identity
        // form; inline dispatch uses pipeline::inline_task_id (see
        // submit_block). These arms only keep the match exhaustive.
        crate::pipeline::Trigger::Inline => "inline",
        crate::pipeline::Trigger::CommandLine => "command_line",
    };
    let kind_str = match kind {
        crate::llm::LlmRequestKind::Extraction => "extraction",
        crate::llm::LlmRequestKind::FactCheck => "fact_check",
        crate::llm::LlmRequestKind::Research => "research",
    };
    format!("{block_hash}:{trigger_str}:{kind_str}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

/// Build the task id for an explicit fact-check command. The base id from
/// task_id_from_identity is disambiguated with a hash of the claim text so
/// that two fact-checks of different claims in the same block get distinct
/// task ids instead of overwriting each other in the store.
pub(crate) fn fact_check_task_id(block_hash: &str, claim_text: &str) -> String {
    format!(
        "{}:{}",
        task_id_from_identity(
            block_hash,
            &crate::pipeline::Trigger::FactCheck,
            &crate::llm::LlmRequestKind::FactCheck,
        ),
        crate::pipeline::stable_hash(claim_text)
    )
}

/// Build the task id for an explicit research command. The base id from
/// task_id_from_identity is disambiguated with a hash of the goal so that two
/// research tasks with different goals in the same block get distinct task
/// ids instead of overwriting each other in the store.
pub(crate) fn research_task_id(block_hash: &str, goal: &str) -> String {
    format!(
        "{}:{}",
        task_id_from_identity(
            block_hash,
            &crate::pipeline::Trigger::Research,
            &crate::llm::LlmRequestKind::Research,
        ),
        crate::pipeline::stable_hash(goal)
    )
}

/// A dedup identity for automatic tasks. Two automatic tasks with the same identity
/// while queued or running should not both be dispatched.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DedupIdentity {
    pub(crate) block_hash: String,
    pub(crate) trigger: crate::pipeline::Trigger,
    pub(crate) kind: crate::llm::LlmRequestKind,
}

/// Build a dedup identity for an automatic task.
pub(crate) fn dedup_identity(
    block_hash: &str,
    trigger: crate::pipeline::Trigger,
    kind: crate::llm::LlmRequestKind,
) -> DedupIdentity {
    DedupIdentity {
        block_hash: block_hash.to_string(),
        trigger,
        kind,
    }
}