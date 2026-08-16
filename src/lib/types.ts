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
  range: SourceRange;
  title: string;
  summary: string;
  // Full response text. Shown in the pane only when expanded; may be empty
  // while a task is queued/running.
  detail: string;
  createdAt: number;
  updatedAt: number;
}

// Immutable snapshot of the current editor selection. Collapsed selections are
// represented as null (no active selection to link against).
export interface EditorSelection {
  from: number;
  to: number;
}
