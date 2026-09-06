//! Slash command model: token scanning, block parsing, and the v1 inline
//! command envelope. Commands are case-sensitive and lowercase.

use serde::{Deserialize, Serialize};

use super::blocks::{stable_hash, Block};
use super::routing::Trigger;

/// A slash command as defined by the app's command model. Commands are
/// case-sensitive and lowercase. Serialized as snake_case ("fact_check",
/// "research", "ignore") for the command envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SlashCommand {
    FactCheck,
    Research,
    Ignore,
}

/// The registry of inline command tokens. A token matches only when it is a
/// `/name` run bounded by whitespace or line edges, so "/researching" and
/// "path/fact-check" never match and "/Fact-Check" is not a command.
const COMMAND_REGISTRY: [(&str, SlashCommand); 3] = [
    ("/fact-check", SlashCommand::FactCheck),
    ("/research", SlashCommand::Research),
    ("/ignore", SlashCommand::Ignore),
];

/// A single inline command found in a line: the command, the byte span of its
/// token within the line, and the optional raw trailing `@selector` token with
/// its byte span. All offsets are relative to the line text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineCommand<'a> {
    pub(crate) command: SlashCommand,
    pub(crate) token_start: usize,
    pub(crate) token_end: usize,
    pub(crate) selector: Option<&'a str>,
    pub(crate) selector_span: Option<(usize, usize)>,
}

/// Scan a whole line for the first registry command token. The token may
/// appear anywhere in the line; it is matched case-sensitively and bounded by
/// whitespace or line edges. The optional selector is the first
/// whitespace-delimited token that starts with '@' and appears after the
/// command word. Pure: no allocations, no IO.
pub(crate) fn parse_slash_command_in_line(line: &str) -> Option<LineCommand<'_>> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            let rest = &line[i..];
            for (name, command) in COMMAND_REGISTRY {
                if rest.starts_with(name) {
                    let after = i + name.len();
                    if after == bytes.len() || bytes[after].is_ascii_whitespace() {
                        let (selector, selector_span) = first_at_token(line, after);
                        return Some(LineCommand {
                            command,
                            token_start: i,
                            token_end: after,
                            selector,
                            selector_span,
                        });
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// The first whitespace-delimited token at or after `from` that starts with
/// '@', returned with its byte span within the line.
fn first_at_token(line: &str, from: usize) -> (Option<&str>, Option<(usize, usize)>) {
    let bytes = line.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if bytes[start] == b'@' {
            return (Some(&line[start..i]), Some((start, i)));
        }
    }
    (None, None)
}

/// A parsed slash command: the command, the whole command line's text with the
/// command token and any trailing selector stripped and trimmed, the optional
/// selector, the command line's byte span within the block text, and the block
/// hash. The command line stays in the file; the app never deletes it, moves
/// it, or edits around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedSlashCommand {
    pub(crate) command: SlashCommand,
    pub(crate) focus_text: String,
    pub(crate) selector: Option<String>,
    pub(crate) line_start: usize, // byte offset of the command line within the block text
    pub(crate) line_end: usize,   // byte offset just past the command line within the block text
    pub(crate) block_hash: String,
}

/// Parse a slash command from a Markdown block. Scans the block's lines in
/// document order and returns the first line that contains a registry command.
/// Returns None for excluded blocks (FrontMatter, CodeFence, Html), so
/// slash-like text inside code fences is never treated as a command. All
/// offsets are relative to the block text; add `block.char_start` for file offsets.
pub(crate) fn parse_slash_command_in_block(block: &Block) -> Option<ParsedSlashCommand> {
    if block.excluded {
        return None;
    }
    let mut line_start = 0usize;
    for line in block.text.split_inclusive('\n') {
        let text = line.strip_suffix('\n').unwrap_or(line);
        if let Some(found) = parse_slash_command_in_line(text) {
            // Focus text: the whole command line with the command token and
            // any trailing selector stripped, then trimmed. The selector span
            // is shifted left by the removed token span before stripping.
            let mut focus_text = String::new();
            focus_text.push_str(&text[..found.token_start]);
            focus_text.push_str(&text[found.token_end..]);
            if let Some((sel_start, sel_end)) = found.selector_span {
                let shift = found.token_end - found.token_start;
                focus_text.replace_range(sel_start - shift..sel_end - shift, "");
            }
            let focus_text = focus_text.trim().to_string();
            return Some(ParsedSlashCommand {
                command: found.command,
                focus_text,
                selector: found.selector.map(|s| s.to_string()),
                line_start,
                line_end: line_start + text.len(),
                block_hash: block.block_hash.clone(),
            });
        }
        line_start += line.len();
    }
    None
}

/// The v1 command envelope: the uniform payload every command produces.
/// Field names serialize as camelCase for Tauri IPC; the command and trigger
/// serialize as snake_case. Phase 1 builds inline envelopes only; the dispatch
/// path consumes them in a later phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandEnvelope {
    pub(crate) command: SlashCommand,
    pub(crate) trigger: Trigger,
    pub(crate) scope: String,
    pub(crate) focus_ref: FocusRef,
    pub(crate) focus_text: String,
    pub(crate) selector: Option<String>,
    pub(crate) output_schema_version: u32,
}

/// Where a command applies. For v1 inline commands this is the command line's
/// byte span within the containing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusRef {
    pub(crate) file: Option<String>,
    pub(crate) block_hash: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Build the v1 inline-command envelope for a parsed command. Scope is always
/// "line"; the focus span is the command line's file-relative byte offsets.
pub(crate) fn build_inline_envelope(
    block: &Block,
    parsed: &ParsedSlashCommand,
    file_path: Option<&str>,
) -> CommandEnvelope {
    CommandEnvelope {
        command: parsed.command.clone(),
        trigger: Trigger::Inline,
        scope: "line".to_string(),
        focus_ref: FocusRef {
            file: file_path.map(|p| p.to_string()),
            block_hash: parsed.block_hash.clone(),
            start: block.char_start + parsed.line_start,
            end: block.char_start + parsed.line_end,
        },
        focus_text: parsed.focus_text.clone(),
        selector: parsed.selector.clone(),
        output_schema_version: 1,
    }
}

/// Task identity for an inline command: the hash of the containing block
/// hash, the command name, the focus text, and the resolved scope. The result
/// is a lowercase, non-slashy string suitable as a map key. The command name
/// uses the serde snake_case spelling ("fact_check", "research", "ignore").
/// The block hash is part of the identity so two notes whose identical line
/// "/fact-check some claim" lives in different blocks never collide in the
/// task store.
pub(crate) fn inline_task_id(envelope: &CommandEnvelope) -> String {
    let command_name = match envelope.command {
        SlashCommand::FactCheck => "fact_check",
        SlashCommand::Research => "research",
        SlashCommand::Ignore => "ignore",
    };
    let identity = format!(
        "{}:{}:{}:{}",
        envelope.focus_ref.block_hash, command_name, envelope.focus_text, envelope.scope
    );
    stable_hash(&identity)
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}