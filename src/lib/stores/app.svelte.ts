import { loadProjectConfig as loadProjectConfigCmd } from "../tauri";
import type { ConfigStatus } from "../types";

const STORAGE_KEY = "orange-yeoman:project-root";

function loadRoot(): string | null {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

class AppStore {
  projectRoot = $state<string | null>(loadRoot());
  watching = $state<boolean>(loadRoot() !== null);
  configStatus = $state<ConfigStatus | null>(null);
  error = $state<string | null>(null);

  // Guards against stale responses: a root change while an IPC call is in
  // flight makes the older response irrelevant.
  private gen = 0;

  applyConfigStatus(status: ConfigStatus) {
    this.configStatus = status;
  }

  // Load (root = path) or clear (root = null) the project config in Rust and
  // refresh the merged config status. The response is ignored if the root
  // changed while the IPC call was in flight.
  async loadProjectConfig(root: string | null): Promise<void> {
    const startGen = this.gen;
    try {
      const status = await loadProjectConfigCmd(root);
      if (startGen !== this.gen) return;
      this.applyConfigStatus(status);
    } catch (e) {
      if (startGen !== this.gen) return;
      this.setError(String(e));
    }
  }

  selectRoot(path: string) {
    this.gen++;
    this.projectRoot = path;
    this.setError(null);
    try {
      localStorage.setItem(STORAGE_KEY, path);
    } catch {
      // localStorage may be unavailable; ignore
    }
    void this.loadProjectConfig(path);
  }

  clearProject() {
    this.gen++;
    this.projectRoot = null;
    this.configStatus = null;
    this.setError(null);
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // ignore
    }
    void this.loadProjectConfig(null);
  }

  setWatching(v: boolean) {
    this.watching = v;
  }

  setError(e: string | null) {
    this.error = e;
  }
}

export const appStore = new AppStore();