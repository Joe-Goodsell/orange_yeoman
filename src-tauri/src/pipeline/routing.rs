//! Local signal classification and block routing. Cheap local signals score a
//! Markdown block; the score and any explicit slash command produce a routing
//! decision and the trigger that caused it.

use serde::{Deserialize, Serialize};

use super::blocks::{Block, BlockKind};
use super::slash::{ParsedSlashCommand, SlashCommand};

const POSITIVE_SIGNAL_WEIGHT: i32 = 2;
const NEGATIVE_SIGNAL_WEIGHT: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalSignals {
    pub(crate) positive: u32,
    pub(crate) negative: u32,
    pub(crate) score: i32,
    pub(crate) matched: Vec<String>,
}

/// Compute cheap local signals for a Markdown block.
/// Returns zero signals (all fields empty/zero) when the block is excluded.
pub(crate) fn classify_block(block: &Block) -> LocalSignals {
    if block.excluded {
        return LocalSignals {
            positive: 0,
            negative: 0,
            score: 0,
            matched: vec![],
        };
    }

    let lower = block.text.to_lowercase();
    // Standalone-word matching: split on ASCII whitespace, lowercase each
    // token, strip leading/trailing ASCII punctuation, and drop empties.
    let tokens: Vec<String> = block
        .text
        .split_ascii_whitespace()
        .map(|t| t.to_lowercase())
        .map(|t| {
            t.trim_matches(|c: char| c.is_ascii_punctuation())
                .to_string()
        })
        .filter(|t| !t.is_empty())
        .collect();

    let mut positive: u32 = 0;
    let mut negative: u32 = 0;
    let mut matched: Vec<String> = Vec::new();
    let push = |matched: &mut Vec<String>, counter: &mut u32, name: &str| {
        matched.push(name.to_string());
        *counter += 1;
    };

    // Positive factual signals.
    if lower.chars().any(|c| c.is_ascii_digit()) {
        push(&mut matched, &mut positive, "number");
    }
    if lower.contains('%') {
        push(&mut matched, &mut positive, "percentage");
    }
    {
        // A date is a run of exactly four digits whose value lies in
        // 1000..=2999. Runs of any other length (including 5+ digit runs)
        // never count.
        let bytes = block.text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i - start == 4 {
                    if let Ok(year) = block.text[start..i].parse::<u32>() {
                        if (1000..=2999).contains(&year) {
                            push(&mut matched, &mut positive, "date");
                            break;
                        }
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    const UNITS: [&str; 14] = [
        "kg",
        "km",
        "cm",
        "mm",
        "celsius",
        "fahrenheit",
        "meters",
        "grams",
        "liters",
        "mph",
        "kph",
        "ms",
        "ghz",
        "mhz",
    ];
    if UNITS.iter().any(|u| lower.contains(u)) {
        push(&mut matched, &mut positive, "unit");
    }
    const DEFINITION_PHRASES: [&str; 4] = ["means", "refers to", "was defined as", "is defined as"];
    if DEFINITION_PHRASES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut positive, "definition");
    }
    const ATTRIBUTION_PHRASES: [&str; 4] = [
        "according to",
        "research shows",
        "found that",
        "studies show",
    ];
    if ATTRIBUTION_PHRASES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut positive, "attribution");
    }
    const CAUSAL_PHRASES: [&str; 4] = ["causes", "leads to", "results in", "because"];
    if CAUSAL_PHRASES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut positive, "causal");
    }
    const STRONG_QUANTIFIERS: [&str; 9] = [
        "first", "only", "largest", "smallest", "highest", "lowest", "most", "never", "always",
    ];
    if tokens
        .iter()
        .any(|t| STRONG_QUANTIFIERS.contains(&t.as_str()))
    {
        push(&mut matched, &mut positive, "strong_quantifier");
    }

    // Positive research signals.
    const QUESTION_WORDS: [&str; 7] = ["how", "why", "which", "what", "when", "where", "who"];
    if block.text.contains('?')
        || tokens
            .first()
            .is_some_and(|t| QUESTION_WORDS.contains(&t.as_str()))
    {
        push(&mut matched, &mut positive, "question");
    }
    const GAP_PHRASES: [&str; 5] = [
        "little is known",
        "unclear",
        "unknown",
        "not well understood",
        "needs examples",
    ];
    if GAP_PHRASES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut positive, "gap_phrase");
    }
    if block.kind == BlockKind::Heading {
        push(&mut matched, &mut positive, "topic_heading");
    }

    // Negative signals.
    const FIRST_PERSON_WORDS: [&str; 7] = ["i", "i'm", "i'll", "i've", "my", "mine", "me"];
    if tokens
        .iter()
        .any(|t| FIRST_PERSON_WORDS.contains(&t.as_str()))
    {
        push(&mut matched, &mut negative, "first_person");
    }
    const URL_PREFIXES: [&str; 3] = ["http://", "https://", "www."];
    if URL_PREFIXES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut negative, "url");
    }
    const SPECULATION_PHRASES: [&str; 5] = ["maybe", "perhaps", "i think", "probably", "guess"];
    if SPECULATION_PHRASES.iter().any(|p| lower.contains(p)) {
        push(&mut matched, &mut negative, "speculation");
    }

    let score =
        (positive as i32) * POSITIVE_SIGNAL_WEIGHT - (negative as i32) * NEGATIVE_SIGNAL_WEIGHT;

    LocalSignals {
        positive,
        negative,
        score,
        matched,
    }
}

const HIGH_SCORE_THRESHOLD: i32 = 4;
const MIDDLE_SCORE_THRESHOLD: i32 = 0;

/// The routing decision for a single Markdown block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RoutingDecision {
    HighConfidenceLocal,
    NeedsExtraction,
    Skip,
    FactCheck,
    Research,
    Ignore,
}

/// The trigger that caused a routing decision. Inline and CommandLine are part
/// of the v1 command model, but no dispatch path constructs them yet; the
/// inline-dispatch wiring arrives in a later phase.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Trigger {
    Automatic,
    FactCheck,
    Research,
    Inline,
    #[allow(dead_code)]
    CommandLine,
}

/// Route a Markdown block using its local signals and an optional parsed slash command.
/// Explicit slash commands always override the score. Excluded blocks are skipped.
/// Note: explicit commands apply to normal blocks; the excluded-block guard wins, so
/// an excluded block is `Skip` even when a command is present.
pub(crate) fn route_block(
    block: &Block,
    command: Option<&ParsedSlashCommand>,
) -> RoutingDecision {
    // The excluded-block guard wins over any command.
    if block.excluded {
        return RoutingDecision::Skip;
    }
    // Explicit slash commands bypass the score entirely.
    if let Some(cmd) = command {
        match cmd.command {
            SlashCommand::Ignore => return RoutingDecision::Ignore,
            SlashCommand::FactCheck => return RoutingDecision::FactCheck,
            SlashCommand::Research => return RoutingDecision::Research,
        }
    }
    let signals = classify_block(block);
    if signals.score >= HIGH_SCORE_THRESHOLD {
        return RoutingDecision::HighConfidenceLocal;
    }
    if signals.score >= MIDDLE_SCORE_THRESHOLD {
        return RoutingDecision::NeedsExtraction;
    }
    RoutingDecision::Skip
}

/// Map a RoutingDecision to the Trigger that caused it.
/// HighConfidenceLocal, NeedsExtraction, Skip => Automatic.
/// FactCheck => FactCheck. Research => Research. Ignore => Automatic.
pub(crate) fn decision_trigger(decision: &RoutingDecision) -> Trigger {
    match decision {
        RoutingDecision::FactCheck => Trigger::FactCheck,
        RoutingDecision::Research => Trigger::Research,
        RoutingDecision::HighConfidenceLocal
        | RoutingDecision::NeedsExtraction
        | RoutingDecision::Skip
        | RoutingDecision::Ignore => Trigger::Automatic,
    }
}