export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface ChangeEvent {
  path: string;
  kind: "structure" | "content";
  ts: number;
  // Content hash of the new on-disk bytes (pipeline::stable_hash). Present
  // only on content events, so the store can recognize the editor's own writes.
  hash?: string;
}

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

// Payload emitted by Rust on the "debug://event" channel. Error and warning
// events always arrive; info and below arrive only when the `debug` config
// flag is true. `ts` is stamped in Rust at emit time (ms since UNIX epoch).
export interface DebugEvent {
  category: string;
  message: string;
  level: "error" | "warn" | "info" | "debug" | "trace";
  ts: number;
}

// LLM boundary types. Field names are camelCase because the Rust core
// serializes them with serde rename_all = "camelCase". Results from the mock
// provider are structured JSON marked mock: true; they are never authoritative
// or sourced.

export type LlmRequestKind = "extraction" | "fact_check" | "research";

export interface LlmRequest {
  kind: LlmRequestKind;
  model?: string;
  systemPrompt?: string;
  userPrompt: string;
  maxTokens?: number;
}

export interface LlmUsage {
  inputTokens: number;
  outputTokens: number;
}

export interface LlmResponse {
  responseId: string;
  kind: LlmRequestKind;
  model: string;
  result: unknown;
  usage: LlmUsage;
}

// ---- Agent feedback ----
// Feedback items surface in the right-hand pane and link back to the source
// range they refer to in the editor. The model is transport-agnostic: the LLM
// transport that produces these items is out of scope for now, and the UI only
// consumes this shape.

export type FeedbackKind = "research" | "fact-check" | "correction" | "ignore";

// Lifecycle of a feedback item. `stale` means the referenced file changed
// after the feedback was produced, so it must not be presented as
// authoritative.
export type FeedbackStatus =
  | "queued"
  | "running"
  | "arrived"
  | "stale"
  | "error";

// Source range within a single file, as 0-based character offsets recorded at
// the moment the feedback was produced. Offsets are only valid against the
// snapshot of the file they refer to; after edits they may no longer match the
// on-disk content, which is what `status: "stale"` communicates.
export interface SourceRange {
  file: string;
  from: number;
  to: number;
}

export interface AgentFeedback {
  id: string;
  kind: FeedbackKind;
  status: FeedbackStatus;
  // Model provenance: the provider and exact model that produced this item,
  // recorded at completion time (e.g. provider "openai", model
  // "gpt-5.6-luna"). Later configuration changes do not alter history.
  provider: string;
  model: string;
  range: SourceRange;
  title: string;
  summary: string;
  // Full response text. Shown in the pane only when expanded; may be empty
  // while a task is queued/running.
  detail: string;
  createdAt: number;
  updatedAt: number;
}

// Immutable snapshot of the current editor selection. A collapsed selection
// (from === to) is a caret position and links feedback ranges that contain it.
export interface EditorSelection {
  from: number;
  to: number;
}

// Task domain types. Field names are camelCase because the Rust core
// serializes them with serde rename_all = "camelCase". Status and trigger
// values are lowercase snake_case strings matching the Rust enums.

export type TaskStatus = "queued" | "running" | "completed" | "failed" | "stale";

export type Trigger =
  | "automatic"
  | "fact_check"
  | "research"
  | "inline"
  | "command_line";

export interface TaskMetadata {
  taskId: string;
  filePath: string | null;
  blockHash: string;
  sourceHash: string;
  // File-relative byte span of the inline command line. Only present for
  // inline-trigger tasks (focusStart/focusEnd); absent for all others.
  // These are BYTE offsets relative to the file, not character offsets. The
  // frontend SourceRange uses character offsets, so conversion is needed when
  // wiring the editor.
  focusStart?: number;
  focusEnd?: number;
  // Stable hash of the raw inline command line text. Line-granular staleness
  // key: an edit to the command line itself invalidates the result, while
  // edits to sibling lines in the same block do not. Only present for
  // inline-trigger tasks.
  lineTextHash?: string;
  trigger: Trigger;
  status: TaskStatus;
  stale: boolean;
  error: string | null;
}

export interface TaskEvent {
  taskId: string;
  status: TaskStatus;
  kind: LlmRequestKind;
  stale: boolean;
  error: string | null;
}

export interface TaskResult {
  taskId: string;
  status: TaskStatus;
  kind: LlmRequestKind;
  stale: boolean;
  result: unknown | null;
  error: string | null;
}
