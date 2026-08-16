import {
  loadProjectConfig as loadProjectConfigCmd,
  readTextFile,
} from "../tauri";
import type { ChangeEvent, ConfigStatus } from "../types";

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