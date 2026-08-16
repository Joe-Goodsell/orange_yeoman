import {
  loadProjectConfig as loadProjectConfigCmd,
  readTextFile,
} from "../tauri";
import type {
  AgentFeedback,
  ChangeEvent,
  ConfigStatus,
  EditorSelection,
} from "../types";

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
  loading = $state<boolean>(false);
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
  // (same file only). Drives the "linked" highlight in the agent pane.
  linkedFeedbackIds = $derived(
    (() => {
      const sel = this.editorSelection;
      if (!sel || !this.openFilePath) return [];
      return this.feedback
        .filter(
          (f) =>
            f.range.file === this.openFilePath &&
            f.range.from < sel.to &&
            f.range.to > sel.from
        )
        .map((f) => f.id);
    })()
  );

  private gen = 0;

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
    // Feedback ranges are anchored to the snapshot of their file. When that
    // file changes on disk (external edit), the item can no longer be
    // presented as authoritative; mark it stale so the pane says so and the
    // user can re-run it. `error` items already communicate a failure.
    if (e.kind === "content") {
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
  // selection change; collapsed selections clear the link.
  setEditorSelection(sel: EditorSelection | null) {
    this.editorSelection = sel;
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
      this.openFileContent = content;
      this.dirty = false;
    } catch (e) {
      if (startGen !== this.gen) return;
      this.error = String(e);
    } finally {
      if (startGen === this.gen) this.loading = false;
    }
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