// Orange Yeoman - Markdown block pipeline. Parsing is delegated to the
// `markdown` crate (CommonMark + GFM + frontmatter); this module projects the
// resulting mdast tree into flat Blocks with exact byte offsets,
// heading chains, and exclusion flags.

mod blocks;
mod prompts;
mod routing;
mod slash;

pub(crate) use blocks::{
    block_kind_as_str, block_kind_from_str, parse_markdown_blocks, stable_hash, Block,
};
pub(crate) use prompts::{
    build_extraction_request, build_fact_check_request, build_research_request,
};
pub(crate) use routing::{route_block, RoutingDecision, Trigger};
pub(crate) use slash::{
    build_inline_envelope, inline_task_id, parse_slash_command_in_block, SlashCommand,
};

// Test-only re-exports. The sibling submodules import their items directly;
// the root re-exports these solely so pipeline/tests.rs can keep `use super::*;`
// and so sibling modules' test files can keep `use crate::pipeline::...`.
#[cfg(test)]
pub(crate) use blocks::BlockKind;
#[cfg(test)]
pub(crate) use prompts::{
    EXTRACTION_SYSTEM_PROMPT, FACT_CHECK_SYSTEM_PROMPT, RESEARCH_SYSTEM_PROMPT,
};
#[cfg(test)]
pub(crate) use routing::{classify_block, LocalSignals};
#[cfg(test)]
pub(crate) use slash::{parse_slash_command_in_line, ParsedSlashCommand};

#[cfg(test)]
mod tests;