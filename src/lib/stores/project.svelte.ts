import { readTextFile } from "../tauri";
import type { ChangeEvent } from "../types";

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
  watching = $state<boolean>(loadRoot() !== null);
  changeFeed = $state<ChangeEvent[]>([]);
  structureVersion = $state<number>(0);

  private gen = 0;

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
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // ignore
    }
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