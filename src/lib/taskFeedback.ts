// Pure mapping helpers between the Rust task domain (TaskEvent, TaskResult,
// TaskMetadata) and the frontend AgentFeedback domain. No Svelte, no IPC, no
// DOM: these functions are shared by the project store and the editor so the
// conversion logic stays in one place.
//
// Byte offsets from Rust are UTF-8 byte offsets; the editor uses JavaScript
// string indices (UTF-16 code units). byteOffsetToCharOffset bridges the two.
// Rust byte offsets land on character boundaries, so the conversion is exact.

import type {
  FeedbackKind,
  FeedbackStatus,
  LlmRequestKind,
  TaskStatus,
} from "./types";

/**
 * Convert a UTF-8 byte offset within `text` to a JavaScript character offset
 * (UTF-16 code unit index). Rust byte offsets land on character boundaries.
 * Returns 0 for non-positive input and clamps to the string length.
 */
export function byteOffsetToCharOffset(text: string, byteOffset: number): number {
  if (byteOffset <= 0) return 0;
  const bytes = new TextEncoder().encode(text);
  if (byteOffset >= bytes.length) return text.length;
  const prefix = new TextDecoder("utf-8", { fatal: false }).decode(
    bytes.subarray(0, byteOffset),
  );
  return prefix.length;
}

/**
 * Map a Rust TaskStatus to a frontend FeedbackStatus.
 * completed -> arrived, failed -> error; the rest map directly.
 */
export function taskStatusToFeedbackStatus(s: TaskStatus): FeedbackStatus {
  switch (s) {
    case "queued":
      return "queued";
    case "running":
      return "running";
    case "completed":
      return "arrived";
    case "failed":
      return "error";
    case "stale":
      return "stale";
  }
}

/**
 * Map a Rust LlmRequestKind to a frontend FeedbackKind.
 * extraction maps to correction (the auto-extraction correction path).
 */
export function requestKindToFeedbackKind(k: LlmRequestKind): FeedbackKind {
  switch (k) {
    case "fact_check":
      return "fact-check";
    case "research":
      return "research";
    case "extraction":
      return "correction";
  }
}

/**
 * Map a slash command name (e.g. "/fact-check") to a FeedbackKind, or null
 * when the name is not a recognized command.
 */
export function commandNameToFeedbackKind(name: string): FeedbackKind | null {
  switch (name) {
    case "/fact-check":
      return "fact-check";
    case "/research":
      return "research";
    case "/ignore":
      return "ignore";
    default:
      return null;
  }
}

/**
 * A short title for a feedback card of the given kind.
 */
export function titleForKind(kind: FeedbackKind): string {
  switch (kind) {
    case "fact-check":
      return "Fact-check";
    case "research":
      return "Research";
    case "correction":
      return "Correction";
    case "ignore":
      return "Ignore";
  }
}

/**
 * Extract a human-readable summary and detail from a task result payload.
 *
 * The Rust MockProvider returns one generic shape for every request kind:
 * { schema_version, mock: true, text }. The checks/topics/claims branches
 * below describe expected real-provider result shapes and stay for that
 * future.
 *
 * For errors, the error string becomes the detail. For null or unknown
 * shapes, a generic fallback is returned so nothing is silently dropped.
 */
export function describeTaskResult(
  result: unknown,
  error: string | null,
): { summary: string; detail: string } {
  if (error) {
    return {
      summary: "The provider request failed. Retry to attempt again.",
      detail: error,
    };
  }
  if (result == null) {
    return { summary: "No result returned.", detail: "" };
  }

  const parsed = result as Record<string, unknown>;
  const isMock = parsed.mock === true;

  // Generic mock shape from the Rust MockProvider: { schema_version, mock,
  // text }. Shown as readable text instead of a raw JSON dump.
  if (isMock && typeof parsed.text === "string") {
    return { summary: "Mock provider output.", detail: parsed.text };
  }

  // Fact-check: array of checks with claim + verdict.
  if (Array.isArray(parsed.checks)) {
    const checks = parsed.checks as Array<Record<string, unknown>>;
    if (checks.length === 0) {
      return { summary: "No claims checked.", detail: "" };
    }
    const lines = checks.map((c, i) => {
      const claim = String(c.claim ?? "(no claim)");
      const verdict = String(c.verdict ?? "unknown");
      return `${i + 1}. ${claim} — verdict: ${verdict}.`;
    });
    return {
      summary: isMock ? "Mock fact-check complete." : "Fact-check complete.",
      detail: lines.join("\n"),
    };
  }

  // Research: array of topics with query + status.
  if (Array.isArray(parsed.topics)) {
    const topics = parsed.topics as Array<Record<string, unknown>>;
    if (topics.length === 0) {
      return { summary: "No research topics.", detail: "" };
    }
    const lines = topics.map((t, i) => {
      const query = String(t.query ?? "(no topic)");
      const status = String(t.status ?? "unknown");
      return `${i + 1}. ${query} — status: ${status}.`;
    });
    return {
      summary: isMock ? "Mock research complete." : "Research complete.",
      detail: lines.join("\n"),
    };
  }

  // Extraction: array of claims with text + confidence.
  if (Array.isArray(parsed.claims)) {
    const claims = parsed.claims as Array<Record<string, unknown>>;
    if (claims.length === 0) {
      return { summary: "No claims extracted.", detail: "" };
    }
    const lines = claims.map((c, i) => {
      const text = String(c.text ?? "(no text)");
      const confidence = String(c.confidence ?? "unknown");
      return `${i + 1}. ${text} — confidence: ${confidence}.`;
    });
    return {
      summary: isMock ? "Mock extraction complete." : "Extraction complete.",
      detail: lines.join("\n"),
    };
  }

  // Unknown shape: surface the raw JSON so nothing is silently dropped.
  return {
    summary: "Result received.",
    detail: safeStringify(result),
  };
}

function safeStringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}