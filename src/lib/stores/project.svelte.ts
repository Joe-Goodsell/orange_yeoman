import {
  loadProjectConfig as loadProjectConfigCmd,
  readTextFile,
  writeTextFile,
  onTaskUpdated,
  onResultReady,
  getTaskStatus,
} from "../tauri";
import type {
  AgentFeedback,
  ChangeEvent,
  ConfigStatus,
  EditorSelection,
  FeedbackKind,
  FeedbackStatus,
  SourceRange,
  TaskEvent,
  TaskResult,
} from "../types";
import {
  describeTaskResult,
  requestKindToFeedbackKind,
  taskStatusToFeedbackStatus,
  titleForKind,
} from "../taskFeedback";

const STORAGE_KEY = "orange-yeoman:project-root";

function loadRoot(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

class ProjectStore {
  projectRoot = $state<string | null>(loadRoot());
  openFilePath = $state<string | null>(null);
  openFileContent = $state<string>("");
  dirty = $state<boolean>(false);
  // Whether openFileContent came from the editor's own save ("editor") or from
  // a load/external source ("external"). The editor's doc-sync effect uses
  // this to decide whether a content change is an external load (replace the
  // document) or the editor's own save (local buffer wins — a save must never
  // revert local edits made while the write IPC was in flight).
  contentOrigin = $state<"editor" | "external">("external");
  loading = $state<boolean>(false);
  // True while a save IPC is in flight; the editor header shows it.
  saving = $state<boolean>(false);
  error = $state<string | null>(null);
  // Separate config error state. Kept apart from `error` so a config reload
  // never erases unrelated file/watcher errors.
  configError = $state<string | null>(null);
  watching = $state<boolean>(loadRoot() !== null);
  changeFeed = $state<ChangeEvent[]>([]);
  structureVersion = $state<number>(0);
  // Safe merged config status. Never contains API key values.
  configStatus = $state<ConfigStatus | null>(null);

  // ---- Agent feedback state (mock) ----
  // The LLM transport that will populate this state is out of scope. The
  // actions below are the surface the real transport will drive later; for now
  // they are fed by `seedMockFeedback()`.
  feedback = $state<AgentFeedback[]>([]);
  // Feedback item currently revealed/linked from either pane. The editor
  // reacts to this by selecting and scrolling to the item's source range.
  selectedFeedbackId = $state<string | null>(null);
  // Snapshot of the current editor selection, used to highlight feedback
  // cards whose source range overlaps the selection.
  editorSelection = $state<EditorSelection | null>(null);

  // Feedback items whose source range overlaps the current editor selection
  // (same file only). Drives the "linked" highlight in the agent pane. A
  // collapsed selection is a caret position and links any range containing it.
  linkedFeedbackIds = $derived(
    (() => {
      const sel = this.editorSelection;
      if (!sel || !this.openFilePath) return [];
      return this.feedback
        .filter((f) => {
          if (f.range.file !== this.openFilePath) return false;
          if (sel.from === sel.to) {
            // Caret linking: the caret position sits inside the range. The
            // left boundary is inclusive so a caret at the range start links.
            return f.range.from <= sel.from && sel.from < f.range.to;
          }
          return f.range.from < sel.to && f.range.to > sel.from;
        })
        .map((f) => f.id);
    })()
  );

  private gen = 0;

  // File path -> content hash of the last successful save made by this editor.
  // Watcher content events carry the same hash, so pushChange can recognize
  // the editor's own writes and skip the stale pass for them.
  private selfWrites: Record<string, string> = {};
  // Serialize saves: each save queues behind the previous one so concurrent
  // saves to the same file cannot interleave writes; last content wins.
  private saveChain: Promise<unknown> = Promise.resolve();

  // ---- Task event subscriptions ----
  private taskUnlisteners: Array<() => void> = [];
  private taskSubsAlive = false;

  applyConfigStatus(status: ConfigStatus) {
    this.configStatus = status;
    // Assign even when null so a successful reload clears a stale config error
    // without touching `error`, which belongs to file/watcher failures.
    this.configError = status.error;
  }

  // Load (root = path) or clear (root = null) the project config in Rust and
  // refresh the merged config status. The response is ignored if the root
  // changed while the IPC call was in flight.
  async loadProjectConfig(root: string | null) {
    const startGen = this.gen;
    try {
      const status = await loadProjectConfigCmd(root);
      if (startGen !== this.gen) return null;
      this.applyConfigStatus(status);
      return status;
    } catch (e) {
      if (startGen !== this.gen) return null;
      this.configError = String(e);
      return null;
    }
  }

  selectRoot(path: string) {
    this.gen++;
    this.projectRoot = path;
    this.openFilePath = null;
    this.openFileContent = "";
    this.dirty = false;
    this.loading = false;
    this.error = null;
    this.watching = true;
    this.changeFeed = [];
    this.structureVersion = 0;
    // No file is open under the new root; prune saved-write records and reset
    // the content origin so the next open file is treated as an external load.
    this.selfWrites = {};
    this.contentOrigin = "external";
    this.resetFeedback();
    try {
      localStorage.setItem(STORAGE_KEY, path);
    } catch {
      // localStorage may be unavailable; ignore
    }
    // Load the repository config for the newly selected root.
    void this.loadProjectConfig(path);
  }

  clearProject() {
    this.gen++;
    this.projectRoot = null;
    this.openFilePath = null;
    this.openFileContent = "";
    this.dirty = false;
    this.loading = false;
    this.error = null;
    this.watching = false;
    this.changeFeed = [];
    this.structureVersion = 0;
    // No file is open after clearing; prune saved-write records and reset the
    // content origin so the next open file is treated as an external load.
    this.selfWrites = {};
    this.contentOrigin = "external";
    this.configStatus = null;
    this.configError = null;
    this.resetFeedback();
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // ignore
    }
    // Reset the project portion of the Rust config state.
    void this.loadProjectConfig(null);
  }

  pushChange(e: ChangeEvent) {
    this.changeFeed = [e, ...this.changeFeed].slice(0, 100);
    if (e.kind === "structure") this.structureVersion++;
    // A content event whose hash matches this editor's last save for that file
    // is a self-write: the editor already maps feedback ranges locally on every
    // keystroke, so the save must not mark every item stale.
    const isSelfWrite =
      e.kind === "content" &&
      e.hash !== undefined &&
      this.selfWrites[e.path] === e.hash;
    // Feedback ranges are anchored to the snapshot of their file. When that
    // file changes on disk (external edit), the item can no longer be
    // presented as authoritative; mark it stale so the pane says so and the
    // user can re-run it. `error` items already communicate a failure.
    if (e.kind === "content" && !isSelfWrite) {
      this.feedback = this.feedback.map((f) =>
        f.range.file === e.path && f.status !== "stale" && f.status !== "error"
          ? { ...f, status: "stale", updatedAt: Date.now() }
          : f
      );
    }
  }

  // ---- Agent feedback actions (mock) ----

  resetFeedback() {
    this.feedback = [];
    this.selectedFeedbackId = null;
    this.editorSelection = null;
  }

  addFeedback(item: AgentFeedback) {
    if (this.feedback.some((f) => f.id === item.id)) return;
    this.feedback = [...this.feedback, item];
  }

  updateFeedback(id: string, patch: Partial<AgentFeedback>) {
    this.feedback = this.feedback.map((f) =>
      f.id === id ? { ...f, ...patch, updatedAt: Date.now() } : f
    );
  }

  removeFeedback(id: string) {
    this.feedback = this.feedback.filter((f) => f.id !== id);
    if (this.selectedFeedbackId === id) this.selectedFeedbackId = null;
  }

  clearFeedback() {
    this.feedback = [];
    this.selectedFeedbackId = null;
  }

  // Link the editor to a feedback item (or clear the link). The editor reacts
  // by selecting and scrolling to the item's source range.
  focusFeedback(id: string | null) {
    this.selectedFeedbackId = id;
  }

  // Snapshot of the current editor selection. The editor calls this on every
  // selection change; the agent pane links feedback cards whose source range
  // overlaps the selection. A collapsed selection is a caret position and
  // still links ranges that contain it.
  setEditorSelection(sel: EditorSelection | null) {
    this.editorSelection = sel;
  }

  // ---- Task lifecycle wiring (Rust event channels) ----
  //
  // Subscribe to the two Rust task-event channels on mount and unsubscribe on
  // destroy. The store maps each TaskEvent and TaskResult to an AgentFeedback
  // item keyed by the task id. The editor calls registerTask right after
  // submitBlock returns so the queued card appears before the running event;
  // it also refines the source range from the paragraph-level provisional
  // range to the focus-line range using the byte offsets from metadata.

  async mount() {
    this.taskSubsAlive = true;
    const unUpdate = await onTaskUpdated((e) => this.applyTaskEvent(e));
    if (!this.taskSubsAlive) {
      try {
        unUpdate();
      } catch {
        // ignore
      }
      return;
    }
    const unResult = await onResultReady((r) => this.applyTaskResult(r));
    if (!this.taskSubsAlive) {
      try {
        unUpdate();
      } catch {
        // ignore
      }
      try {
        unResult();
      } catch {
        // ignore
      }
      return;
    }
    this.taskUnlisteners = [unUpdate, unResult];
  }

  destroy() {
    this.taskSubsAlive = false;
    for (const un of this.taskUnlisteners) {
      try {
        un();
      } catch {
        // ignore
      }
    }
    this.taskUnlisteners = [];
  }

  // Register a task the editor just submitted. Creates a queued item
  // immediately so the card appears before the running event. If an event
  // already arrived (race), upserts the range and kind without downgrading the
  // status.
  registerTask(taskId: string, kind: FeedbackKind, range: SourceRange) {
    const existing = this.feedback.find((f) => f.id === taskId);
    if (existing) {
      this.updateFeedback(taskId, { range, kind });
      return;
    }
    const now = this.timestamp();
    this.addFeedback({
      id: taskId,
      kind,
      status: "queued",
      provider: "mock",
      model: "mock-provider-v1",
      range,
      title: titleForKind(kind),
      summary: "Queued...",
      detail: "",
      createdAt: now,
      updatedAt: now,
    });
  }

  // Handle a TaskEvent (status changed to running). Updates the existing item
  // or adopts via a metadata fetch when the event arrived before registration.
  applyTaskEvent(event: TaskEvent) {
    const kind = requestKindToFeedbackKind(event.kind);
    const status = taskStatusToFeedbackStatus(event.status);
    if (this.feedback.some((f) => f.id === event.taskId)) {
      this.updateFeedback(event.taskId, { status, kind });
    } else {
      void this.adoptTask(event.taskId, kind, status);
    }
  }

  // Handle a TaskResult (completed, stale, or failed). Updates the existing
  // item with the mock result text, or adopts when no prior item exists.
  applyTaskResult(result: TaskResult) {
    const status = taskStatusToFeedbackStatus(result.status);
    const kind = requestKindToFeedbackKind(result.kind);
    const { summary, detail } = describeTaskResult(result.result, result.error);
    if (this.feedback.some((f) => f.id === result.taskId)) {
      this.updateFeedback(result.taskId, { status, summary, detail });
    } else {
      void this.adoptTask(result.taskId, kind, status, summary, detail);
    }
  }

  // Defensive fallback: an event or result arrived before the editor
  // registered the task. Fetch metadata for the file path and create the item
  // with a degenerate range; the editor's registerTask will refine the range.
  private async adoptTask(
    taskId: string,
    kind: FeedbackKind,
    status: FeedbackStatus,
    summary?: string,
    detail?: string,
  ) {
    let file: string | null = null;
    let resolvedKind = kind;
    try {
      const meta = await getTaskStatus(taskId);
      if (meta) {
        file = meta.filePath;
      }
    } catch {
      // metadata fetch failed; use the open file as a best guess
    }
    // The item may have been created by registerTask while this adopt was
    // suspended at the metadata fetch. If so, update the existing item instead
    // of silently no-oping (addFeedback guards on duplicate ids). Also bail
    // when the store was destroyed while suspended.
    if (!this.taskSubsAlive) return;
    if (this.feedback.some((f) => f.id === taskId)) {
      this.updateFeedback(taskId, {
        status,
        kind: resolvedKind,
        ...(summary !== undefined ? { summary } : {}),
        ...(detail !== undefined ? { detail } : {}),
      });
      return;
    }
    const now = this.timestamp();
    this.addFeedback({
      id: taskId,
      kind: resolvedKind,
      status,
      provider: "mock",
      model: "mock-provider-v1",
      range: { file: file ?? this.openFilePath ?? "", from: 0, to: 0 },
      title: titleForKind(resolvedKind),
      summary: summary ?? (status === "running" ? "Running..." : ""),
      detail: detail ?? "",
      createdAt: now,
      updatedAt: now,
    });
  }

  private timestamp(): number {
    return Date.now();
  }

  // Mock re-run of a stale/errored item. Re-queues and simulates the async
  // lifecycle (queued -> running -> arrived) with timers. The real transport
  // will replace this later.
  requeueFeedback(id: string) {
    const item = this.feedback.find((f) => f.id === id);
    if (!item) return;
    this.updateFeedback(id, { status: "queued" });
    window.setTimeout(() => {
      this.updateFeedback(id, { status: "running" });
    }, 700);
    window.setTimeout(() => {
      this.updateFeedback(id, { status: "arrived" });
    }, 1800);
  }

  // Dispatch a mock feedback item for a detected slash command. The range
  // anchors the item to the paragraph the command was typed in. Rust
  // dispatches /fact-check and /research through the task pipeline, so only
  // /ignore falls back here; its card arrives immediately because /ignore is
  // a directive. Unrecognized names do nothing.
  dispatchMockFeedback(command: { name: string }, range: SourceRange): void {
    if (command.name !== "/ignore") return;
    const now = Date.now();
    this.addFeedback({
      id: `mock-ignore-${now}`,
      kind: "ignore",
      status: "arrived",
      provider: "mock",
      model: "mock-1",
      range,
      title: "Ignore: block excluded",
      summary:
        "This block is excluded from automatic research and fact-checking.",
      detail:
        "Marked as ignored. No agent activity will run on this block. Remove the /ignore command to re-enable automatic processing.",
      createdAt: now,
      updatedAt: now,
    });
  }

  // Map the source ranges of feedback items for one file through a document
  // change. The editor passes its change set's mapPos, so ranges stay attached
  // to the text they refer to as the user types. Items for other files are
  // untouched. Degenerate ranges (fully deleted text) keep their mapped
  // values; the editor's decoration guard skips from >= to. The store stays
  // CodeMirror-free: the caller owns the mapPos callable.
  mapFeedbackRanges(
    mapPos: (pos: number, assoc: -1 | 1) => number,
    file: string | null
  ): void {
    if (file === null) return;
    this.feedback = this.feedback.map((item) => {
      if (item.range.file !== file) return item;
      let from = mapPos(item.range.from, -1);
      let to = mapPos(item.range.to, 1);
      if (from < 0) from = 0;
      if (to < 0) to = 0;
      return { ...item, range: { ...item.range, from, to } };
    });
  }

  // Demo data so the linked feedback UI can be exercised without an LLM
  // transport. Re-seeding replaces previous demo items.
  seedMockFeedback() {
    const file = this.openFilePath;
    if (!file) return;
    const doc = this.openFileContent;
    this.feedback = this.feedback.filter((f) => !f.id.startsWith("demo-"));
    this.selectedFeedbackId = null;

    const now = Date.now();
    const items: AgentFeedback[] = [
      {
        id: "demo-fact-1",
        kind: "fact-check",
        status: "arrived",
        provider: "openai",
        model: "gpt-5.6-luna",
        range: findRange(file, doc, "fact-check"),
        title: "Fact-check: source claim",
        summary:
          "Checked against 3 web sources. 2 support the claim, 1 is unrelated.",
        detail:
          "Sources checked:\n1. Example source A - supports the claim (score 0.92).\n2. Example source B - supports the claim (score 0.84).\n3. Example source C - unrelated to the claim (ignored).\nThe claim is mostly supported. Consider adding the reference from source A.",
        createdAt: now - 90_000,
        updatedAt: now - 90_000,
      },
      {
        id: "demo-research-1",
        kind: "research",
        status: "running",
        provider: "anthropic",
        model: "claude-opus-4.6",
        range: findRange(file, doc, "research"),
        title: "Research: background material",
        summary: "Searching the web for background sources...",
        detail: "",
        createdAt: now - 12_000,
        updatedAt: now - 4_000,
      },
      {
        id: "demo-correct-1",
        kind: "correction",
        status: "stale",
        provider: "openai",
        model: "gpt-5.6-luna",
        range: findRange(file, doc, "correction"),
        title: "Correction: suggested rewording",
        summary: "Suggested a clearer rewording of this sentence.",
        detail:
          "Original: the sentence as written.\nSuggested: a clearer rewording that keeps the same meaning.",
        createdAt: now - 600_000,
        updatedAt: now - 300_000,
      },
      {
        id: "demo-error-1",
        kind: "fact-check",
        status: "error",
        provider: "anthropic",
        model: "claude-opus-4.6",
        range: findRange(file, doc, "error"),
        title: "Fact-check: failed",
        summary: "The provider request failed. Retry to attempt again.",
        detail: "Provider request failed: rate limit exceeded (HTTP 429).",
        createdAt: now - 2_000,
        updatedAt: now - 2_000,
      },
    ];
    this.feedback = [...this.feedback, ...items];

    // Simulate the running research item completing.
    window.setTimeout(() => {
      this.updateFeedback("demo-research-1", {
        status: "arrived",
        detail:
          "Background sources found:\n1. A 2023 overview article on the topic.\n2. A primary source document.\nBoth are ready to cite in the note.",
      });
    }, 3500);
  }

  async openFile(path: string) {
    if (this.openFilePath === path && !this.dirty) return;
    this.loading = true;
    this.error = null;
    const startGen = this.gen;
    try {
      const content = await readTextFile(path);
      if (startGen !== this.gen) return;
      this.openFilePath = path;
      // A freshly read file is external content: the editor's doc-sync effect
      // must replace the document with it, never keep the previous buffer.
      this.contentOrigin = "external";
      this.openFileContent = content;
      this.dirty = false;
    } catch (e) {
      if (startGen !== this.gen) return;
      this.error = String(e);
    } finally {
      if (startGen === this.gen) this.loading = false;
    }
  }

  // Persist an arbitrary file's content to disk. Serialized through the same
  // promise chain as saveOpenFile so writes to the same file never interleave;
  // the last caller's content wins. Records the write in selfWrites so the
  // watcher recognizes it as our own. It does NOT touch openFileContent, dirty,
  // or contentOrigin: it also runs for a file that is no longer open (the
  // editor flushes the previous file on switch), and those fields belong to
  // saveOpenFile's gen/path-guarded success path. Returns true on success;
  // failures land in `error`.
  async saveFileContent(path: string, content: string): Promise<boolean> {
    const run = this.saveChain.then(async () => {
      this.saving = true;
      try {
        const hash = await writeTextFile(path, content);
        this.selfWrites[path] = hash;
        return true;
      } catch (e) {
        this.error = String(e);
        return false;
      } finally {
        this.saving = false;
      }
    });
    // Keep the chain alive: a queued save runs after this one settles, and a
    // rejection never leaks past the chain (all errors are captured above).
    this.saveChain = run.then(
      () => undefined,
      () => undefined
    );
    return run;
  }

  // Persist the current editor buffer to disk. Returns true on success.
  // Delegates the write to saveFileContent (shared serializer). The success
  // path applies the result to the open-file state only when the generation is
  // unchanged and the open file still matches the saved path, so a file switch
  // or project change mid-save must not clobber the new file's state. The
  // "editor" content origin marks this content change as our own save so the
  // doc-sync effect keeps the local buffer instead of replacing it.
  async saveOpenFile(content: string): Promise<boolean> {
    const path = this.openFilePath;
    if (!path) return false;
    const startGen = this.gen;
    const ok = await this.saveFileContent(path, content);
    if (ok && startGen === this.gen && this.openFilePath === path) {
      this.contentOrigin = "editor";
      this.openFileContent = content;
      this.dirty = false;
    }
    return ok;
  }
}

export const project = new ProjectStore();

// Anchor a demo source range to the first occurrence of a phrase in the open
// document, so decorations land on real words. Falls back to the start of the
// document when the phrase is absent.
function findRange(
  file: string,
  doc: string,
  phrase: string
): { file: string; from: number; to: number } {
  const idx = doc.indexOf(phrase);
  if (idx >= 0) {
    return { file, from: idx, to: idx + phrase.length };
  }
  return { file, from: 0, to: Math.min(24, doc.length) };
}