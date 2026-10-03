// Safe view of the merged config as produced by the Rust core. Contains no API
// key values, only provider names.
export interface ConfigStatus {
  global_loaded: boolean;
  project_loaded: boolean;
  project_path: string | null;
  small_model: string;
  large_model: string;
  // Named provider of each model, present only when the config used the
  // structured { provider, id } form; null for the legacy plain-string form.
  small_provider: string | null;
  large_provider: string | null;
  configured_providers: string[];
  error: string | null;
  debug: boolean;
  mock_llm: boolean;
}

// Backend event types for the "backend://event" channel. BackendEventType
// mirrors the Rust `EventType` enum in src-tauri/src/events.rs; adding a
// backend variant requires adding the TS union member and the
// KNOWN_EVENT_TYPES entry here (PER-31 will add concept-extraction types).
export type BackendEventType =
  | "watcher_file"
  | "block_diff"
  | "store_update"
  | "llm_call"
  | "config_reload";

export type BackendEventStatus = "queued" | "in_flight" | "done" | "failed";

export const KNOWN_EVENT_TYPES: readonly BackendEventType[] = [
  "watcher_file",
  "block_diff",
  "store_update",
  "llm_call",
  "config_reload",
];

// Reference to the file/block a backend event is about. Optional fields are
// null when unknown so the pane can distinguish "unknown" from "zero".
export interface BlockRef {
  filePath: string | null;
  blockHash: string;
  blockKind: string | null;
  blockStart: number | null;
  blockEnd: number | null;
  positionStart: number | null;
  positionEnd: number | null;
  excerpt: string | null;
}

// One backend event card payload. Same id = same card, updated in place by the
// pane. `createdAt` is set at first emission; `ts` refreshes on every
// emission; `durationMs` is set at the terminal status.
export interface BackendEvent {
  id: string;
  eventType: BackendEventType;
  status: BackendEventStatus;
  createdAt: number;
  ts: number;
  durationMs: number | null;
  model: string | null;
  estimatedCostUsd: number | null;
  finalCostUsd: number | null;
  block: BlockRef | null;
  summary: string;
  // Mirrors Rust Option<serde_json::Value>. Every current emitter sends a
  // JSON object; a future non-object detail would need a wider type here.
  detail: Record<string, unknown> | null;
}