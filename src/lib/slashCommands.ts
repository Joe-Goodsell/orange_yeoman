// Pure slash-command helpers shared by the editor wiring. No IPC, no Svelte,
// no DOM: these functions mirror the Rust command model in
// src-tauri/src/pipeline.rs (COMMAND_REGISTRY and parse_slash_command_in_line)
// and the block layout produced by parse_markdown_blocks.
//
// Index convention: all offsets in this module are JavaScript string indices
// (UTF-16 code units). The Rust parser uses byte offsets; the two agree for
// ASCII content, which is the common case for command lines. Non-ASCII text
// before a token may make the offsets diverge; that is accepted for now.

/** The registered slash commands, mirroring Rust COMMAND_REGISTRY order. */
export const SLASH_COMMANDS = ["/fact-check", "/research", "/ignore"] as const;

/** A command token found in a line, with its UTF-16 span within the line. */
export interface DetectedCommand {
  name: string;
  tokenStart: number;
  tokenEnd: number;
}

/** A paragraph slice of a document: its text and its UTF-16 start offset. */
export interface ParagraphSlice {
  text: string;
  startOffset: number;
}

/**
 * True for ASCII whitespace, matching Rust `char::is_ascii_whitespace`.
 * JavaScript `\s` also matches non-ASCII spaces, so an explicit set keeps the
 * behavior identical to the Rust parser.
 */
function isAsciiWhitespace(ch: string): boolean {
  return (
    ch === " " || ch === "\t" || ch === "\n" || ch === "\r" || ch === "\f" || ch === "\v"
  );
}

/**
 * Scan a line for the first registered command token. A token is a `/name` run
 * that begins at the line start or after ASCII whitespace, matches a registered
 * command case-sensitively, and is followed by end-of-line or ASCII whitespace.
 * Returns the first match in document order, or null when none exists.
 * Non-matches: "/researching" (unbounded), "path/fact-check" (slash not
 * bounded), "/Fact-Check" (case-sensitive).
 */
export function detectSlashCommandInLine(line: string): DetectedCommand | null {
  for (let i = 0; i < line.length; i++) {
    if (line[i] !== "/") continue;
    if (i > 0 && !isAsciiWhitespace(line[i - 1])) continue;
    for (const name of SLASH_COMMANDS) {
      if (!line.startsWith(name, i)) continue;
      const after = i + name.length;
      if (after === line.length || isAsciiWhitespace(line[after])) {
        return { name, tokenStart: i, tokenEnd: after };
      }
    }
  }
  return null;
}

/**
 * Return the blank-line-bounded paragraph containing `pos`: the maximal run of
 * non-empty lines around `pos`, bounded by empty lines or the string edges.
 * A line counts as empty only when its text is exactly "", mirroring Rust's
 * block separator rule in parse_markdown_blocks. When `pos` sits on an empty
 * line (for example a fresh line below a paragraph), the slice is empty, so a
 * cursor on a blank line never resolves to a neighboring paragraph. The text
 * is the run's lines joined with "\n" (no trailing newline), which equals the
 * Rust paragraph block text. `startOffset` is the UTF-16 index where the
 * paragraph begins in `doc`.
 */
export function paragraphAround(doc: string, pos: number): ParagraphSlice {
  const lines: { start: number; text: string }[] = [];
  let start = 0;
  for (let i = 0; i < doc.length; i++) {
    if (doc.charCodeAt(i) === 10 /* \n */) {
      lines.push({ start, text: doc.slice(start, i) });
      start = i + 1;
    }
  }
  lines.push({ start, text: doc.slice(start) });

  if (lines.length === 0) return { text: "", startOffset: 0 };

  // The last line whose start is at or before pos contains the cursor.
  let lineIdx = lines.length - 1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].start <= pos) lineIdx = i;
    else break;
  }

  // A cursor on an empty line belongs to no paragraph.
  if (lines[lineIdx].text === "") {
    return { text: "", startOffset: lines[lineIdx].start };
  }

  let lo = lineIdx;
  while (lo > 0 && lines[lo - 1].text !== "") lo--;
  let hi = lineIdx;
  while (hi < lines.length - 1 && lines[hi + 1].text !== "") hi++;

  const startOffset = lines[lo].start;
  const text = lines
    .slice(lo, hi + 1)
    .map((l) => l.text)
    .join("\n");
  return { text, startOffset };
}

/**
 * Stable FNV-1a 32-bit hash over UTF-16 code units, returned as a lowercase
 * hex string. Deterministic across runs; used to build dispatch dedup keys.
 */
export function hashText(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16);
}