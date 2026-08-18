// CodeMirror autocompletion for slash commands. The source only offers
// options while the token before the cursor is a strict prefix of a registered
// command; the moment the token is a complete, bounded command the popup is
// closed. This matters because Enter must never be forced to choose between
// accepting a completion and dispatching the command: by the time the command
// word is complete, the popup is already gone.
//
// Uses the default autocompletion theme. Custom styling is roadmap.

import { autocompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import { SLASH_COMMANDS } from "./slashCommands";

/** True for ASCII whitespace, mirroring the Rust bounding rule. */
function isAsciiWhitespace(ch: string): boolean {
  return (
    ch === " " || ch === "\t" || ch === "\n" || ch === "\r" || ch === "\f" || ch === "\v"
  );
}

/** The slash-command autocompletion extension for the editor. */
export function slashCommandAutocomplete(): Extension {
  return autocompletion({
    override: [slashCommandSource],
    activateOnTyping: true,
  });
}

/**
 * Completion source: matches a `/`-prefixed token immediately before the
 * cursor when the slash is bounded by the line start or ASCII whitespace.
 * Offers the registered commands that start with the token while the token is
 * a strict prefix (not yet a complete bounded command); returns null
 * otherwise, closing any open popup.
 */
function slashCommandSource(context: CompletionContext): CompletionResult | null {
  const line = context.state.doc.lineAt(context.pos);
  const posInLine = context.pos - line.from;

  // Extend backward from the cursor to the start of the token.
  let start = posInLine;
  while (start > 0 && !isAsciiWhitespace(line.text[start - 1])) start--;
  const token = line.text.slice(start, posInLine);

  if (!token.startsWith("/")) return null;
  // The slash must be bounded: line start or preceded by ASCII whitespace.
  if (start > 0 && !isAsciiWhitespace(line.text[start - 1])) return null;

  const matches = SLASH_COMMANDS.filter((c) => c.startsWith(token));
  if (matches.length === 0) return null;

  // A strict prefix still has text to type; offer the matching commands. An
  // exact match means the command word is complete and bounded, so close the
  // popup (Enter will dispatch instead of accepting a completion).
  const isStrictPrefix = matches.some((c) => c.length > token.length);
  if (!isStrictPrefix) return null;

  return {
    from: line.from + start,
    options: matches.map((label) => ({ label, type: "command" })),
  };
}