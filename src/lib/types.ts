export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
}

export interface ChangeEvent {
  path: string;
  kind: "structure" | "content";
  ts: number;
}

// Safe view of the merged config as produced by the Rust core. Contains no API
// key values, only provider names.
export interface ConfigStatus {
  global_loaded: boolean;
  project_loaded: boolean;
  project_path: string | null;
  small_model: string;
  large_model: string;
  configured_providers: string[];
  error: string | null;
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

// Task domain types. Field names are camelCase because the Rust core
// serializes them with serde rename_all = "camelCase". Status and trigger
// values are lowercase snake_case strings matching the Rust enums.

export type TaskStatus = "queued" | "running" | "completed" | "failed" | "stale";

export type Trigger = "automatic" | "fact_check" | "research";

export interface TaskMetadata {
  taskId: string;
  filePath: string | null;
  blockHash: string;
  sourceHash: string;
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
  stale: boolean;
  result: unknown | null;
  error: string | null;
}
