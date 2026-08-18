// Markdown syntax highlighting for the editor.
//
// This wires token coloring ONLY — it is not markdown rendering. The editor
// stays a plaintext writing surface; rendered/live preview remains a roadmap
// item per outline.md. The parser is Lezer markdown via
// @codemirror/lang-markdown's `markdown()` factory, which uses the commonmark
// grammar out of the box (headings, emphasis, strong, links, autolinks, lists,
// block quotes, inline and fenced code, thematic breaks, escapes). GFM
// extensions (strikethrough, tables, task lists, YAML frontmatter) are opt-in
// via `extensions: [GFM]` and are intentionally left out for now.
//
// Palette choice: the app follows the OS theme (`color-scheme: light dark` in
// app.css) with a #ffffff surface in light mode and a #1e1e1e surface in dark
// mode. Rather than maintain two HighlightStyle tables, the colors below are a
// single muted mid-tone palette. Each color's luminance sits between the two
// backgrounds, keeping contrast readable in both modes (roughly 3:1 to 5:1
// against either surface) while staying subtle enough for the distraction-free
// main pane. Weight and style carry the rest of the distinction: headings are
// semibold, emphasis is italic, strong is bold.
//
// Rule order matters: for nodes that carry multiple tags (e.g. the `#` of a
// heading carries both `heading` and `processingInstruction`), later rules
// take CSS precedence. The `processingInstruction` rule therefore sits before
// the `heading` rule so heading markers adopt the heading color, while list
// and code markers (which also carry `processingInstruction`) keep the quiet
// gray.

import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

const markdownHighlightStyle = HighlightStyle.define([
  // Emphasis: italic carries the distinction; color follows the body text.
  { tag: tags.emphasis, fontStyle: "italic" },
  // Strong: weight carries the distinction; color follows the body text.
  { tag: tags.strong, fontWeight: "bold" },
  // Links and their URLs: muted accent blue, clearly distinct from body text.
  { tag: [tags.link, tags.url], color: "#4a86c8" },
  // Inline code, fenced code bodies, and fence info labels: muted green.
  { tag: [tags.monospace, tags.labelName], color: "#3f7a54" },
  // Block quotes and thematic breaks: dimmed gray.
  { tag: [tags.quote, tags.contentSeparator], color: "#8a8a8a" },
  // List item bodies: muted violet, quieter than headings.
  { tag: tags.list, color: "#8a6da8" },
  // Backslash escapes: quiet gray, like other markup.
  { tag: tags.escape, color: "#8a8a8a" },
  // Markup markers (heading #, list -/*, quote >, link brackets, code
  // backticks, fence delimiters): quiet gray so markers recede and content
  // carries the color.
  { tag: tags.processingInstruction, color: "#8a8a8a" },
  // Headings: semibold slate. Declared last so the `#` markers (which also
  // carry processingInstruction) adopt the heading color.
  { tag: tags.heading, color: "#5b6e8c", fontWeight: "600" },
]);

/**
 * The markdown syntax highlighting extension for the editor: the Lezer
 * markdown parser plus a theme-aware token color scheme.
 */
export function markdownHighlight(): Extension {
  return [
    // codeLanguages is empty so fenced code blocks stay plain monospace text
    // (the editor is plaintext-only; per-language code highlighting is not in
    // scope).
    markdown({ codeLanguages: [] }),
    syntaxHighlighting(markdownHighlightStyle),
  ];
}